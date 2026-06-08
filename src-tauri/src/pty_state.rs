//! Registry-shaped PTY state owned by the Tauri app via `.manage()`.
//!
//! M1 only ever holds ONE live PTY, but the state is shaped as a *registry*
//! (`HashMap<PtyId, PtyState>`) on purpose: M2 introduces a real multi-session
//! `SessionRegistry`, and keeping the key/lookup shape here now means the bridge
//! commands do not have to change when that lands (ADR-4).
//!
//! This is NOT the `spectty-core` `SessionRegistry` — it imports nothing from
//! core. It is a Tauri-side holding pen for the OS-level PTY handles plus the
//! read-thread bookkeeping that lets `pty_kill`/`Drop` shut a thread down
//! without leaking it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use spectty_adapters::PtyTransport;

/// Identifier for a live PTY. A plain `String` in M1 (a monotonic counter
/// rendered as text); M2 can swap the minting strategy without touching callers.
pub type PtyId = String;

/// The Tauri-side handle to one live PTY.
///
/// `transport` is a boxed [`PtyTransport`] (not a concrete `PtyAdapter`) so the
/// command layer operates on `&mut dyn PtyTransport` and unit tests can insert a
/// fake without opening a real pseudo-terminal. `stop` signals the dedicated
/// read thread to exit; `reader_thread` is joined on shutdown so no thread leaks.
pub struct PtyState {
    /// Write/resize/kill side of the PTY (real adapter in production, fake in tests).
    pub transport: Box<dyn PtyTransport>,
    /// Cooperative stop flag observed by the read loop.
    pub stop: Arc<AtomicBool>,
    /// Handle to the dedicated read thread, taken and joined on shutdown.
    pub reader_thread: Option<JoinHandle<()>>,
}

impl PtyState {
    /// Signal the read thread to stop, kill the child (which closes the master
    /// and unblocks a blocking read), and join the thread. Best-effort: errors
    /// are swallowed because this runs on the shutdown/`Drop` path where there is
    /// no caller to report to and a half-torn-down PTY must not panic the app.
    ///
    /// Idempotent: the `stop` flag is used as a one-shot latch so an explicit
    /// `pty_kill` followed by `Drop` does NOT kill the child (or join) twice. The
    /// `swap` returns the previous value; if it was already `true` the PTY has
    /// already been torn down and there is nothing left to do.
    pub fn shutdown(&mut self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        // Killing the child closes the slave; the master then reports EOF so a
        // read blocked in the dedicated thread returns and the loop exits.
        let _ = self.transport.kill();
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PtyState {
    fn drop(&mut self) {
        // Guard against a leaked read thread if a PtyState is dropped without an
        // explicit `pty_kill` (e.g. app teardown or a removed-then-dropped entry).
        self.shutdown();
    }
}

/// Registry of live PTYs managed by the Tauri app.
///
/// Registry-shaped for the M2 `SessionRegistry` seam (one entry in M1). The
/// `Mutex` guards concurrent access from command handlers and the read threads.
/// A poisoned lock is surfaced as an error at the command boundary rather than
/// panicking, so a crashed PTY thread cannot brick the UI.
#[derive(Default)]
pub struct PtyRegistry(pub Mutex<HashMap<PtyId, PtyState>>);
