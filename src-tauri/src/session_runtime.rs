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
//! The signal `tx` is a BOUNDED [`sync_channel`] (drop-on-full / drop-NEWEST,
//! [`signal_try_send`]) so a slow signal thread can NEVER back-pressure the read
//! thread and therefore can never back-pressure the renderer (R6/D9). Status
//! detection only needs the LATEST window folded into the rolling buffer, so when
//! the buffer is full we discard the NEWEST slice while the already-buffered slices
//! survive — acceptable because every slice folds into the same rolling window and
//! the dropped bytes were the least-settled ones.
//!
//! ## What lives here in PR5a
//!
//! - [`observe_and_diff`] — the PURE detect→transition→diff step (9.3), mirroring
//!   M1's `forward_step` testability discipline.
//! - [`signal_try_send`] / [`signal_channel`] — the bounded drop-on-full / drop-newest
//!   seam (9.4).
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

use spectty_adapters::{event_to_observed, OutputSignalProducer, StateFileReader};
use spectty_core::ports::clock::ClockPort;
use spectty_core::{
    AgentRunner, AgentStatus, OutputSignal, QuickAction, SessionId, SessionRegistry,
};

/// Bound on the signal tee channel. Status detection only needs the LATEST window,
/// so a small buffer is enough; on overflow the read thread DROPS the NEWEST slice
/// rather than block (D9). 64 mirrors the design's suggested capacity.
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
/// `StatusChanged` event shape and is serialized over the Tauri `status_changed`
/// event (WU-9.9). Kept here (not in `commands/`) so the pipeline owns its own
/// output type; it derives `Serialize` (every field is a Core serde type) but NOT
/// any `tauri` trait, so `run_signal_loop` stays Tauri-free — `commands/session.rs`
/// supplies the `app.emit` closure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
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

/// Build the bounded, drop-on-full / drop-newest signal tee channel (D9). The read
/// thread holds the [`SyncSender`] and feeds it with [`signal_try_send`]; the signal
/// thread owns the [`Receiver`].
#[must_use]
pub fn signal_channel(cap: usize) -> (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) {
    std::sync::mpsc::sync_channel(cap)
}

