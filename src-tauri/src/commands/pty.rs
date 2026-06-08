//! PTY lifecycle commands exposed to the frontend.
//!
//! Four `#[tauri::command]`s drive the live terminal:
//! - `pty_spawn` — open a real PTY, start the dedicated read thread that streams
//!   coalesced raw bytes over an `ipc::Channel<Vec<u8>>`, and return the id.
//! - `send_input` / `pty_resize` / `pty_kill` — operate on the transport stored
//!   in the registry, looked up by id.
//!
//! The command *bodies* delegate to small free functions that take the registry
//! mutex directly. That split is deliberate: the free functions are unit-tested
//! against a `FakePtyTransport` (no real PTY), while the thin `#[tauri::command]`
//! wrappers only adapt Tauri's `State`/`AppHandle`/`Channel` to them.

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use spectty_adapters::{Coalescer, PtyAdapter, PtySpawnConfig};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use crate::pty_state::{PtyId, PtyRegistry, PtyState};

/// Read buffer size for one PTY read syscall. Sized to the coalescer chunk so a
/// single full read maps to roughly one flushed chunk.
const READ_BUF: usize = 8 * 1024;
/// Size threshold at which the coalescer flushes (bytes).
const MAX_CHUNK: usize = 8 * 1024;
/// Time threshold at which the coalescer flushes buffered bytes even if small.
const FLUSH_INTERVAL: Duration = Duration::from_millis(8);

/// Lifecycle event emitted when a PTY's child process exits.
///
/// This is the ONLY event the bridge emits — all high-frequency output goes over
/// the per-spawn `ipc::Channel`. `code` is the child's exit status when known.
#[derive(Clone, serde::Serialize)]
pub struct PtyExit {
    /// The id of the PTY whose child exited.
    pub id: PtyId,
    /// The child's exit code, if the platform reported one.
    pub code: Option<i32>,
}

type Registry = Mutex<HashMap<PtyId, PtyState>>;

/// Lock the registry, mapping a poisoned mutex to a boundary error string.
///
/// A poisoned lock means a read thread panicked while holding it; the design
/// chooses to surface that as a command error rather than re-panic, so one dead
/// PTY cannot crash the whole UI.
fn lock_registry(
    registry: &Registry,
) -> Result<std::sync::MutexGuard<'_, HashMap<PtyId, PtyState>>, String> {
    registry
        .lock()
        .map_err(|_| "pty registry mutex poisoned".to_string())
}

/// Forward input bytes to the PTY identified by `id`.
fn send_input_impl(registry: &Registry, id: &str, data: &[u8]) -> Result<(), String> {
    let mut guard = lock_registry(registry)?;
    let state = guard
        .get_mut(id)
        .ok_or_else(|| format!("unknown pty id: {id}"))?;
    state.transport.write(data).map_err(|e| e.to_string())
}

/// Resize the PTY identified by `id` (raises SIGWINCH for the child).
fn resize_impl(registry: &Registry, id: &str, cols: u16, rows: u16) -> Result<(), String> {
    let mut guard = lock_registry(registry)?;
    let state = guard
        .get_mut(id)
        .ok_or_else(|| format!("unknown pty id: {id}"))?;
    state
        .transport
        .resize(cols, rows)
        .map_err(|e| e.to_string())
}

/// Kill the PTY identified by `id`: shut down its read thread and remove it from
/// the registry. Removing the entry drops its `PtyState`, whose `Drop`/`shutdown`
/// joins the thread (idempotent with the explicit `shutdown` here).
fn kill_impl(registry: &Registry, id: &str) -> Result<(), String> {
    let mut state = {
        let mut guard = lock_registry(registry)?;
        guard
            .remove(id)
            .ok_or_else(|| format!("unknown pty id: {id}"))?
    };
    // Drop the guard before joining the thread: the read loop may itself try to
    // lock the registry, so holding the lock across a join could deadlock.
    state.shutdown();
    Ok(())
}

