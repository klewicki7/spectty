//! The OutputSignal status pipeline — the read loop's SECOND consumer (D9/D10).
//!
//! M1's read thread streams raw PTY bytes to the renderer (read thread → `mpsc` →
//! forwarder → [`Coalescer`](spectty_adapters::Coalescer) → `pty_output` Channel).
//! M2 adds a SECOND, independent consumer that turns the same byte stream into
//! agent status transitions, WITHOUT ever throttling the renderer.
//!
//! ```text
//! read thread ──┬─ tx_render.send(slice)    → forwarder → Coalescer → pty_output  (M1, UNCHANGED, never blocked)
//!               └─ signal_try_send(slice)    → signal thread → producer.ingest → observe_and_diff → emit
//! ```
//!
//! The render `tx` keeps its M1 UNBOUNDED behavior so rendering is never starved.
//! The signal `tx` is a BOUNDED [`sync_channel`] (drop-oldest, [`signal_try_send`])
//! so a slow signal thread can NEVER back-pressure the read thread and therefore can
//! never back-pressure the renderer (R6/D9). Status detection only needs the LATEST
//! window, so dropping an older slice is harmless.
//!
//! ## What lives here in PR5a
//!
//! - [`observe_and_diff`] — the PURE detect→transition→diff step (9.3), mirroring
//!   M1's `forward_step` testability discipline.
//! - [`signal_try_send`] / [`signal_channel`] — the bounded drop-oldest seam (9.4).
//! - [`run_signal_loop`] — the THIRD thread's loop: `recv_timeout(QUIESCE)` ticks so
//!   `idle_ms`/`is_active` advance while the PTY is quiescent (the M1 R3 insight),
//!   `producer.ingest` → `clock.now()` stamp → `producer.snapshot` → `observe_and_diff`
//!   → emit, and on EOF a FINAL terminal signal carrying `exit_code` (9.5).
//!
//! ## Emit seam (PR5a → PR5b boundary)
//!
//! [`run_signal_loop`] does NOT touch Tauri. It calls an injected
//! `emit: impl FnMut(StatusChanged)` on every actual status change, so the whole
//! pipeline is unit-testable with NO `AppHandle`. PR5b's `spawn_session` supplies a
//! closure that does `app.emit("status_changed", payload)` and registers the
//! `status_changed` event — that is the ONLY wiring PR5b adds on top of this.

use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::Duration;

use spectty_adapters::OutputSignalProducer;
use spectty_core::ports::clock::ClockPort;
use spectty_core::{
    AgentRunner, AgentStatus, OutputSignal, QuickAction, SessionId, SessionRegistry,
};

/// Bound on the signal tee channel. Status detection only needs the LATEST window,
/// so a small buffer is enough; on overflow the read thread DROPS the slice rather
/// than block (D9). 64 mirrors the design's suggested capacity.
pub const SIGNAL_CHANNEL_CAP: usize = 64;

/// Quiescent tick interval. When the PTY is silent, the signal thread still wakes
/// every `QUIESCE` to re-snapshot so `idle_ms`/`is_active` advance and an idle
/// timeout can fire — the exact quiescent-flush insight M1 used for its renderer
/// (R3), reused here for status detection.
pub const QUIESCE: Duration = Duration::from_millis(200);

/// Window byte cap for the per-session [`OutputSignalProducer`]. Large enough to
/// hold the scrollback a detector pattern needs, bounded so a long-running agent
/// never grows the window unboundedly (drop-oldest front).
pub const SIGNAL_WINDOW_BYTES: usize = 8 * 1024;

/// The payload emitted on an actual status change. Mirrors the design's
/// `StatusChanged` event shape; PR5b serializes it over the Tauri `status_changed`
/// event. Kept here (not in `commands/`) so the pipeline owns its own output type
/// and stays Tauri-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusChanged {
    /// The session whose status changed (== PtyId, D13).
    pub session_id: SessionId,
    /// The NEW status the Core `transition` produced.
    pub status: AgentStatus,
    /// Quick actions the runner offers for the new status (empty in M2 — skeleton).
    pub quick_actions: Vec<QuickAction>,
}