/// Tee one raw slice onto the bounded signal channel WITHOUT ever blocking (D9).
///
/// This is the R6 render-protection seam: a [`SyncSender::send`] would BLOCK the
/// read thread once the buffer fills, which would in turn stall the renderer tee.
/// Instead we [`try_send`](SyncSender::try_send) and DROP the slice when the buffer
/// is full. NOTE on drop semantics: a [`sync_channel`] + `try_send` drops the
/// NEWEST slice on overflow (the already-buffered slices survive in FIFO order),
/// NOT the oldest. That is acceptable here because every slice is folded into the
/// same rolling window by the signal thread, so the buffered (older) slices still
/// reach the window and only the least-settled newest bytes are discarded. Returns
/// `true` if the slice was queued, `false` if it was dropped (full buffer) or the
/// signal thread is gone (disconnected). The read thread ignores the result either
/// way — it must NEVER act on signal back-pressure.
pub fn signal_try_send(tx: &SyncSender<Vec<u8>>, slice: Vec<u8>) -> bool {
    match tx.try_send(slice) {
        Ok(()) => true,
        // Full buffer (drop-on-full: we discard the NEWEST slice; buffered ones
        // survive) or the signal thread has exited. Either way the read thread does
        // not block.
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
/// - **slice** → poll `hook_reader` (hook FIRST per tick, D24); then
///   `producer.ingest(slice)`, stamp `last_byte_at = clock.now()`, snapshot with
///   `is_active = true` and `idle_ms = 0` (output just arrived);
/// - **timeout (quiescent)** → poll `hook_reader` (hook FIRST per tick); then
///   snapshot with `is_active = false` and `idle_ms = now - last_byte_at` so an
///   idle-timeout detector (Generic exit-criterion 5) can fire while the PTY is
///   silent (the M1 R3 insight);
/// - **disconnect (EOF)** → mark the producer's `exit_code`, take ONE FINAL
///   quiescent-then-terminal pass (no hook poll — session is ending), and stop.
///
/// ## Hook-first ordering (D24)
///
/// On each Ingest and Quiesce arm, `hook_reader.poll(read_fn)` is called BEFORE
/// the PTY observation. If the hook returns `Some(event)`, it is mapped through
/// `event_to_observed` and fed into `observe_and_diff` — using the SAME authority
/// as the PTY scraping path (`transition()` unchanged, D24). Double-emit is
/// impossible because `observe_and_diff` only emits on an ACTUAL status change.
///
/// The `read_fn` supplied by the caller is `std::fs::read_to_string` wrapped as a
/// closure that returns `Ok(Some(contents))` / `Ok(None)` / `Err(_)`.  This keeps
/// the loop Tauri-free: `run_signal_loop` never performs I/O directly.
///
/// ## Hook-gate: suppress scraping-derived Ready on Running (C1 fix, D24 option b)
///
/// When `hooks_active` is `true` (hooks are provisioned for this session, i.e. the
/// caller passed a non-empty `hook_runtime_dir` at the wiring site), scraping-derived
/// `Observed::Ready` is suppressed from the `Running` state on the Ingest and Quiesce
/// arms. Only a hook `Stop` event may drive `Running → Idle` in that mode (D24
/// primary fix). This prevents a brief 200ms output pause from spuriously flipping a
/// working agent to Idle before the Stop hook fires.
///
/// Sessions without hooks (`hooks_active = false`, i.e. Generic agents or sessions
/// spawned with an empty `hook_runtime_dir`) keep the M2 stopgap behavior where
/// quiescence drives `Running → Idle` via scraping.
///
/// `hooks_active` MUST be computed at the wiring site as `!hook_runtime_dir.is_empty()`
/// BEFORE constructing the `StateFileReader`. Do NOT derive it from `hook_reader.path()`
/// after construction: `StateFileReader::new("", id)` builds path `"/spectty-{id}.state"`
/// (non-empty), so path-based derivation is ALWAYS true regardless of whether hooks
/// are provisioned — the session-local empty-string convention is lost.
///
/// The EOF arm always applies the quiescent snapshot WITHOUT the gate so a session
/// that exits while Running can still reach Idle → Completed via the normal EOF path.
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
// The `hook_reader` param was added by WU-7 and `hooks_active` by the C1 re-review
// fix; the total is 9 args. `#[allow]` keeps the public API as a free function (not
// a builder or struct) which matches the existing testability discipline — every
// collaborator is explicit in the call site.
#[allow(clippy::too_many_arguments)]
pub fn run_signal_loop(
    rx: &Receiver<Vec<u8>>,
    runner: &dyn AgentRunner,
    sessions: &SessionRegistry,
    id: &SessionId,
    clock: &dyn ClockPort,
    hook_reader: &mut StateFileReader,
    // Computed at the wiring site as `!hook_runtime_dir.is_empty()` BEFORE constructing
    // the `StateFileReader` — see the rustdoc above for why path-based derivation fails.
    hooks_active: bool,
    exit_code_on_eof: impl Fn() -> i32,
    mut emit: impl FnMut(StatusChanged),
) {
    let mut producer = OutputSignalProducer::new(SIGNAL_WINDOW_BYTES);
    // Last instant a byte was seen, so quiescent ticks can compute a real `idle_ms`.
    let mut last_byte_at = clock.now();

    // `hooks_active` is passed in explicitly by the caller — computed at the wiring
    // site as `!hook_runtime_dir.is_empty()`. See rustdoc above: deriving it here
    // from `hook_reader.path()` would always be true because StateFileReader::new("", id)
    // builds path "/spectty-{id}.state" (non-empty), losing the empty-dir convention.

    // The read closure for the hook reader: wraps `std::fs::read_to_string` into
    // the `Fn(&str) -> io::Result<Option<String>>` seam. Absent files → `Ok(None)`.
    let read_state = |path: &str| -> std::io::Result<Option<String>> {
        match std::fs::read_to_string(path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    };

    loop {
        let outcome = rx.recv_timeout(QUIESCE);
        match signal_step(outcome) {
            SignalAction::Ingest(slice) => {
                // Hook FIRST (D24): poll the state file before processing PTY bytes.
                emit_hook_if_present(hook_reader, &read_state, runner, sessions, id, &mut emit);

                producer.ingest(&slice);
                last_byte_at = clock.now();
                let signal = producer.snapshot(last_byte_at, 0, true);
                // C1 FIX: when hooks active, gate out scraping Ready from Running.
                emit_scraping_guarded(runner, sessions, id, &signal, hooks_active, &mut emit);
            }
            SignalAction::Quiesce => {
                // Hook FIRST (D24): poll the state file before the quiescent snapshot.
                emit_hook_if_present(hook_reader, &read_state, runner, sessions, id, &mut emit);

                let now = clock.now();
                let idle_ms = last_byte_at.elapsed_ms_until(now);
                let signal = producer.snapshot(last_byte_at, idle_ms, false);
                // C1 FIX: when hooks active, gate out scraping Ready from Running.
                emit_scraping_guarded(runner, sessions, id, &signal, hooks_active, &mut emit);
            }
            SignalAction::Eof => {
                // FAST-EXIT ORDERING (see the rustdoc above): a quiescent snapshot
                // FIRST so a `Starting` session can reach `Idle`/`Running` before the
                // terminal observation (the table forbids `Starting -> Completed`).
                // NOTE: the EOF quiescent snapshot is NOT hook-gated — on process exit
                // Running→Idle via the quiescent path is correct (the session is ending,
                // no hook Stop will fire). This preserves the Starting→Idle→Completed
                // fast-exit path for Cooperative agents too.
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

/// Poll the hook reader and, if an event is returned, feed it through
/// `observe_and_diff` and emit on an actual status change (D24 hook-first path).
///
/// This helper is called once per Ingest/Quiesce arm, BEFORE the PTY observation.
/// It never panics: absent/malformed state files produce `None` from `poll`.
fn emit_hook_if_present(
    hook_reader: &mut StateFileReader,
    read_state: &dyn Fn(&str) -> std::io::Result<Option<String>>,
    runner: &dyn AgentRunner,
    sessions: &SessionRegistry,
    id: &SessionId,
    emit: &mut impl FnMut(StatusChanged),
) {
    if let Some(hook_event) = hook_reader.poll(read_state) {
        let observed = event_to_observed(hook_event);
        if let Some(status) = sessions.apply_observed(id, observed) {
            emit(StatusChanged {
                session_id: id.clone(),
                status,
                quick_actions: runner.quick_actions(&status),
            });
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

/// C1 FIX (D24 option b): scraping path with hook-gate for the Ingest/Quiesce arms.
///
/// When `hooks_active` is true (the session has hooks provisioned), this function
/// suppresses scraping-derived `Observed::Ready` from the `Running` state so that
/// only a hook `Stop` event may drive `Running → Idle`. All other observations
/// (Working, NeedsInput, Finished, Failed, and Ready from non-Running states) are
/// applied normally — only the specific `(Running, Ready)` transition is gated.
///
/// When `hooks_active` is false (Generic agents, sessions without hooks), this is
/// identical to [`emit_on_change`], preserving the M2 stopgap behavior where
/// quiescence drives `Running → Idle`.
///
/// Design rationale (D24): hooks are the AUTHORITATIVE signal for a Cooperative
/// agent's turn end. A single 200ms quiescent tick is NOT reliable evidence that the
/// agent has finished — Claude Code may pause output briefly mid-computation. The
/// hook `Stop` event fires deterministically when the agent's turn actually ends.
fn emit_scraping_guarded(
    runner: &dyn AgentRunner,
    sessions: &SessionRegistry,
    id: &SessionId,
    signal: &OutputSignal,
    hooks_active: bool,
    emit: &mut impl FnMut(StatusChanged),
) {
    use spectty_core::entities::agent_status::Observed;
    use spectty_core::AgentStatus;

    if !hooks_active {
        // No hooks for this session: use the normal path (M2 stopgap preserved).
        return emit_on_change(runner, sessions, id, signal, emit);
    }

    // Hooks active: gate out scraping-derived Ready from the Running state.
    // Call detect_status directly so we can inspect the observation before applying it.
    let Some(observed) = runner.detect_status(signal) else {
        return; // no confident observation this tick → nothing to do
    };

    if observed == Observed::Ready {
        // Check the session's current status to decide whether to suppress.
        // TOCTOU note: we read status outside the registry lock; if the status changes
        // between the read and the apply_observed call below, the worst case is we
        // allow a Ready to reach a non-Running state (harmless — transition() is a
        // legal no-op or a correct transition for non-Running states).
        let current_status = sessions.get(id).map(|s| s.status);
        if current_status == Some(AgentStatus::Running) {
            // Suppress: only a hook Stop drives Running→Idle when hooks are active.
            return;
        }
    }

    // Apply the observation normally (non-Ready, or Ready from non-Running state).
    if let Some(status) = sessions.apply_observed(id, observed) {
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
    // try_send side NEVER blocks the read thread. NOTE: `sync_channel` + `try_send`
    // drops the NEWEST slice on a full buffer (the buffered ones survive), which is
    // what this test pins — see `signal_try_send`'s drop-semantics rustdoc.
    #[test]
    fn bounded_signal_channel_drops_on_full_never_blocks() {
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
        // No hook file: reader points at a nonexistent path → always returns None.
        let mut hook_reader = StateFileReader::new("/tmp/__no_hook_test_eof", "eof-session");

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
            &mut hook_reader,
            false, // no hooks for this test session
            || 0,  // clean exit
            |sc| emitted.push(sc),
        );

        // The ingest tick (active output) drives Starting -> Running; the EOF quiescent
        // snapshot (GenericRunner sees is_active=false → Ready) now drives Running → Idle
        // (M3 PRIMARY FIX: Running+Ready=Idle); the terminal snapshot drives Idle → Completed.
        let statuses: Vec<AgentStatus> = emitted.iter().map(|sc| sc.status).collect();
        assert!(
            statuses.contains(&AgentStatus::Running),
            "ingest tick must emit Running; got {statuses:?}"
        );
        assert_eq!(
            *statuses.last().expect("non-empty"),
            AgentStatus::Completed,
            "the FINAL status must be Completed; got {statuses:?}"
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
        // No hook file.
        let mut hook_reader = StateFileReader::new("/tmp/__no_hook_fast_exit", "fast-exit-session");

        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);
        drop(tx); // immediate EOF, session never left Starting

        let mut emitted: Vec<StatusChanged> = Vec::new();
        run_signal_loop(
            &rx,
            &runner,
            &sessions,
            &id,
            &clock,
            &mut hook_reader,
            false, // no hooks for this test session
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

    // ── WU-7 GREEN TESTS: hook_reader augmentation (D24) ────────────────────
    //
    // These tests exercise the new `hook_reader` parameter on `run_signal_loop`.
    // They write real temp state files so the loop's fs::read_to_string finds them.

    // WU-7.1: A scripted hook reader returning {Stop, ts:1} from Running state
    // must emit StatusChanged(Idle) on the first Ingest tick.
    #[test]
    fn run_signal_loop_hook_stop_from_running_emits_idle() {
        // Registry in Running state (hook fires Stop → Ready → Idle).
        let (sessions, id) = registry_with(AgentStatus::Running);
        // Scripted runner never detects anything from PTY.
        let runner = ScriptedRunner::new(vec![None, None]);
        let clock = FakeClock::at(0);

        // Write a real temp state file so the loop's fs::read_to_string finds it.
        // Path must match StateFileReader's formula: {runtime_dir}/spectty-{session_id}.state
        let tmp = std::env::temp_dir();
        let state_path = tmp.join("spectty-test-session-71.state");
        std::fs::write(
            &state_path,
            r#"{"event":"Stop","ts":1,"session_id":"test-session-71"}"#,
        )
        .unwrap();

        let runtime_dir = tmp.to_string_lossy().into_owned();
        let mut hook_reader = StateFileReader::new(&runtime_dir, "test-session-71");

        // Send one empty slice (triggers the Ingest arm → hook is polled), then
        // drop tx so the next recv_timeout returns Disconnected → EOF.
        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);
        signal_try_send(&tx, vec![]); // one Ingest tick
        drop(tx);

        let mut emitted: Vec<StatusChanged> = Vec::new();
        run_signal_loop(
            &rx,
            &runner,
            &sessions,
            &id,
            &clock,
            &mut hook_reader,
            true, // hooks are active: runtime_dir is non-empty
            || 0,
            |sc| emitted.push(sc),
        );

        // Clean up temp file.
        let _ = std::fs::remove_file(&state_path);

        // The hook Stop → Ready should have triggered Running → Idle.
        let statuses: Vec<AgentStatus> = emitted.iter().map(|sc| sc.status).collect();
        assert!(
            statuses.contains(&AgentStatus::Idle),
            "hook Stop from Running must emit Idle; got {statuses:?}"
        );
    }

    // WU-7.2: Hook fires Stop (→ Ready → Idle) AND scripted runner also returns
    // Ready on the same tick. Only ONE StatusChanged must be emitted (no double-emit).
    #[test]
    fn run_signal_loop_hook_does_not_double_emit_when_same_tick_scrape_agrees() {
        // Registry starts at Running; hook Stop → Ready and PTY scrape also Ready.
        // First observe_and_diff (hook): Running→Idle=Some. Second (PTY): Idle+Ready=None.
        let (sessions, id) = registry_with(AgentStatus::Running);
        // Scripted runner returns Ready (agreeing with the hook observation).
        let runner = ScriptedRunner::new(vec![
            Some(spectty_core::entities::agent_status::Observed::Ready),
            None,
        ]);
        let clock = FakeClock::at(0);

        let tmp = std::env::temp_dir();
        // Path must match StateFileReader's formula: {runtime_dir}/spectty-{session_id}.state
        let state_path = tmp.join("spectty-session-72.state");
        std::fs::write(
            &state_path,
            r#"{"event":"Stop","ts":1,"session_id":"session-72"}"#,
        )
        .unwrap();

        let runtime_dir = tmp.to_string_lossy().into_owned();
        let mut hook_reader = StateFileReader::new(&runtime_dir, "session-72");

        // Send one empty slice to trigger the Ingest arm where hook is polled.
        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);
        signal_try_send(&tx, vec![]);
        drop(tx);

        let mut emitted: Vec<StatusChanged> = Vec::new();
        run_signal_loop(
            &rx,
            &runner,
            &sessions,
            &id,
            &clock,
            &mut hook_reader,
            true, // hooks are active: runtime_dir is non-empty
            || 0,
            |sc| emitted.push(sc),
        );

        let _ = std::fs::remove_file(&state_path);

        let statuses: Vec<AgentStatus> = emitted.iter().map(|sc| sc.status).collect();
        let idle_count = statuses.iter().filter(|&&s| s == AgentStatus::Idle).count();
        assert_eq!(
            idle_count, 1,
            "Idle must appear EXACTLY ONCE (hook fires first; PTY re-observe is a no-op); got {statuses:?}"
        );
    }

    // WU-7.3 RED: When no hook file is present (reader returns None), the loop must
    // fall through to PTY scraping. A scripted runner returning Ready from Starting
    // must still emit Idle.
    #[test]
    fn run_signal_loop_hook_absent_file_falls_through_to_scraping() {
        let (sessions, id) = registry_with(AgentStatus::Starting);
        // Runner returns Ready on the first observe call (PTY path).
        let runner = ScriptedRunner::new(vec![Some(
            spectty_core::entities::agent_status::Observed::Ready,
        )]);
        let clock = FakeClock::at(0);

        // Build a reader pointing at a nonexistent file.
        let mut hook_reader =
            StateFileReader::new("/tmp/__nonexistent_spectty_dir", "absent-session");

        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);
        drop(tx);

        let mut emitted: Vec<StatusChanged> = Vec::new();
        run_signal_loop(
            &rx,
            &runner,
            &sessions,
            &id,
            &clock,
            &mut hook_reader,
            false, // no hooks for this test (nonexistent runtime dir)
            || 0,
            |sc| emitted.push(sc),
        );

        let statuses: Vec<AgentStatus> = emitted.iter().map(|sc| sc.status).collect();
        assert!(
            statuses.contains(&AgentStatus::Idle),
            "absent hook file must fall through to PTY scraping; got {statuses:?}"
        );
    }

    // WU-7.4 RED: A hook reader whose file contains malformed JSON must not panic and
    // must allow the PTY scraping path to proceed normally.
    #[test]
    fn run_signal_loop_hook_malformed_file_is_silent() {
        let (sessions, id) = registry_with(AgentStatus::Starting);
        let runner = ScriptedRunner::new(vec![Some(
            spectty_core::entities::agent_status::Observed::Ready,
        )]);
        let clock = FakeClock::at(0);

        // Write a malformed state file.
        let tmp = std::env::temp_dir();
        let state_path = tmp.join("spectty-malformed-session.state");
        std::fs::write(&state_path, b"not valid json at all").unwrap();

        let runtime_dir = tmp.to_string_lossy().into_owned();
        let mut hook_reader = StateFileReader::new(&runtime_dir, "malformed-session");

        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);
        drop(tx);

        let mut emitted: Vec<StatusChanged> = Vec::new();
        // Must not panic.
        run_signal_loop(
            &rx,
            &runner,
            &sessions,
            &id,
            &clock,
            &mut hook_reader,
            true, // hooks provisioned (non-empty runtime_dir), but file is malformed
            || 0,
            |sc| emitted.push(sc),
        );

        let _ = std::fs::remove_file(&state_path);

        // PTY scraping still fires normally (Starting→Idle via Ready).
        // Session is Starting (not Running), so the hook gate does not apply here.
        let statuses: Vec<AgentStatus> = emitted.iter().map(|sc| sc.status).collect();
        assert!(
            statuses.contains(&AgentStatus::Idle),
            "malformed hook file must be silent and PTY path must proceed; got {statuses:?}"
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
            env: Vec::new(),
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
        // No hook file for this integration test — reader points at a nonexistent path.
        let mut hook_reader = StateFileReader::new("/tmp/__no_hook_real_pty", "real-pty-session");
        let mut emitted: Vec<StatusChanged> = Vec::new();
        run_signal_loop(
            &rx,
            &runner,
            &sessions,
            &id,
            &clock,
            &mut hook_reader,
            false, // no hooks for this integration test
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

    // ── C1 RED→GREEN: single-tick scraping quiescence must NOT flip Running→Idle
    //    when hooks are active for the session (D24 option b).
    //
    // The adversarial defect: `ClaudeCodeRunner::detect_status` returns
    // `Observed::Ready` on EVERY quiescent tick (is_active=false, no pattern
    // match). With `(Running, Ready) => Idle` in the transition table, a working
    // agent that pauses output for one 200ms QUIESCE tick flips Running→Idle via
    // scraping — BEFORE the hook Stop fires.
    //
    // Fix: when the hook reader's path is non-empty (hooks provisioned for this
    // session), `emit_scraping_guarded` suppresses scraping-derived Ready from
    // the Running state. Only a hook Stop event may drive Running→Idle.
    //
    // This test exercises `emit_scraping_guarded` DIRECTLY (private function,
    // accessible from the same module's test block) to avoid the 200ms QUIESCE
    // timeout required to trigger the loop's Quiesce arm. It uses the REAL
    // `ClaudeCodeRunner` (not ScriptedRunner) and simulates a quiescent signal
    // (is_active=false, no pattern) — the same scenario the Quiesce arm produces.

    #[test]
    fn c1_scraping_quiescence_does_not_flip_running_to_idle_when_hooks_active() {
        use spectty_adapters::ClaudeCodeRunner;
        use spectty_core::ports::clock::Timestamp;

        // Session starts in Running — the agent is mid-turn.
        let (sessions, id) = registry_with(AgentStatus::Running);
        // REAL ClaudeCodeRunner: quiescent signal (is_active=false, no pattern) → Ready.
        let runner = ClaudeCodeRunner::new();

        // A quiescent signal — exactly what the Quiesce arm produces for a silent PTY.
        // ClaudeCodeRunner returns Observed::Ready for is_active=false + no pattern match.
        let quiescent_signal = OutputSignal {
            text_window: "bypass permissions on (shift+tab to cycle)".to_string(),
            is_active: false,
            exit_code: None,
            last_byte_at: Timestamp(0),
            idle_ms: 250, // 250ms quiescent
        };

        let mut emitted: Vec<StatusChanged> = Vec::new();

        // hooks_active = true: non-empty path simulates hooks provisioned for this session.
        emit_scraping_guarded(
            &runner,
            &sessions,
            &id,
            &quiescent_signal,
            true, // hooks_active = true → gate must suppress Running→Idle
            &mut |sc| emitted.push(sc),
        );

        // With hooks active, a quiescent tick must NOT flip Running→Idle.
        assert!(
            emitted.is_empty(),
            "C1: with hooks_active=true, a quiescent Ready observation from Running \
             must be suppressed; got {:?}",
            emitted.iter().map(|sc| sc.status).collect::<Vec<_>>()
        );
        // Session must still be Running — the gate must not have applied any transition.
        assert_eq!(
            sessions.get(&id).expect("present").status,
            AgentStatus::Running,
            "session must remain Running after hook-gated quiescent tick"
        );
    }

    // Complementary test: WITHOUT hooks active (hooks_active=false), the same
    // quiescent signal MUST flip Running→Idle (M2 stopgap preserved).
    #[test]
    fn c1_scraping_quiescence_still_drives_running_to_idle_when_no_hooks() {
        use spectty_adapters::ClaudeCodeRunner;
        use spectty_core::ports::clock::Timestamp;

        // Session starts in Running — the agent is mid-turn.
        let (sessions, id) = registry_with(AgentStatus::Running);
        let runner = ClaudeCodeRunner::new();

        // Same quiescent signal as the hooks-active test.
        let quiescent_signal = OutputSignal {
            text_window: "bypass permissions on (shift+tab to cycle)".to_string(),
            is_active: false,
            exit_code: None,
            last_byte_at: Timestamp(0),
            idle_ms: 250,
        };

        let mut emitted: Vec<StatusChanged> = Vec::new();

        // hooks_active = false: empty path simulates no hooks for this session (M2 stopgap mode).
        emit_scraping_guarded(
            &runner,
            &sessions,
            &id,
            &quiescent_signal,
            false, // hooks_active = false → normal M2 stopgap, no gate
            &mut |sc| emitted.push(sc),
        );

        // M2 stopgap: without hooks, quiescent scraping drives Running→Idle.
        let statuses: Vec<AgentStatus> = emitted.iter().map(|sc| sc.status).collect();
        assert_eq!(
            statuses,
            vec![AgentStatus::Idle],
            "M2 stopgap: with hooks_active=false, quiescent Ready must drive Running→Idle; got {statuses:?}"
        );
    }

    // ── C1 RE-REVIEW RED TEST: empty hook_runtime_dir must NOT suppress scraping ──
    //
    // Defect (confirmed adversarial re-review): `StateFileReader::new("", session_id)`
    // builds path `"/spectty-{id}.state"` — NON-empty — so the gate
    // `let hooks_active = !hook_reader.path().is_empty()` at line 237 is ALWAYS true,
    // even for no-hooks/Generic sessions that pass an empty runtime_dir.
    //
    // Consequence: for Generic sessions the Quiesce arm's `emit_scraping_guarded`
    // call always applies the hook gate, suppressing scraping-derived Ready from the
    // Running state so the Quiesce arm can NEVER drive Running→Idle for these sessions.
    //
    // The session remains Running after the Quiesce arm fires; only the EOF arm
    // (intentionally ungated) can eventually drive Running→Idle. For a long-running
    // PTY process the badge sticks on Running until the process exits.
    //
    // This test MUST FAIL on current code: it exercises the Quiesce arm by holding
    // the channel open for one QUIESCE interval, then checks the status BEFORE
    // dropping tx (before EOF fires). The Quiesce arm must have driven Running→Idle;
    // the always-true gate prevents this on current code.
    //
    // Fix (GREEN step): pass `hooks_active: bool` explicitly into `run_signal_loop`,
    // computed at the wiring site as `!hook_runtime_dir.is_empty()`.
    #[test]
    fn no_hooks_session_quiesce_arm_still_drives_running_to_idle() {
        use spectty_adapters::ClaudeCodeRunner;
        use std::sync::{Arc, Mutex};

        // Generic / no-hooks session: starts Running.
        // The wiring site passes empty runtime_dir → StateFileReader::new("", id)
        // builds path "/spectty-{id}.state" (NON-empty), triggering the always-true
        // hooks_active gate on current code.
        let (sessions, id) = registry_with(AgentStatus::Running);

        let sessions = Arc::new(sessions);
        let id_arc = Arc::new(id);
        let emitted: Arc<Mutex<Vec<StatusChanged>>> = Arc::new(Mutex::new(Vec::new()));

        let sessions_t = Arc::clone(&sessions);
        let id_t = Arc::clone(&id_arc);
        let emitted_t = Arc::clone(&emitted);

        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);

        // Run the signal loop on a background thread. Keep `tx` alive so the loop's
        // recv_timeout fires at least one Quiesce arm tick before EOF.
        let handle = std::thread::spawn(move || {
            // ClaudeCodeRunner: quiescent signal (is_active=false, no pattern) → Ready.
            let runner = ClaudeCodeRunner::new();
            let clock = FakeClock::at(0);
            // EMPTY runtime_dir — exactly what no-hooks/Generic sessions pass at the
            // wiring site (session.rs:559-563). This builds path "/spectty-{id}.state"
            // which is NON-empty, causing hooks_active=true on current code.
            let mut hook_reader = StateFileReader::new("", &id_t.0);
            // hooks_active = !hook_runtime_dir.is_empty() = !"".is_empty() = false
            // This is the correct value for a no-hooks session at the wiring site.
            // On the OLD buggy code: hooks_active = !hook_reader.path().is_empty()
            //   = !"/spectty-{id}.state".is_empty() = TRUE → gate suppresses Running→Idle.
            // With the fix: hooks_active is passed explicitly as false → gate disabled.
            run_signal_loop(
                &rx,
                &runner,
                &sessions_t,
                &id_t,
                &clock,
                &mut hook_reader,
                false, // !hook_runtime_dir.is_empty() where runtime_dir = ""
                || 0,
                |sc| emitted_t.lock().unwrap().push(sc),
            );
        });

        // Wait one full QUIESCE interval (200ms) for the Quiesce arm to fire at least
        // once. At this point, if the bug is present, the Quiesce arm has suppressed
        // Ready from Running → session is still Running. If fixed, session is Idle.
        std::thread::sleep(QUIESCE + Duration::from_millis(30));

        // Check the status BEFORE dropping tx: this isolates the Quiesce arm's effect
        // from the EOF arm (which is intentionally ungated and would also drive Idle).
        let status_after_quiesce = sessions.get(&id_arc).map(|s| s.status);

        // Drop tx to stop the loop.
        drop(tx);
        handle.join().unwrap();

        // The Quiesce arm must have driven Running→Idle via scraping-derived Ready
        // while the channel was still open (i.e. before EOF).
        // On current code: hooks_active = !"/spectty-{id}.state".is_empty() = TRUE
        // → Quiesce arm suppresses Ready from Running → session stays Running here.
        assert_eq!(
            status_after_quiesce,
            Some(AgentStatus::Idle),
            "C1 re-review: no-hooks session with empty runtime_dir must reach Idle via \
             the Quiesce arm (Running→Idle M2 stopgap) BEFORE EOF fires; \
             got {status_after_quiesce:?}"
        );
    }
}