/// Open a PTY, start its read thread, register it, and return its id.
///
/// `async` + owned argument types only (Tauri requirement for async commands).
#[tauri::command]
pub async fn pty_spawn(
    app: AppHandle,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    on_output: Channel<Vec<u8>>,
    registry: State<'_, PtyRegistry>,
) -> Result<PtyId, String> {
    let cfg = PtySpawnConfig::shell(cols, rows, cwd, |k| std::env::var(k).ok());
    let (adapter, reader) = PtyAdapter::spawn(&cfg).map_err(|e| e.to_string())?;

    // Mint an id. A monotonic counter rendered as a string is enough for M1's
    // single session; M2's SessionRegistry will own real id minting.
    let id: PtyId = next_pty_id();

    let stop = Arc::new(AtomicBool::new(false));
    let reader_thread = spawn_read_thread(app, id.clone(), reader, on_output, Arc::clone(&stop))?;

    let state = PtyState {
        transport: Box::new(adapter),
        stop,
        reader_thread: Some(reader_thread),
    };

    lock_registry(&registry.0)?.insert(id.clone(), state);
    Ok(id)
}

/// Forward keystrokes/input bytes to a live PTY.
#[tauri::command]
pub fn send_input(
    id: PtyId,
    data: Vec<u8>,
    registry: State<'_, PtyRegistry>,
) -> Result<(), String> {
    send_input_impl(&registry.0, &id, &data)
}

/// Resize a live PTY's window, raising SIGWINCH for the child program.
#[tauri::command]
pub fn pty_resize(
    id: PtyId,
    cols: u16,
    rows: u16,
    registry: State<'_, PtyRegistry>,
) -> Result<(), String> {
    resize_impl(&registry.0, &id, cols, rows)
}

/// Terminate a live PTY and clean up its read thread.
#[tauri::command]
pub fn pty_kill(id: PtyId, registry: State<'_, PtyRegistry>) -> Result<(), String> {
    kill_impl(&registry.0, &id)
}

/// What the forwarder loop must do after one `recv_timeout` outcome.
///
/// Extracting the per-message decision into this pure value (computed by
/// [`forward_step`]) is what makes the quiescent-flush fix unit-testable WITHOUT
/// a real PTY, a thread, or a sleep: a test feeds outcomes and asserts the
/// returned action.
#[derive(Debug, PartialEq, Eq)]
enum ForwardAction {
    /// Keep looping. `Some(chunk)` is output to send over the channel now;
    /// `None` means nothing to send this step.
    Continue(Option<Vec<u8>>),
    /// The read side disconnected (EOF/error): send the final remainder (if any),
    /// emit `pty_exit`, and stop the forwarder.
    Exit(Option<Vec<u8>>),
}

/// Pure decision for one forwarder step. Given a `recv_timeout` outcome and the
/// current time, decide what to flush.
///
/// THIS is the R3 fix: a `Timeout` (the PTY went quiet) drives `drain_due`, so
/// bytes buffered by a small write are flushed within `FLUSH_INTERVAL` even
/// though no further read ever unblocks. The old code only called `drain_due`
/// inside the read-return branch, stranding those bytes until the next read.
///
/// - `Ok(bytes)` → push (size-threshold flush on the hot path).
/// - `Err(Timeout)` → time-flush any stranded bytes.
/// - `Err(Disconnected)` → drain everything left and signal exit.
fn forward_step(
    coalescer: &mut Coalescer,
    outcome: Result<Vec<u8>, RecvTimeoutError>,
    now: Instant,
) -> ForwardAction {
    match outcome {
        Ok(bytes) => ForwardAction::Continue(coalescer.push(&bytes, now)),
        Err(RecvTimeoutError::Timeout) => ForwardAction::Continue(coalescer.drain_due(now)),
        Err(RecvTimeoutError::Disconnected) => ForwardAction::Exit(coalescer.drain_all()),
    }
}