/// PURE decision for one signal tick (mirrors M1's `forward_step`): given the
/// runner's observation of `signal` and the registry's current status for `id`,
/// decide whether the status CHANGED.
///
/// Returns `Some(new_status)` only when the status actually changed (→ the caller
/// emits `status_changed`), and `None` when there is nothing to emit:
/// - `detect_status` returned `None` (no confident observation this tick), OR
/// - `apply_observed` returned `None` (a legal no-op, a terminal-absorbing
///   observation, or an absent session).
///
/// The transition itself happens INSIDE the registry lock (D19) via
/// [`SessionRegistry::apply_observed`], so the diff is atomic with respect to a
/// concurrent `close`/`remove`. This free-fn shape is what makes the
/// detect→transition→emit wiring unit-testable without a thread or a PTY.
#[must_use]
pub fn observe_and_diff(
    runner: &dyn AgentRunner,
    sessions: &SessionRegistry,
    id: &SessionId,
    signal: &OutputSignal,
) -> Option<AgentStatus> {
    let observed = runner.detect_status(signal)?;
    sessions.apply_observed(id, observed)
}

/// Build the bounded, drop-oldest signal tee channel (D9). The read thread holds
/// the [`SyncSender`] and feeds it with [`signal_try_send`]; the signal thread owns
/// the [`Receiver`].
#[must_use]
pub fn signal_channel(cap: usize) -> (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) {
    std::sync::mpsc::sync_channel(cap)
}

/// Tee one raw slice onto the bounded signal channel WITHOUT ever blocking (D9).
///
/// This is the R6 render-protection seam: a [`SyncSender::send`] would BLOCK the
/// read thread once the buffer fills, which would in turn stall the renderer tee.
/// Instead we [`try_send`](SyncSender::try_send) and DROP the slice when the buffer
/// is full — status detection only needs the most recent window, so an older slice
/// is expendable. Returns `true` if the slice was queued, `false` if it was dropped
/// (full buffer) or the signal thread is gone (disconnected). The read thread
/// ignores the result either way — it must NEVER act on signal back-pressure.
pub fn signal_try_send(tx: &SyncSender<Vec<u8>>, slice: Vec<u8>) -> bool {
    match tx.try_send(slice) {
        Ok(()) => true,
        // Full buffer (drop-oldest: we simply discard the NEW slice) or the signal
        // thread has exited. Either way the read thread does not block.
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
    }
}

/// One outcome of the signal thread's `recv_timeout(QUIESCE)` loop, resolved into a
/// pure action. Extracted as a value (computed by [`signal_step`]) so the
/// ingest/quiesce/EOF policy is unit-testable WITHOUT a thread, a clock, or a PTY —
/// the same discipline M1 used for `forward_step`.
#[derive(Debug, PartialEq, Eq)]
enum SignalAction {
    /// A slice arrived: ingest it (`is_active` = true this tick), then snapshot.
    Ingest(Vec<u8>),
    /// The PTY was quiescent for one `QUIESCE` interval: snapshot WITHOUT ingest
    /// (`is_active` = false), advancing `idle_ms` so an idle-timeout can fire.
    Quiesce,
    /// The read thread dropped its sender (EOF/error): emit the FINAL terminal
    /// snapshot (with `exit_code` already marked) and stop.
    Eof,
}

/// Pure mapping from a `recv_timeout` outcome to a [`SignalAction`].
fn signal_step(outcome: Result<Vec<u8>, RecvTimeoutError>) -> SignalAction {
    match outcome {
        Ok(slice) => SignalAction::Ingest(slice),
        Err(RecvTimeoutError::Timeout) => SignalAction::Quiesce,
        Err(RecvTimeoutError::Disconnected) => SignalAction::Eof,
    }
}

