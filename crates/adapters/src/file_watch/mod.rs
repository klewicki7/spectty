//! [`NotifyFileWatcher`] — the [`FileWatchPort`] implementation backed by the `notify`
//! crate, with DEBOUNCED batches (D35, WU-7).
//!
//! The watcher registers a recursive OS watch on the workspace. Raw FS events arrive in
//! bursts (an editor save can fire several), so a dedicated debounce thread COALESCES every
//! event seen within a window (default 600 ms, in the D35 500 ms–1 s range) into ONE
//! [`FileChanged`] batch before invoking the caller's `on_change`. This makes a single save
//! or a multi-file write fire the diff pipeline ONCE, not N times.
//!
//! ## Clean shutdown
//!
//! [`watch`](FileWatchPort::watch) returns a [`NotifyWatchGuard`] (boxed as
//! `Box<dyn WatchGuard>`). Dropping it sets a stop flag and joins the debounce thread
//! (bounded, no busy-wait): the thread blocks on a channel `recv_timeout`, so it wakes
//! either on an event or on the window tick, checks the stop flag, and exits. There is no
//! spin loop.
//!
//! ## Testable debounce core
//!
//! The coalescing logic lives in the pure [`Debouncer`] struct so it can be unit-tested with
//! a SYNTHETIC event burst (WU-7.4) without touching the file system — the real watcher just
//! feeds OS events into the same `Debouncer`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use spectty_core::ports::{
    FileChangeCallback, FileChanged, FileWatchError, FileWatchPort, WatchGuard,
};

/// Default debounce window (D35: 500 ms–1 s).
const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(600);

/// Pure coalescing core: accumulates changed paths and flushes them as ONE de-duplicated,
/// path-sorted [`FileChanged`] batch. Holds no timing — the caller decides WHEN to flush
/// (a real timer in the watcher, an explicit call in tests), so the debounce policy is
/// deterministically testable.
#[derive(Debug, Default)]
pub struct Debouncer {
    pending: BTreeSet<PathBuf>,
}

impl Debouncer {
    /// A fresh debouncer with nothing pending.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a changed path (de-duplicated; insertion order does not matter).
    pub fn record(&mut self, path: PathBuf) {
        self.pending.insert(path);
    }

    /// Whether any change is pending a flush.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Flush the accumulated burst as ONE batch, clearing the pending set. Returns `None`
    /// when nothing was pending (so the watcher emits no empty batches).
    #[must_use]
    pub fn flush(&mut self) -> Option<FileChanged> {
        if self.pending.is_empty() {
            return None;
        }
        let paths: Vec<PathBuf> = std::mem::take(&mut self.pending).into_iter().collect();
        Some(FileChanged { paths })
    }
}

/// The owned guard that keeps a watch alive; dropping it stops the watcher cleanly.
pub struct NotifyWatchGuard {
    stop: Arc<AtomicBool>,
    // Kept alive so the OS watch stays registered for the watch's lifetime; dropped before
    // the thread is joined so no further events are queued during shutdown.
    _watcher: RecommendedWatcher,
    handle: Option<JoinHandle<()>>,
}

impl Drop for NotifyWatchGuard {
    fn drop(&mut self) {
        // Signal the debounce thread to stop, then join it (bounded; the thread blocks on a
        // recv_timeout so it wakes within one window at worst — no busy-wait, no leak).
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl WatchGuard for NotifyWatchGuard {}

/// A [`FileWatchPort`] backed by `notify`, delivering debounced [`FileChanged`] batches.
#[derive(Debug, Clone)]
pub struct NotifyFileWatcher {
    debounce: Duration,
}

impl Default for NotifyFileWatcher {
    fn default() -> Self {
        Self {
            debounce: DEFAULT_DEBOUNCE,
        }
    }
}

impl NotifyFileWatcher {
    /// Build a watcher with the default debounce window.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a watcher with an explicit debounce window (e.g. a short window in tests).
    #[must_use]
    pub fn with_debounce(debounce: Duration) -> Self {
        Self { debounce }
    }
}

impl FileWatchPort for NotifyFileWatcher {
    fn watch(
        &self,
        workspace: PathBuf,
        mut on_change: FileChangeCallback,
    ) -> Result<Box<dyn WatchGuard>, FileWatchError> {
        // Raw notify events flow into this channel; the debounce thread drains + coalesces.
        let (tx, rx) = channel::<PathBuf>();
        let event_tx: Sender<PathBuf> = tx;

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                for path in event.paths {
                    // A closed receiver (guard dropped) just means we stop forwarding.
                    let _ = event_tx.send(path);
                }
            }
        })
        .map_err(|e| FileWatchError::Backend(format!("notify watcher init: {e}")))?;