/// Spawn the read thread (and the forwarder thread it owns) that stream coalesced
/// PTY output over the channel and emit `pty_exit` when the stream ends.
///
/// ## Why two threads (the R3 fix)
///
/// Reading is DECOUPLED from coalescing via an `mpsc` channel so a time-flush can
/// fire even while the PTY is silent:
/// - The **read thread** does the blocking `reader.read(..)` and forwards each
///   slice over the `mpsc`; on EOF/error it drops its `Sender`, which the
///   forwarder observes as `Disconnected`.
/// - The **forwarder thread** owns the [`Coalescer`] + the output [`Channel`] +
///   the [`AppHandle`] and loops on `rx.recv_timeout(FLUSH_INTERVAL)`. A
///   `Timeout` drives `drain_due`, flushing bytes a quiescent PTY would otherwise
///   strand (an `ESC[6n` DSR query, a prompt, a tab-completion) — that is the
///   bug PR4 caught (design risk R3).
///
/// ## ADR-3 DELIBERATE DEVIATION from `tokio::spawn_blocking` (called out for
/// sdd-verify)
///
/// Both loops live for the PTY's ENTIRE lifetime — the read loop blocks on
/// `reader.read(..)` until EOF and the forwarder blocks on `recv_timeout` until
/// disconnect. `spawn_blocking` is for SHORT blocking tasks; parking a
/// never-returning loop on a Tokio blocking-pool worker would pin that worker
/// forever and eventually starve the pool. Dedicated `std::thread`s (the WezTerm
/// pattern) keep the async runtime unblocked.
///
/// ## Lifecycle / no leak
///
/// The returned `JoinHandle` is the READ thread; it OWNS the forwarder's
/// `JoinHandle` and joins it before returning, so joining the read thread (via
/// `PtyState::shutdown`) tears down BOTH. `stop` short-circuits the read loop,
/// and dropping the read thread's `Sender` disconnects the forwarder so it drains
/// and exits — neither thread can leak.
fn spawn_read_thread(
    app: AppHandle,
    id: PtyId,
    mut reader: Box<dyn Read + Send>,
    on_output: Channel<Vec<u8>>,
    stop: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>, String> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    // Forwarder thread: owns the coalescer, the output channel, and the app
    // handle. It is the only side that flushes and emits `pty_exit`.
    let forwarder = std::thread::Builder::new()
        .name(format!("pty-forward-{id}"))
        .spawn(move || {
            let mut coalescer = Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, Instant::now());
            loop {
                let outcome = rx.recv_timeout(FLUSH_INTERVAL);
                match forward_step(&mut coalescer, outcome, Instant::now()) {
                    ForwardAction::Continue(Some(chunk)) => {
                        if on_output.send(chunk).is_err() {
                            break;
                        }
                    }
                    ForwardAction::Continue(None) => {}
                    ForwardAction::Exit(remainder) => {
                        if let Some(chunk) = remainder {
                            let _ = on_output.send(chunk);
                        }
                        break;
                    }
                }
            }
            // Lifecycle: tell the UI the PTY ended. Exit code is not retrievable
            // from the read side without owning the child handle, so M1 reports
            // `None`; the child is reaped via `kill`/`Drop` on the registry side.
            let _ = app.emit("pty_exit", PtyExit { id, code: None });
        })
        .map_err(|e| format!("failed to spawn pty forwarder thread: {e}"))?;

    // Read thread: blocking reads → forward each slice over the mpsc. Owns and
    // joins the forwarder so a single `JoinHandle` tears down both threads.
    std::thread::Builder::new()
        .name("pty-read".to_string())
        .spawn(move || {
            let mut buf = [0u8; READ_BUF];
            loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                match reader.read(&mut buf) {
                    // EOF: the child closed the PTY. Dropping `tx` below
                    // disconnects the forwarder, which drains the remainder.
                    Ok(0) => break,
                    Ok(n) => {
                        // Send-error means the forwarder is gone; stop reading.
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    // A signal interrupted the read; retry rather than treating it
                    // as a fatal error.
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    // Any other read error closes the PTY: stop and let the
                    // forwarder drain on disconnect.
                    Err(_) => break,
                }
            }
            // Drop the sender so the forwarder sees `Disconnected`, drains the
            // final bytes, and emits `pty_exit`; then join it so neither leaks.
            drop(tx);
            let _ = forwarder.join();
        })
        .map_err(|e| format!("failed to spawn pty read thread: {e}"))
}