/// Run the signal thread's loop until the read thread disconnects (EOF/error).
///
/// On each [`recv_timeout`](Receiver::recv_timeout):
/// - **slice** → `producer.ingest(slice)`, stamp `last_byte_at = clock.now()`,
///   snapshot with `is_active = true` and `idle_ms = 0` (output just arrived);
/// - **timeout (quiescent)** → snapshot with `is_active = false` and
///   `idle_ms = now - last_byte_at` so an idle-timeout detector (Generic
///   exit-criterion 5) can fire while the PTY is silent (the M1 R3 insight);
/// - **disconnect (EOF)** → mark the producer's `exit_code`, take ONE FINAL
///   quiescent-then-terminal pass, and stop.
///
/// Every actual status change ([`observe_and_diff`] returns `Some`) is reported via
/// the injected `emit` closure — the Tauri-free seam PR5b wires to `app.emit`.
///
/// ## Fast-exit ordering (carry-forward from the PR2a fresh review)
///
/// The transition table FORBIDS `Starting -> Completed`: a process that exits while
/// the session is still `Starting` would no-op on the terminal observation and the
/// UI would never leave `Starting`. So on EOF this loop emits a QUIESCENT
/// (`is_active = false`, no exit code) snapshot FIRST — which lets the runner
/// observe `Ready`/`Finished-from-idle` and the registry reach a non-`Starting`
/// state (`Idle`, or `Running` via any earlier output) — and ONLY THEN marks the
/// exit code and emits the TERMINAL snapshot. That guarantees a reachable
/// `Starting -> Idle/Running -> Completed` path even for a command that exits almost
/// immediately. The ordering is enforced HERE (the EOF arm), backed by
/// `OutputSignalProducer::mark_exit` never erasing the window.
pub fn run_signal_loop(
    rx: &Receiver<Vec<u8>>,
    runner: &dyn AgentRunner,
    sessions: &SessionRegistry,
    id: &SessionId,
    clock: &dyn ClockPort,
    exit_code_on_eof: impl Fn() -> i32,
    mut emit: impl FnMut(StatusChanged),
) {
    let mut producer = OutputSignalProducer::new(SIGNAL_WINDOW_BYTES);
    // Last instant a byte was seen, so quiescent ticks can compute a real `idle_ms`.
    let mut last_byte_at = clock.now();

    loop {
        let outcome = rx.recv_timeout(QUIESCE);
        match signal_step(outcome) {
            SignalAction::Ingest(slice) => {
                producer.ingest(&slice);
                last_byte_at = clock.now();
                let signal = producer.snapshot(last_byte_at, 0, true);
                emit_on_change(runner, sessions, id, &signal, &mut emit);
            }
            SignalAction::Quiesce => {
                let now = clock.now();
                let idle_ms = last_byte_at.elapsed_ms_until(now);
                let signal = producer.snapshot(last_byte_at, idle_ms, false);
                emit_on_change(runner, sessions, id, &signal, &mut emit);
            }
            SignalAction::Eof => {
                // FAST-EXIT ORDERING (see the rustdoc above): a quiescent snapshot
                // FIRST so a `Starting` session can reach `Idle`/`Running` before the
                // terminal observation (the table forbids `Starting -> Completed`)...
                let now = clock.now();
                let idle_ms = last_byte_at.elapsed_ms_until(now);
                let quiescent = producer.snapshot(last_byte_at, idle_ms, false);
                emit_on_change(runner, sessions, id, &quiescent, &mut emit);

                // ...THEN the terminal snapshot carrying the exit code, so the
                // session reaches `Completed`/`Error` from a legal source state.
                producer.mark_exit(exit_code_on_eof());
                let terminal = producer.snapshot(now, idle_ms, false);
                emit_on_change(runner, sessions, id, &terminal, &mut emit);
                break;
            }
        }
    }
}

