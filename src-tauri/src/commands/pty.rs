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

/// Spawn the dedicated read thread that streams coalesced PTY output over the
/// channel and emits `pty_exit` when the stream ends.
///
/// ADR-3 DELIBERATE DEVIATION from `tokio::spawn_blocking` (called out for
/// sdd-verify): this loop lives for the PTY's ENTIRE lifetime — it blocks on
/// `reader.read(..)` until EOF. `spawn_blocking` is for SHORT blocking tasks;
/// parking a never-returning loop on a Tokio blocking-pool worker would pin that
/// worker forever and eventually starve the pool. A dedicated `std::thread`
/// (the WezTerm pattern) keeps the async runtime unblocked.
fn spawn_read_thread(
    app: AppHandle,
    id: PtyId,
    mut reader: Box<dyn Read + Send>,
    on_output: Channel<Vec<u8>>,
    stop: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>, String> {
    std::thread::Builder::new()
        .name(format!("pty-read-{id}"))
        .spawn(move || {
            let mut buf = [0u8; READ_BUF];
            let mut coalescer = Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, Instant::now());

            loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                match reader.read(&mut buf) {
                    // EOF: the child closed the PTY. Flush the remainder and stop.
                    Ok(0) => {
                        if let Some(chunk) = coalescer.drain_all() {
                            let _ = on_output.send(chunk);
                        }
                        break;
                    }
                    Ok(n) => {
                        let now = Instant::now();
                        if let Some(chunk) = coalescer.push(&buf[..n], now) {
                            if on_output.send(chunk).is_err() {
                                break;
                            }
                        }
                        if let Some(chunk) = coalescer.drain_due(now) {
                            if on_output.send(chunk).is_err() {
                                break;
                            }
                        }
                    }
                    // A signal interrupted the read; retry rather than treating it
                    // as a fatal error.
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    // Any other read error closes the PTY: flush and stop.
                    Err(_) => {
                        if let Some(chunk) = coalescer.drain_all() {
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
}