/// Mint a process-unique PTY id. Monotonic counter rendered as a decimal string.
fn next_pty_id() -> PtyId {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectty_adapters::{PtyError, PtyTransport};

    /// Calls recorded by the fake transport. Shared via `Arc` so the test keeps a
    /// handle to inspect what the command forwarded after the boxed fake is moved
    /// into the registry.
    #[derive(Default)]
    struct FakeCalls {
        writes: Vec<Vec<u8>>,
        resizes: Vec<(u16, u16)>,
        kills: u32,
    }

    /// A recording fake standing in for a real PTY transport so the command-layer
    /// logic can be exercised with NO pseudo-terminal opened. It records into a
    /// shared `Arc<Mutex<FakeCalls>>` (rather than into `self`) so the test can
    /// read the calls back without downcasting a `Box<dyn PtyTransport>`.
    struct FakePtyTransport(Arc<Mutex<FakeCalls>>);

    impl PtyTransport for FakePtyTransport {
        fn write(&mut self, data: &[u8]) -> Result<(), PtyError> {
            self.0
                .lock()
                .expect("calls not poisoned")
                .writes
                .push(data.to_vec());
            Ok(())
        }
        fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtyError> {
            self.0
                .lock()
                .expect("calls not poisoned")
                .resizes
                .push((cols, rows));
            Ok(())
        }
        fn kill(&mut self) -> Result<(), PtyError> {
            self.0.lock().expect("calls not poisoned").kills += 1;
            Ok(())
        }
    }

    /// Build a registry holding a single fake-backed PTY state under `id`, and
    /// return the shared call-recorder handle alongside it. `reader_thread` is
    /// `None` so `shutdown`/`Drop` has no thread to join.
    fn registry_with_fake(id: &str) -> (Registry, Arc<Mutex<FakeCalls>>) {
        let calls = Arc::new(Mutex::new(FakeCalls::default()));
        let mut map = HashMap::new();
        map.insert(
            id.to_string(),
            PtyState {
                transport: Box::new(FakePtyTransport(Arc::clone(&calls))),
                stop: Arc::new(AtomicBool::new(false)),
                reader_thread: None,
            },
        );
        (Mutex::new(map), calls)
    }

    #[test]
    fn send_input_writes_bytes_to_transport() {
        let (registry, calls) = registry_with_fake("p1");

        send_input_impl(&registry, "p1", b"ls -la\n").expect("send_input ok");

        assert_eq!(
            calls.lock().unwrap().writes,
            vec![b"ls -la\n".to_vec()],
            "send_input must forward the exact bytes to the transport"
        );
    }

    #[test]
    fn pty_resize_forwards_cols_rows() {
        let (registry, calls) = registry_with_fake("p1");

        resize_impl(&registry, "p1", 120, 40).expect("resize ok");

        assert_eq!(
            calls.lock().unwrap().resizes,
            vec![(120, 40)],
            "pty_resize must forward cols then rows, untransposed"
        );
    }

    #[test]
    fn pty_kill_invokes_transport_kill_and_removes_entry() {
        let (registry, calls) = registry_with_fake("p1");

        kill_impl(&registry, "p1").expect("kill ok");

        assert_eq!(
            calls.lock().unwrap().kills,
            1,
            "pty_kill must invoke the transport's kill exactly once"
        );

        // The entry is removed (its Drop already ran), so the PTY is gone.
        let guard = registry.lock().expect("registry not poisoned");
        assert!(
            guard.get("p1").is_none(),
            "pty_kill must remove the entry from the registry"
        );
    }

    #[test]
    fn send_input_unknown_id_returns_err() {
        let (registry, _calls) = registry_with_fake("p1");

        let result = send_input_impl(&registry, "does-not-exist", b"x");

        assert!(
            result.is_err(),
            "an unknown pty id must return Err, not panic"
        );
        assert!(
            result.unwrap_err().contains("unknown pty id"),
            "the error message must identify the unknown-id failure"
        );
    }

    /// W1 closure (from the PR1 verify report): the command-fake tests above guard
    /// the registry/dispatch logic, but they do NOT exercise the REAL
    /// `PtyAdapter::write`/`resize`/`kill` against an actual pseudo-terminal, so a
    /// cols/rows transposition or a broken writer would only surface at manual
    /// acceptance. This test opens a REAL PTY on the CI runner, drives the same
    /// read/coalesce loop the read thread uses, and asserts both that real output
    /// bytes are received AND that real resize/write/kill succeed against the live
    /// master.
    ///
    /// Deterministic and CI-safe: it runs a non-interactive `printf` (Unix) /
    /// `echo` (Windows) of a fixed marker and waits for EOF, no TTY interaction.
    /// R3 REGRESSION (the bug PR4 caught): when the PTY goes quiet, a `Timeout`
    /// from `recv_timeout` MUST drive a time-flush so buffered-but-stranded bytes
    /// reach the UI within `FLUSH_INTERVAL`. The old code only called `drain_due`
    /// inside the read-return branch, so a small write followed by silence (e.g.
    /// an `ESC[6n` DSR query, a prompt, or a tab-completion) was withheld until
    /// the next read unblocked — breaking atuin/fancy prompts and autocomplete.
    ///
    /// This test drives the forwarder's pure decision step directly: first a
    /// small chunk arrives (buffered, not yet flushed because it is under the
    /// size threshold), then SILENCE (a `Timeout`). The `Timeout` step alone must
    /// emit the stranded bytes — no further input required.
    #[test]
    fn quiescent_timeout_flushes_stranded_bytes_within_interval() {
        let t0 = Instant::now();
        let mut coalescer = Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, t0);

        // A small write arrives (e.g. the shell emits `ESC[6n` then blocks on
        // input). It is under MAX_CHUNK, so push buffers it without flushing.
        let on_data = forward_step(&mut coalescer, Ok(b"\x1b[6n".to_vec()), t0);
        assert_eq!(
            on_data,
            ForwardAction::Continue(None),
            "a small write under the size threshold must buffer, not flush yet"
        );

        // Now the PTY is SILENT: recv_timeout returns Timeout. The forwarder must
        // flush the stranded bytes because FLUSH_INTERVAL has elapsed — WITHOUT
        // any further read. This is the fix for R3.
        let on_timeout = forward_step(
            &mut coalescer,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout),
            t0 + FLUSH_INTERVAL,
        );
        assert_eq!(
            on_timeout,
            ForwardAction::Continue(Some(b"\x1b[6n".to_vec())),
            "a quiescent Timeout must flush the stranded bytes within FLUSH_INTERVAL"
        );
    }

    /// Triangulation 1: a `Disconnected` (read thread dropped the sender on
    /// EOF/error) must drain ALL remaining bytes and signal exit, so the final
    /// partial output is never lost and `pty_exit` is emitted.
    #[test]
    fn disconnect_drains_all_and_signals_exit() {
        let t0 = Instant::now();
        let mut coalescer = Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, t0);

        // Buffer a small tail that never reached the size/time threshold.
        let _ = forward_step(&mut coalescer, Ok(b"bye".to_vec()), t0);

        let action = forward_step(
            &mut coalescer,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected),
            t0,
        );
        assert_eq!(
            action,
            ForwardAction::Exit(Some(b"bye".to_vec())),
            "disconnect must drain the remainder and signal exit"
        );
    }

    /// Triangulation 2: a large incoming chunk that meets the size threshold must
    /// flush immediately on the data path (not wait for a timeout), proving the
    /// `Ok` branch still honors the size policy.
    #[test]
    fn oversized_data_flushes_on_size_threshold() {
        let t0 = Instant::now();
        let mut coalescer = Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, t0);

        let big = vec![b'x'; MAX_CHUNK];
        let action = forward_step(&mut coalescer, Ok(big.clone()), t0);
        assert_eq!(
            action,
            ForwardAction::Continue(Some(big)),
            "a chunk at the size threshold must flush immediately on the data path"
        );
    }

    /// Triangulation 3: a `Timeout` with an EMPTY buffer must emit nothing and
    /// keep looping — the quiescent flush must never produce a spurious empty
    /// chunk when there is nothing stranded.
    #[test]
    fn quiescent_timeout_with_empty_buffer_emits_nothing() {
        let t0 = Instant::now();
        let mut coalescer = Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, t0);

        let action = forward_step(
            &mut coalescer,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout),
            t0 + FLUSH_INTERVAL * 10,
        );
        assert_eq!(
            action,
            ForwardAction::Continue(None),
            "a timeout with an empty buffer must not emit a spurious chunk"
        );
    }

    #[cfg(unix)]
    #[test]
    fn real_pty_streams_output_and_accepts_resize_write_kill() {
        use spectty_adapters::{PtyAdapter, PtySpawnConfig, PtyTransport};

        // A shell that prints a deterministic marker and exits. Using the shell's
        // own builtin keeps this hermetic (no reliance on a specific echo binary).
        let marker = "SPECTTY_PTY_OK";
        let cfg = PtySpawnConfig {
            program: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), format!("printf '{marker}'")],
            cwd: None,
            cols: 80,
            rows: 24,
        };

        let (mut adapter, mut reader) = PtyAdapter::spawn(&cfg).expect("real pty spawns");

        // Real resize against the live master must succeed (this is the path W1
        // flagged as untested) — cols/rows forwarded without transposition.
        adapter
            .resize(100, 30)
            .expect("real pty resize succeeds against the live master");

        // A zero-byte write must round-trip through the real writer.
        adapter.write(b"").expect("real pty write succeeds");

        // Drive the exact coalesce loop the read thread uses and collect output
        // until EOF (the child exits after printing the marker).
        let mut coalescer = Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, Instant::now());
        let mut collected: Vec<u8> = Vec::new();
        let mut buf = [0u8; READ_BUF];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    if let Some(chunk) = coalescer.drain_all() {
                        collected.extend_from_slice(&chunk);
                    }
                    break;
                }
                Ok(n) => {
                    let now = Instant::now();
                    if let Some(chunk) = coalescer.push(&buf[..n], now) {
                        collected.extend_from_slice(&chunk);
                    }
                    if let Some(chunk) = coalescer.drain_due(now) {
                        collected.extend_from_slice(&chunk);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    if let Some(chunk) = coalescer.drain_all() {
                        collected.extend_from_slice(&chunk);
                    }
                    break;
                }
            }
        }

        let output = String::from_utf8_lossy(&collected);
        assert!(
            output.contains(marker),
            "real PTY output must contain the printed marker; got: {output:?}"
        );

        // Kill must succeed against the real child (idempotent after natural exit).
        adapter.kill().expect("real pty kill succeeds");
    }

    /// R3 END-TO-END (the quiescent stall, proven against a REAL PTY): a single
    /// SMALL write with NO trailing burst — the child writes one short marker and
    /// then BLOCKS on stdin (`read`) — must still be delivered through the read +
    /// forwarder pipeline within a bounded time. Under the old code the marker
    /// was stranded in the coalescer because the read thread blocked and the
    /// time-flush never fired. Here we wire the real read thread to an mpsc and
    /// drive the forwarder loop's decision on a `recv_timeout`, asserting the
    /// lone small write surfaces via the Timeout-driven flush WITHOUT EOF.
    #[cfg(unix)]
    #[test]
    fn real_pty_lone_small_write_is_not_stranded_while_quiescent() {
        use spectty_adapters::{PtyAdapter, PtySpawnConfig};
        use std::sync::mpsc;

        let marker = "Q"; // one byte: well under MAX_CHUNK, so only a time-flush can deliver it
        let cfg = PtySpawnConfig {
            program: "/bin/sh".to_string(),
            // Print one short marker, then block on stdin so the PTY goes QUIET
            // (no EOF, no further output) — exactly the stall condition.
            args: vec!["-c".to_string(), format!("printf '{marker}'; exec cat")],
            cwd: None,
            cols: 80,
            rows: 24,
        };

        let (mut adapter, mut reader) = PtyAdapter::spawn(&cfg).expect("real pty spawns");

        // Read thread: blocking reads → forward each slice over the mpsc. Mirrors
        // production `spawn_read_thread`. It will block in `cat` after the marker.
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let reader_handle = std::thread::spawn(move || {
            let mut buf = [0u8; READ_BUF];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });

        // Forwarder side: drive the real decision step. Collect until the marker
        // appears via a Timeout-driven flush (NOT via EOF — the child is alive).
        let mut coalescer = Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, Instant::now());
        let mut collected: Vec<u8> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while collected.is_empty() && Instant::now() < deadline {
            let outcome = rx.recv_timeout(FLUSH_INTERVAL);
            match forward_step(&mut coalescer, outcome, Instant::now()) {
                ForwardAction::Continue(Some(chunk)) => collected.extend_from_slice(&chunk),
                ForwardAction::Continue(None) => {}
                ForwardAction::Exit(remainder) => {
                    if let Some(chunk) = remainder {
                        collected.extend_from_slice(&chunk);
                    }
                    break;
                }
            }
            // Ignore a Disconnected-style break only on real disconnect; here the
            // child stays alive in `cat`, so we rely on the Timeout flush.
            if matches!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected)) {
                break;
            }
        }

        // Stop the child and join the reader so the test leaks nothing.
        adapter.kill().expect("real pty kill succeeds");
        let _ = reader_handle.join();

        let output = String::from_utf8_lossy(&collected);
        assert!(
            output.contains(marker),
            "a lone small write must reach the forwarder via the quiescent time-flush \
             (no EOF, no further input); got: {output:?}"
        );
    }
}