/// Run [`observe_and_diff`] for one snapshot and emit on an actual change.
fn emit_on_change(
    runner: &dyn AgentRunner,
    sessions: &SessionRegistry,
    id: &SessionId,
    signal: &OutputSignal,
    emit: &mut impl FnMut(StatusChanged),
) {
    if let Some(status) = observe_and_diff(runner, sessions, id, signal) {
        emit(StatusChanged {
            session_id: id.clone(),
            status,
            quick_actions: runner.quick_actions(&status),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectty_core::entities::agent_spec::{AgentKind, AgentSpec, AgentTier};
    use spectty_core::entities::agent_status::Observed;
    use spectty_core::entities::session::Session;
    use spectty_core::entities::workspace::WorkspaceId;
    use spectty_core::ports::agent_runner::{LaunchContext, LaunchSpec};
    use spectty_core::ports::clock::{ClockPort, Timestamp};
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A scripted runner: pops one `Option<Observed>` per `detect_status` call, in
    /// order, so a test can drive the pure pipeline through an exact observation
    /// sequence WITHOUT a real agent or any pattern scraping.
    struct ScriptedRunner {
        script: Cell<usize>,
        observations: Vec<Option<Observed>>,
    }

    impl ScriptedRunner {
        fn new(observations: Vec<Option<Observed>>) -> Self {
            Self {
                script: Cell::new(0),
                observations,
            }
        }
    }

    // `Cell` is not `Sync`; the pure-pipeline tests never share a `ScriptedRunner`
    // across threads, so this unsafe impl is sound for the test harness only.
    unsafe impl Sync for ScriptedRunner {}

    impl AgentRunner for ScriptedRunner {
        fn launch_spec(&self, _ctx: &LaunchContext) -> LaunchSpec {
            unreachable!("launch_spec is not exercised by the runtime pipeline tests")
        }

        fn detect_status(&self, _signal: &OutputSignal) -> Option<Observed> {
            let i = self.script.get();
            self.script.set(i + 1);
            self.observations.get(i).copied().flatten()
        }

        fn descriptor(&self) -> spectty_core::AgentDescriptor {
            unreachable!("descriptor is not exercised by the runtime pipeline tests")
        }

        fn tier(&self) -> AgentTier {
            AgentTier::Generic
        }
    }

    /// A clock that returns a fixed, manually-advanced millis value so quiescent
    /// `idle_ms` math is deterministic under test.
    struct FakeClock(AtomicU64);

    impl FakeClock {
        fn at(ms: u64) -> Self {
            Self(AtomicU64::new(ms))
        }
    }

    impl ClockPort for FakeClock {
        fn now(&self) -> Timestamp {
            Timestamp(self.0.load(Ordering::SeqCst))
        }
    }

    fn registry_with(status: AgentStatus) -> (SessionRegistry, SessionId) {
        let registry = SessionRegistry::default();
        let id = registry.mint_id();
        registry.insert(Session {
            id: id.clone(),
            workspace: WorkspaceId("/repo".to_string()),
            agent: AgentSpec {
                kind: AgentKind("generic".to_string()),
                command: None,
                tier: AgentTier::Generic,
            },
            status,
            title: "t".to_string(),
            created_at: Timestamp(0),
        });
        (registry, id)
    }

    fn any_signal() -> OutputSignal {
        OutputSignal {
            text_window: String::new(),
            is_active: false,
            exit_code: None,
            last_byte_at: Timestamp(0),
            idle_ms: 0,
        }
    }

    // WU-9.2 RED → 9.3 GREEN: observe_and_diff emits ONLY on an actual change.
    #[test]
    fn observe_and_diff_emits_only_on_change() {
        // The runner observes Ready, then Ready again. From `Starting`, the first
        // Ready transitions to `Idle` (a CHANGE → Some), the second Ready is a legal
        // no-op (`Idle + Ready = Idle` → None).
        let runner = ScriptedRunner::new(vec![Some(Observed::Ready), Some(Observed::Ready)]);
        let (sessions, id) = registry_with(AgentStatus::Starting);

        assert_eq!(
            observe_and_diff(&runner, &sessions, &id, &any_signal()),
            Some(AgentStatus::Idle),
            "the first Ready must change Starting -> Idle and be emitted"
        );
        assert_eq!(
            observe_and_diff(&runner, &sessions, &id, &any_signal()),
            None,
            "a second Ready is a legal no-op (Idle stays Idle) → no emit"
        );
    }

    #[test]
    fn observe_and_diff_none_when_detect_status_is_none() {
        // `detect_status` returning None (no confident observation) must short-circuit
        // BEFORE touching the registry — nothing to diff, nothing to emit.
        let runner = ScriptedRunner::new(vec![None]);
        let (sessions, id) = registry_with(AgentStatus::Starting);

        assert_eq!(
            observe_and_diff(&runner, &sessions, &id, &any_signal()),
            None,
            "no observation this tick → no emit"
        );
        assert_eq!(
            sessions.get(&id).expect("present").status,
            AgentStatus::Starting,
            "a None observation must not mutate the session status"
        );
    }

    // WU-9.4 RED → 9.5 GREEN: the bounded signal channel drops on overflow and the
    // try_send side NEVER blocks the read thread.
    #[test]
    fn bounded_signal_channel_drops_oldest_never_blocks() {
        let cap = 2;
        let (tx, rx) = signal_channel(cap);

        // Fill the buffer to capacity: both sends are accepted without a receiver
        // draining, proving `try_send` does not block on a free slot.
        assert!(signal_try_send(&tx, b"a".to_vec()), "first send fits");
        assert!(signal_try_send(&tx, b"b".to_vec()), "second send fits");

        // The buffer is now FULL. A real `send` would BLOCK here forever (no
        // receiver is draining) — but `signal_try_send` must return immediately,
        // dropping the slice, so the read thread is never stalled (R6/D9).
        assert!(
            !signal_try_send(&tx, b"c".to_vec()),
            "an over-capacity send must DROP (return false), never block the read thread"
        );

        // The dropped slice ("c") is gone; the buffered ones survive in FIFO order.
        assert_eq!(rx.recv().unwrap(), b"a".to_vec());
        assert_eq!(rx.recv().unwrap(), b"b".to_vec());
        assert!(
            rx.try_recv().is_err(),
            "the over-capacity slice was dropped, not queued"
        );
    }

    #[test]
    fn signal_try_send_returns_false_when_receiver_dropped() {
        // A disconnected channel (signal thread gone) must also not block or panic —
        // the read thread just gets `false` and carries on rendering.
        let (tx, rx) = signal_channel(4);
        drop(rx);
        assert!(
            !signal_try_send(&tx, b"x".to_vec()),
            "a disconnected signal channel must drop, not block or panic"
        );
    }

    // WU-9.5: the signal loop emits a status change on ingest, then terminates on EOF
    // emitting a terminal status — exercising the full ingest → snapshot →
    // observe_and_diff → emit → EOF path with NO Tauri and NO real PTY.
    #[test]
    fn signal_loop_emits_change_then_terminal_on_eof() {
        let (sessions, id) = registry_with(AgentStatus::Starting);
        // The real GenericRunner: active output → Working; clean exit → Finished.
        let runner = spectty_adapters::GenericRunner::new(3_000, |_| None);
        let clock = FakeClock::at(0);

        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);
        // One slice of output, then drop the sender to signal EOF.
        signal_try_send(&tx, b"building...\n".to_vec());
        drop(tx);

        let mut emitted: Vec<StatusChanged> = Vec::new();
        run_signal_loop(
            &rx,
            &runner,
            &sessions,
            &id,
            &clock,
            || 0, // clean exit
            |sc| emitted.push(sc),
        );

        // The ingest tick (active output) drives Starting -> Running; the EOF
        // terminal tick (exit 0) drives Running -> Completed.
        let statuses: Vec<AgentStatus> = emitted.iter().map(|sc| sc.status).collect();
        assert_eq!(
            statuses,
            vec![AgentStatus::Running, AgentStatus::Completed],
            "ingest emits Running, EOF emits the terminal Completed"
        );
        assert!(
            emitted.iter().all(|sc| sc.session_id == id),
            "every emitted change must carry the session id"
        );
        assert_eq!(
            sessions.get(&id).expect("present").status,
            AgentStatus::Completed
        );
    }

    // CARRY-FORWARD (PR2a review): a process that exits almost immediately from
    // `Starting` must still reach a non-`Starting` state BEFORE the terminal
    // observation, because the table forbids `Starting -> Completed`. The EOF arm's
    // quiescent-then-terminal ordering must therefore produce
    // `Starting -> Idle -> Completed`, never a stuck `Starting`.
    #[test]
    fn fast_exit_from_starting_reaches_idle_before_completed() {
        let (sessions, id) = registry_with(AgentStatus::Starting);
        let runner = spectty_adapters::GenericRunner::new(3_000, |_| None);
        // No prior output at all: the session is still `Starting` when EOF hits.
        let clock = FakeClock::at(0);

        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);
        drop(tx); // immediate EOF, session never left Starting

        let mut emitted: Vec<StatusChanged> = Vec::new();
        run_signal_loop(
            &rx,
            &runner,
            &sessions,
            &id,
            &clock,
            || 0,
            |sc| emitted.push(sc),
        );

        let statuses: Vec<AgentStatus> = emitted.iter().map(|sc| sc.status).collect();
        assert_eq!(
            statuses,
            vec![AgentStatus::Idle, AgentStatus::Completed],
            "EOF must emit a quiescent Idle FIRST (Starting->Idle), THEN Completed \
             (Idle->Completed) — never a stuck Starting (table forbids Starting->Completed)"
        );
    }

    #[test]
    fn signal_step_maps_outcomes_to_actions() {
        assert_eq!(
            signal_step(Ok(b"x".to_vec())),
            SignalAction::Ingest(b"x".to_vec())
        );
        assert_eq!(
            signal_step(Err(RecvTimeoutError::Timeout)),
            SignalAction::Quiesce
        );
        assert_eq!(
            signal_step(Err(RecvTimeoutError::Disconnected)),
            SignalAction::Eof
        );
    }

    /// WU-11.1 — roadmap exit-criterion 5 in MINIATURE against a REAL PTY.
    ///
    /// Spawns a deterministic Generic command (`/bin/sh -c "printf 'hi\n'; sleep
    /// 0.2"`), drives the REAL M1 read loop → the bounded signal tee → the real
    /// [`OutputSignalProducer`] → `GenericRunner::detect_status` → the Core
    /// transition, and asserts the session reaches `Running` (active output) then
    /// `Completed` (clean exit). It uses the EOF/exit path, NOT a wall-clock idle
    /// timeout, so it is fast (~0.2s) and deterministic — no flaky idle dependency.
    ///
    /// This is the end-to-end RED→GREEN for the whole PR5a pipeline: it exercises the
    /// exact `signal_channel` + `run_signal_loop` wiring production uses, only with
    /// the emit callback collecting into a Vec instead of `app.emit`.
    #[cfg(unix)]
    #[test]
    fn real_pty_generic_reaches_running_then_completed() {
        use spectty_adapters::{PtyAdapter, PtySpawnConfig, PtyTransport};
        use std::io::Read;

        // A deterministic Generic command: print a marker, briefly sleep, then exit 0.
        // The sleep keeps the child alive long enough for one active-output ingest
        // tick before EOF, so the pipeline observes Running THEN Completed.
        let cfg = PtySpawnConfig {
            program: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), "printf 'hi\\n'; sleep 0.2".to_string()],
            cwd: None,
            cols: 80,
            rows: 24,
        };
        let (mut adapter, mut reader) = PtyAdapter::spawn(&cfg).expect("real pty spawns");

        let (sessions, id) = registry_with(AgentStatus::Starting);
        let runner = spectty_adapters::GenericRunner::new(3_000, |_| None);
        let clock = spectty_adapters::SystemClock::new();

        // REAL read thread: blocking reads → tee each slice onto the bounded signal
        // channel via the production `signal_try_send`. On EOF it drops `tx`, which
        // the signal loop observes as the terminal `Disconnected`.
        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);
        let reader_handle = std::thread::spawn(move || {
            let mut buf = [0u8; 8 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = signal_try_send(&tx, buf[..n].to_vec());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            // Dropping `tx` here disconnects the signal loop → terminal snapshot.
        });

        // Drive the REAL signal loop in this thread; collect emitted changes.
        let mut emitted: Vec<StatusChanged> = Vec::new();
        run_signal_loop(
            &rx,
            &runner,
            &sessions,
            &id,
            &clock,
            || 0,
            |sc| emitted.push(sc),
        );

        let _ = adapter.kill();
        let _ = reader_handle.join();

        let statuses: Vec<AgentStatus> = emitted.iter().map(|sc| sc.status).collect();
        // The marker output drives Starting -> Running; the clean exit drives
        // -> Completed. Other statuses (a quiescent Running no-op) emit nothing.
        assert!(
            statuses.contains(&AgentStatus::Running),
            "active real-PTY output must reach Running; got {statuses:?}"
        );
        assert_eq!(
            statuses.last(),
            Some(&AgentStatus::Completed),
            "a clean real-PTY exit must reach Completed last; got {statuses:?}"
        );
        assert_eq!(
            sessions.get(&id).expect("present").status,
            AgentStatus::Completed
        );
    }
}