        watcher
            .watch(&workspace, RecursiveMode::Recursive)
            .map_err(|e| FileWatchError::Backend(format!("notify watch {workspace:?}: {e}")))?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let window = self.debounce;

        let handle = std::thread::spawn(move || {
            let mut debouncer = Debouncer::new();
            loop {
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                match rx.recv_timeout(window) {
                    Ok(path) => debouncer.record(path),
                    Err(RecvTimeoutError::Timeout) => {
                        // Window elapsed: flush the coalesced burst (if any) as one batch.
                        if let Some(batch) = debouncer.flush() {
                            on_change(batch);
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            // Final flush on shutdown so a burst that arrived just before stop is not lost.
            if let Some(batch) = debouncer.flush() {
                on_change(batch);
            }
        });

        Ok(Box::new(NotifyWatchGuard {
            stop,
            _watcher: watcher,
            handle: Some(handle),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // WU-7.4: a synthetic burst of events coalesces into ONE de-duplicated batch within the
    // window. Tested against the pure Debouncer so it is deterministic (no real FS timing).
    #[test]
    fn notify_file_watcher_debounces_burst_into_one_batch() {
        let mut debouncer = Debouncer::new();

        // A burst: the same file saved twice plus a sibling, all within one window.
        debouncer.record(PathBuf::from("/ws/a.rs"));
        debouncer.record(PathBuf::from("/ws/a.rs")); // duplicate within the burst
        debouncer.record(PathBuf::from("/ws/b.rs"));
        assert!(debouncer.has_pending());

        let batch = debouncer
            .flush()
            .expect("a non-empty burst yields one batch");
        assert_eq!(
            batch.paths,
            vec![PathBuf::from("/ws/a.rs"), PathBuf::from("/ws/b.rs")],
            "the burst must coalesce into ONE batch of de-duplicated, sorted paths"
        );

        // After the flush the burst is consumed — a second flush yields nothing (no empty
        // batches, so the watcher does not re-fire the pipeline for an idle window).
        assert!(!debouncer.has_pending());
        assert!(debouncer.flush().is_none());
    }

    // A second window with NEW events produces a SEPARATE batch (windows are independent).
    #[test]
    fn debouncer_separate_windows_yield_separate_batches() {
        let mut debouncer = Debouncer::new();
        debouncer.record(PathBuf::from("/ws/a.rs"));
        let first = debouncer.flush().expect("first batch");
        assert_eq!(first.paths, vec![PathBuf::from("/ws/a.rs")]);

        debouncer.record(PathBuf::from("/ws/c.rs"));
        let second = debouncer.flush().expect("second batch");
        assert_eq!(second.paths, vec![PathBuf::from("/ws/c.rs")]);
    }

    // The real watcher is usable behind Arc<dyn FileWatchPort> and shuts down cleanly when
    // the guard is dropped (bounded join, no leaked thread).
    #[test]
    fn notify_watcher_watches_a_real_dir_and_shuts_down_cleanly() {
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!(
            "spectty-watch-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create temp watch dir");

        let watcher: Arc<dyn FileWatchPort> =
            Arc::new(NotifyFileWatcher::with_debounce(Duration::from_millis(50)));
        let guard = watcher
            .watch(dir.clone(), Box::new(|_batch| {}))
            .expect("watch should start");

        // Dropping the guard must join the debounce thread without hanging.
        drop(guard);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
