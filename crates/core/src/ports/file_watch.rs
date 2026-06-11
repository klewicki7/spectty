//! [`FileWatchPort`] — the Core seam for watching a workspace and delivering DEBOUNCED
//! file-change batches (D35).
//!
//! The diff pipeline (PR-5) uses this as the GENERIC-tier trigger: when a cooperative agent
//! does NOT emit `spectty_diff` signals (`emits_diff_signals == false`), file-system changes
//! drive the pipeline instead. The watcher coalesces a burst of raw events into ONE
//! [`FileChanged`] batch per debounce window (500 ms–1 s), so a single editor save or a
//! multi-file write fires the pipeline once, not N times.
//!
//! This port is a PURE, **SYNC** trait — `notify` and the debounce timer live in the
//! adapter. Core gains NO `notify` dependency, keeping the R6 quarantine green.
//!
//! ## Lifecycle / clean shutdown
//!
//! [`watch`](FileWatchPort::watch) returns a [`WatchGuard`]: an opaque, owned handle whose
//! `Drop` MUST stop the underlying watcher and join its thread cleanly (no busy-wait, no
//! leaked thread). Dropping the guard is the single, deterministic shutdown seam — mirroring
//! the session-runtime loop-shutdown discipline.

use std::path::PathBuf;

use thiserror::Error;

/// A debounced batch of file-system changes for a watched workspace (D35).
///
/// One `FileChanged` represents a coalesced burst: the de-duplicated set of paths that
/// changed within a single debounce window. The pipeline treats it as a "something changed,
/// re-diff now" signal — it does not need per-event granularity, so the batch is a
/// lightweight set of paths plus the count of raw events that were coalesced (useful for
/// logging/metrics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChanged {
    /// The de-duplicated paths that changed within the debounce window.
    pub paths: Vec<PathBuf>,
}

/// The callback invoked once per debounced batch. `Send` so the watcher can call it from
/// its own thread; `'static` so it can outlive the `watch` call.
pub type FileChangeCallback = Box<dyn FnMut(FileChanged) + Send + 'static>;

/// Errors from a [`FileWatchPort`] implementation (e.g. the path does not exist, or the
/// OS watch could not be registered).
#[derive(Debug, Error)]
pub enum FileWatchError {
    /// The underlying watch backend failed to start.
    #[error("file watch error: {0}")]
    Backend(String),
}

/// An owned handle that keeps a watch alive; dropping it stops the watcher cleanly.
///
/// The adapter boxes its concrete guard as `Box<dyn WatchGuard>`. The trait is empty: its
/// SOLE contract is `Drop` — when the guard is dropped, the watcher thread MUST be signalled
/// to stop and joined (bounded, no busy-wait). `Send` so the guard can be stored alongside
/// the other per-session shutdown handles in the session runtime.
pub trait WatchGuard: Send {}

/// Port for watching a workspace and delivering debounced [`FileChanged`] batches (D35).
///
/// `watch` registers a recursive watch on `workspace` and invokes `on_change` once per
/// debounce window with the coalesced batch. It returns a [`WatchGuard`]; dropping the guard
/// stops the watcher cleanly. The debounce duration is the adapter's concern (500 ms–1 s per
/// D35); Core only fixes the batched contract.
///
/// `&self` + `Send + Sync` so the adapter can be shared behind `Arc<dyn FileWatchPort>`.
pub trait FileWatchPort: Send + Sync {
    /// Begin watching `workspace`; call `on_change` once per debounced batch.
    ///
    /// Returns a guard whose `Drop` performs clean shutdown. The watcher MUST NOT busy-wait
    /// and MUST coalesce a burst of raw events into ONE [`FileChanged`] per window.
    fn watch(
        &self,
        workspace: PathBuf,
        on_change: FileChangeCallback,
    ) -> Result<Box<dyn WatchGuard>, FileWatchError>;
}
