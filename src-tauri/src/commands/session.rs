//! Agent-session lifecycle commands exposed to the frontend.
//!
//! These are the M2 supervision counterparts of M1's raw `pty_*` commands:
//! - `spawn_session` — resolve the runner for an [`AgentSpec`], (optionally) inject
//!   the Spectty MCP provisioning for a cooperative agent, open a real PTY, mint the
//!   session in the Core [`SessionRegistry`], wire the OutputSignal status pipeline
//!   (the PR5a [`run_signal_loop`](crate::session_runtime::run_signal_loop) emit
//!   seam → `app.emit("status_changed", …)`), and return the session id.
//! - `close_session` — kill the PTY (M1 path), retract the injected provisioning for
//!   the session's stored scope, remove it from the registry, and emit `session_closed`.
//! - `list_sessions` / `get_session` — read-only projections of the registry.
//!
//! Like `commands/pty.rs`, the `#[tauri::command]` bodies delegate to small free
//! `*_impl` functions that take their collaborators directly (the registries, a
//! `&dyn ProvisioningPort`, a `&dyn AgentRunner`). That split keeps the lifecycle
//! logic — id minting, the `requires_provisioning` inject decision, the
//! kill-then-retract-then-remove close ordering — unit-testable against fakes with
//! NO real PTY and NO `AppHandle`.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use spectty_adapters::{
    is_git_tracked, resolve_scope, AgentRunnerRegistry, PtyAdapter, PtySpawnConfig,
    StateFileReader, SystemClock,
};
use spectty_core::ports::agent_runner::LaunchContext;
use spectty_core::{
    AgentSpec, AgentStatus, ClockPort, ProvisioningHandle, ProvisioningPort, ProvisioningScope,
    Session, SessionId, SessionRegistry, SessionSummary, WorkspaceId,
};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use crate::pty_state::{PtyId, PtyRegistry, PtyState};
use crate::session_runtime::{
    run_signal_loop, signal_channel, signal_try_send, StatusChanged, SIGNAL_CHANNEL_CAP,
};

/// Outcome of the pure spawn decision: the minted id plus the provisioning handle
/// that was injected (if any), so the caller can stash the handle for retraction at
/// close and the test can assert WHETHER injection happened.
pub struct SpawnOutcome {
    pub id: SessionId,
    pub handle: Option<ProvisioningHandle>,
}

/// The testable core of `spawn_session` WITHOUT the PTY or the signal pipeline.
///
/// Performs design §6.1 steps 1–4 + 6 + 9's data prep: mint the id, resolve the
/// runner, and — ONLY when the runner's descriptor reports `requires_provisioning`
/// — inject the Spectty provisioning at `scope`, storing the returned handle on the
/// freshly-inserted Session's bookkeeping (returned here). The real PTY spawn (step
/// 5) and the runtime wiring (step 7) live in the `#[tauri::command]` wrapper so
/// this function stays pure (no OS handle, no `AppHandle`).
///
/// `scope` is pre-resolved by the caller (the composition root resolves it once via
/// `resolve_scope`) so this function does not shell out to git.
// The collaborators (registries + ports + scope + clock value) are the design §6.1
// inputs; bundling them into a struct would only obscure the explicit dependency
// list this seam exists to make testable.
#[allow(clippy::too_many_arguments)]
pub fn spawn_session_impl(
    agent: &AgentSpec,
    workspace_path: &str,
    title: &str,
    cols: u16,
    rows: u16,
    sessions: &SessionRegistry,
    runners: &AgentRunnerRegistry,
    provisioner: &dyn ProvisioningPort,
    scope: ProvisioningScope,
    now: spectty_core::Timestamp,
) -> Result<SpawnOutcome, String> {
    // 1. Mint the session id through the SOLE minter (D13).
    let id = sessions.mint_id();

    // 2. Resolve the runner for this agent kind (D12). An unknown kind is a caller
    //    error, surfaced as a String (M0/M1 convention) — no panic.
    let runner = runners
        .resolve(&agent.kind)
        .ok_or_else(|| format!("no runner registered for agent kind: {}", agent.kind.0))?;

    // 3. (launch_spec is computed by the command wrapper for the real PTY; the
    //    LaunchContext is shaped here so the wrapper reuses it.)
    let _ctx = LaunchContext {
        cwd: workspace_path.to_string(),
        cols,
        rows,
        session_id: id.0.clone(),
        user_command: agent.command.clone(),
    };

    // 4. Inject provisioning ONLY when the agent requires it (Generic = false → no
    //    ProvisioningPort touched). The capability flag is how the composition root
    //    decides WITHOUT the runner carrying a provisioner() method (R9/D7).
    let handle = if runner.descriptor().capabilities.requires_provisioning {
        Some(
            provisioner
                .inject(scope)
                .map_err(|e| format!("provisioning inject failed: {e}"))?,
        )
    } else {
        None
    };

    // 6. Insert the fully-formed Session (status: Starting) into the aggregate root.
    sessions.insert(Session {
        id: id.clone(),
        workspace: WorkspaceId(workspace_path.to_string()),
        agent: agent.clone(),
        status: AgentStatus::Starting,
        title: title.to_string(),
        created_at: now,
    });

    Ok(SpawnOutcome { id, handle })
}

/// Best-effort teardown of a spawn that failed AFTER [`spawn_session_impl`] already
/// inserted the Session and (for a cooperative agent) injected provisioning.
///
/// Without this, a post-insert failure (the real PTY refusing to open, the read/signal
/// threads failing to spawn, or the `PtyRegistry` lock being poisoned) would LEAK an
/// orphaned session in `list_sessions` AND an un-retracted `spectty_*` entry in the
/// user's real `~/.claude.json`. Cleanup ALWAYS removes the session; retraction is
/// best-effort (a leaked key is harmless — D14 — and the next clean spawn/close
/// retracts it), so a retract error never aborts the removal and never panics.
fn cleanup_failed_spawn(
    sessions: &SessionRegistry,
    provisioner: &dyn ProvisioningPort,
    id: &SessionId,
    handle: Option<&ProvisioningHandle>,
) {
    if let Some(handle) = handle {
        // Best-effort: ignore the error but ALWAYS proceed to remove the session.
        let _ = provisioner.retract(handle);
    }
    sessions.remove(id);
}

/// The testable seam for the post-insert half of `spawn_session` (design §6.1 steps
/// 5, 7, 8): run the injected `spawn_pty` work (open the real PTY, wire the read/signal
/// threads, store the live `PtyState`) and, on ANY error, run [`cleanup_failed_spawn`]
/// BEFORE returning the error so a failed spawn never leaks the orphaned session or the
/// injected provisioning.
///
/// `spawn_pty` is injected as a closure so this seam is unit-testable with a forced
/// post-insert failure and a recording provisioner — NO real PTY and NO `AppHandle`.
/// In production the closure performs `PtyAdapter::spawn` + `spawn_session_threads` +
/// the `PtyRegistry` insert; any of those three error paths flows through the SAME
/// cleanup here.
fn finish_spawn_impl<T>(
    outcome: SpawnOutcome,
    sessions: &SessionRegistry,
    provisioner: &dyn ProvisioningPort,
    spawn_pty: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    match spawn_pty() {
        Ok(value) => Ok(value),
        Err(e) => {
            cleanup_failed_spawn(sessions, provisioner, &outcome.id, outcome.handle.as_ref());
            Err(e)
        }
    }
}

/// The testable core of `close_session`: kill the PTY (M1 path) THEN retract the
/// injected provisioning for the session's stored scope THEN remove from the
/// registry. ORDER MATTERS — the PTY child must die before we touch the config so a
/// still-running agent never re-reads a half-retracted config, and the registry
/// removal is last so a concurrent observer still sees the session until teardown
/// completes.
///
/// `kill` is injected as a closure so the test drives it against `PtyRegistry`'s
/// `kill_impl` (or a recording fake) without a real PTY. Retraction is best-effort:
/// a failure is returned to the caller (which logs it) but does not abort the
/// removal — a leaked key is harmless (D14) and the next clean close retracts it.
pub fn close_session_impl(
    id: &SessionId,
    handle: Option<&ProvisioningHandle>,
    sessions: &SessionRegistry,
    provisioner: &dyn ProvisioningPort,
    kill: impl FnOnce(&SessionId) -> Result<(), String>,
) -> Result<(), String> {
    // 1. Kill the PTY first (M1 path).
    kill(id)?;

    // 2. Retract the EXACT scope that was injected (best-effort).
    if let Some(handle) = handle {
        provisioner
            .retract(handle)
            .map_err(|e| format!("provisioning retract failed: {e}"))?;
    }

    // 3. Remove from the aggregate root last.
    sessions.remove(id);
    Ok(())
}

/// Read-buffer size for one PTY read syscall on the agent read thread (mirrors the
/// M1 `READ_BUF`).
const READ_BUF: usize = 8 * 1024;

/// Spawn an agent session in a real PTY, wire BOTH the M1 render path AND the M2
/// OutputSignal status pipeline, mint + register the session, and return its id.
///
/// `async` + OWNED argument types only (Tauri's async-command requirement — no
/// borrowed `&str`); the `State<Arc<dyn Trait>>` types match EXACTLY what `lib.rs`
/// `.manage`s so there is no state-type panic.
///
/// Steps follow design §6.1:
/// 1–4,6: `spawn_session_impl` (mint, resolve runner, conditional inject, insert).
/// 5: `PtyAdapter::spawn` from the runner's `launch_spec`.
/// 7: the combined read thread tees raw bytes to BOTH the render `Channel` (M1) and
///    the bounded signal channel feeding `run_signal_loop`, whose `emit` closure
///    fires `status_changed`.
/// 8: store the live PTY (with its provisioning handle) in the `PtyRegistry`.
/// 9: emit `session_created`.
// Tauri injects each `State`/`Channel`/`AppHandle` as a separate command argument;
// this IS the IPC command surface, so the argument count is unavoidable.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn spawn_session(
    app: AppHandle,
    agent: AgentSpec,
    workspace_path: String,
    title: String,
    cols: u16,
    rows: u16,
    on_output: Channel<Vec<u8>>,
    sessions: State<'_, Arc<SessionRegistry>>,
    ptys: State<'_, PtyRegistry>,
    runners: State<'_, AgentRunnerRegistry>,
    provisioner: State<'_, Arc<dyn ProvisioningPort>>,
    clock: State<'_, Arc<dyn ClockPort>>,
) -> Result<SessionId, String> {
    // Resolve provisioning scope once at the composition root (D18): Project when the
    // config is git-tracked, else Global. The real git probe lives in the adapter.
    let config_path = format!("{workspace_path}/.mcp.json");
    let scope = resolve_scope(Some(&workspace_path), &config_path, is_git_tracked);

    // Steps 1–4 + 6 (pure, fake-tested): mint, resolve, conditional inject, insert.
    let outcome = spawn_session_impl(
        &agent,
        &workspace_path,
        &title,
        cols,
        rows,
        sessions.inner(),
        &runners,
        provisioner.inner().as_ref(),
        scope,
        clock.now(),
    )?;
    let id = outcome.id.clone();

    // Step 5: build the launch spec for the resolved runner and open the real PTY.
    let runner_kind = agent.kind.clone();
    let spec = {
        let runner = runners
            .resolve(&runner_kind)
            .ok_or_else(|| format!("no runner registered for agent kind: {}", runner_kind.0))?;
        runner.launch_spec(&LaunchContext {
            cwd: workspace_path.clone(),
            cols,
            rows,
            session_id: id.0.clone(),
            user_command: agent.command.clone(),
        })
    };
    let cfg = PtySpawnConfig {
        program: spec.program,
        args: spec.args,
        cwd: Some(spec.cwd),
        cols: spec.cols,
        rows: spec.rows,
    };

    // Steps 5,7,8 routed through `finish_spawn_impl`: open the PTY, wire the read/signal
    // threads, store the live PTY. If ANY of those fails, the seam removes the orphaned
    // session AND retracts the injected provisioning BEFORE surfacing the error, so a
    // failed spawn never leaks a phantom session or a `spectty_*` config entry.
    let handle_for_state = outcome.handle.clone();
    finish_spawn_impl(
        outcome,
        sessions.inner(),
        provisioner.inner().as_ref(),
        || {
            let (adapter, reader) = PtyAdapter::spawn(&cfg).map_err(|e| e.to_string())?;

            // Step 7: wire the render path (M1) AND the status pipeline (M2) on dedicated
            // threads. The signal pipeline gets its own `AgentRunnerRegistry` +
            // `SystemClock` because the read/signal threads outlive this command frame
            // and cannot borrow `State`. Resolving the runner kind again inside the
            // thread keeps the pipeline self-contained.
            let stop = Arc::new(AtomicBool::new(false));
            let reader_thread = spawn_session_threads(
                app.clone(),
                id.clone(),
                reader,
                on_output,
                Arc::clone(&stop),
                runner_kind,
                Arc::clone(sessions.inner()),
                // WU-8 will supply the real spectty_runtime_dir() here; for now
                // an empty string means the hook reader path won't exist and
                // poll() will silently return None every tick (graceful no-op).
                String::new(),
                id.0.clone(),
            )?;

            // Step 8: store the live PTY (with its provisioning handle for retraction).
            let state = PtyState {
                transport: Box::new(adapter),
                stop,
                reader_thread: Some(reader_thread),
                provisioning: handle_for_state,
            };
            ptys.0
                .lock()
                .map_err(|_| "pty registry mutex poisoned".to_string())?
                .insert(id.0.clone(), state);
            Ok(())
        },
    )?;

    // Step 9: announce the new session to the UI.
    let summary = sessions
        .get(&id)
        .map(|s| SessionSummary::from(&s))
        .ok_or_else(|| "session vanished after insert".to_string())?;
    let _ = app.emit("session_created", summary);

    Ok(id)
}

/// Tear down an agent session: kill the PTY (M1 path) → retract the injected
/// provisioning for the stored scope → remove from the registry → emit
/// `session_closed`. Ordering is enforced by `close_session_impl`.
#[tauri::command]
pub async fn close_session(
    app: AppHandle,
    id: SessionId,
    sessions: State<'_, Arc<SessionRegistry>>,
    ptys: State<'_, PtyRegistry>,
    provisioner: State<'_, Arc<dyn ProvisioningPort>>,
) -> Result<(), String> {
    // Remove the PTY state up-front so we can read its provisioning handle AND own
    // it for the kill closure; the M1 `shutdown` (kill child + join threads) runs
    // inside that closure so the kill-then-retract-then-remove order holds.
    let pty_state = {
        let mut guard = ptys
            .0
            .lock()
            .map_err(|_| "pty registry mutex poisoned".to_string())?;
        guard.remove(&id.0)
    };
    let handle = pty_state.as_ref().and_then(|s| s.provisioning.clone());

    let kill = move |_id: &SessionId| -> Result<(), String> {
        if let Some(mut state) = pty_state {
            // `shutdown` is best-effort/idempotent: kill the child + join threads.
            state.shutdown();
        }
        Ok(())
    };

    close_session_impl(
        &id,
        handle.as_ref(),
        sessions.inner(),
        provisioner.inner().as_ref(),
        kill,
    )?;

    let _ = app.emit("session_closed", id);
    Ok(())
}

/// Project every live session into a `SessionSummary` for the session list.
#[tauri::command]
pub fn list_sessions(
    sessions: State<'_, Arc<SessionRegistry>>,
) -> Result<Vec<SessionSummary>, String> {
    Ok(sessions.summaries())
}

/// Look up one session's summary by id (`None` when absent).
#[tauri::command]
pub fn get_session(
    id: SessionId,
    sessions: State<'_, Arc<SessionRegistry>>,
) -> Result<Option<SessionSummary>, String> {
    Ok(sessions.get(&id).map(|s| SessionSummary::from(&s)))
}

/// Spawn the combined read thread (render tee + signal tee) and the signal thread.
///
/// The read thread does the blocking `reader.read(..)` and forwards each slice to
/// TWO consumers:
/// - the M1 render path (a `Coalescer` → `on_output` Channel), on its own forwarder
///   thread (UNBOUNDED, never throttled);
/// - the M2 bounded signal channel (`signal_try_send`, drop-on-full), feeding the
///   signal thread's [`run_signal_loop`].
///
/// `hook_runtime_dir` and `hook_session_id` are forwarded into the signal thread to
/// construct the [`StateFileReader`] that polls the hook state file on each tick
/// (WU-7/WU-8). Pass an empty string for `hook_runtime_dir` (or a nonexistent path)
/// when no hook file is expected — `poll` returns `None` silently.
///
/// Returns the READ thread's `JoinHandle`; it owns and joins both the forwarder and
/// the signal thread so a single join (via `PtyState::shutdown`) tears down all
/// three with no leak.
#[allow(clippy::too_many_arguments)]
fn spawn_session_threads(
    app: AppHandle,
    id: SessionId,
    mut reader: Box<dyn Read + Send>,
    on_output: Channel<Vec<u8>>,
    stop: Arc<AtomicBool>,
    runner_kind: spectty_core::AgentKind,
    sessions: Arc<SessionRegistry>,
    hook_runtime_dir: String,
    hook_session_id: String,
) -> Result<std::thread::JoinHandle<()>, String> {
    use std::sync::mpsc;

    // Render side: reuse the M1 forwarder discipline (Coalescer → Channel) on its own
    // thread, fed by an UNBOUNDED mpsc so render is never back-pressured.
    let (render_tx, render_rx) = mpsc::channel::<Vec<u8>>();
    let render_id: PtyId = id.0.clone();
    let render_app = app.clone();
    let forwarder =
        crate::commands::pty::spawn_render_forwarder(render_app, render_id, render_rx, on_output)?;

    // Signal side: a bounded drop-on-full channel + the signal thread running the
    // PR5a `run_signal_loop`, whose `emit` closure fires `status_changed`.
    let (signal_tx, signal_rx) = signal_channel(SIGNAL_CHANNEL_CAP);
    let signal_id = id.clone();
    let signal_app = app.clone();
    let signal_thread = std::thread::Builder::new()
        .name(format!("session-signal-{}", id.0))
        .spawn(move || {
            // The signal pipeline owns its own runner + clock (the read/signal threads
            // outlive the command frame, so they cannot borrow `State`).
            let runners = AgentRunnerRegistry::with_builtin();
            let clock = SystemClock::new();
            let Some(runner) = runners.resolve(&runner_kind) else {
                return;
            };
            // Construct the StateFileReader inside the thread (WU-7/WU-8 wiring).
            // The runtime dir and session id are resolved by spawn_session (WU-8) and
            // forwarded here. For cooperative agents this points at the real state
            // file; for Generic agents hook_runtime_dir is empty → poll returns None.
            let mut hook_reader = StateFileReader::new(&hook_runtime_dir, &hook_session_id);
            run_signal_loop(
                &signal_rx,
                runner,
                &sessions,
                &signal_id,
                &clock,
                &mut hook_reader,
                // M2 cannot retrieve the real child exit code from the read side
                // without owning the child handle (same limitation as M1 `pty_exit`),
                // so EOF reports a clean exit; the terminal status is `Completed`.
                || 0,
                |sc: StatusChanged| {
                    let _ = signal_app.emit("status_changed", sc);
                },
            );
        })
        .map_err(|e| format!("failed to spawn session signal thread: {e}"))?;

    // Read thread: blocking reads → tee each slice to BOTH consumers. Owns + joins
    // the forwarder AND the signal thread so one handle tears down all three.
    std::thread::Builder::new()
        .name(format!("session-read-{}", id.0))
        .spawn(move || {
            let mut buf = [0u8; READ_BUF];
            loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let slice = buf[..n].to_vec();
                        // Render tee: unbounded, never dropped. If the forwarder is
                        // gone the render side has ended; keep feeding the signal side.
                        let _ = render_tx.send(slice.clone());
                        // Signal tee: bounded, drop-on-full, NEVER blocks (R6/D9).
                        let _ = signal_try_send(&signal_tx, slice);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            // Drop both senders: the forwarder drains + emits `pty_exit`, and the
            // signal loop sees `Disconnected` → emits the terminal status. Then join
            // both so neither leaks.
            drop(render_tx);
            drop(signal_tx);
            let _ = forwarder.join();
            let _ = signal_thread.join();
        })
        .map_err(|e| format!("failed to spawn session read thread: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use spectty_core::entities::agent_spec::{AgentKind, AgentTier};

    /// A provisioning port that records every inject/retract so a test can assert
    /// WHETHER (and for which scope) provisioning happened — without touching a real
    /// config file.
    #[derive(Default)]
    struct RecordingProvisioner {
        injects: Mutex<Vec<ProvisioningScope>>,
        retracts: Mutex<Vec<ProvisioningScope>>,
    }

    impl ProvisioningPort for RecordingProvisioner {
        fn inject(
            &self,
            scope: ProvisioningScope,
        ) -> Result<ProvisioningHandle, spectty_core::ProvisioningError> {
            self.injects.lock().unwrap().push(scope.clone());
            Ok(ProvisioningHandle { scope })
        }
        fn retract(
            &self,
            handle: &ProvisioningHandle,
        ) -> Result<(), spectty_core::ProvisioningError> {
            self.retracts.lock().unwrap().push(handle.scope.clone());
            Ok(())
        }
    }

    fn claude_spec() -> AgentSpec {
        AgentSpec {
            kind: AgentKind("claude-code".to_string()),
            command: None,
            tier: AgentTier::Cooperative,
        }
    }

    fn generic_spec() -> AgentSpec {
        AgentSpec {
            kind: AgentKind("generic".to_string()),
            command: Some(vec!["/bin/sh".to_string()]),
            tier: AgentTier::Generic,
        }
    }

    // WU-9.6: spawn mints + inserts + injects ONLY when the runner requires it.
    #[test]
    fn spawn_session_impl_mints_inserts_and_injects_only_when_required() {
        let sessions = SessionRegistry::default();
        let runners = AgentRunnerRegistry::with_builtin();
        let provisioner = RecordingProvisioner::default();

        // Claude Code is cooperative → requires_provisioning: true → inject called.
        let outcome = spawn_session_impl(
            &claude_spec(),
            "/repo",
            "Fix the auth bug",
            80,
            24,
            &sessions,
            &runners,
            &provisioner,
            ProvisioningScope::Global,
            spectty_core::Timestamp(0),
        )
        .expect("claude spawn ok");

        // id minted + session inserted at Starting.
        let stored = sessions.get(&outcome.id).expect("session inserted");
        assert_eq!(stored.status, AgentStatus::Starting);
        assert_eq!(stored.title, "Fix the auth bug");
        assert_eq!(stored.agent.kind, AgentKind("claude-code".to_string()));

        // inject called exactly once, for the resolved scope, and a handle returned.
        assert_eq!(
            *provisioner.injects.lock().unwrap(),
            vec![ProvisioningScope::Global],
            "a cooperative (requires_provisioning) agent must inject exactly once"
        );
        assert_eq!(
            outcome.handle.as_ref().map(|h| &h.scope),
            Some(&ProvisioningScope::Global),
            "the returned handle carries the injected scope for retraction at close"
        );

        // Generic does NOT require provisioning → inject NOT called for it.
        let generic_provisioner = RecordingProvisioner::default();
        let generic = spawn_session_impl(
            &generic_spec(),
            "/repo",
            "scratch shell",
            80,
            24,
            &sessions,
            &runners,
            &generic_provisioner,
            ProvisioningScope::Global,
            spectty_core::Timestamp(0),
        )
        .expect("generic spawn ok");

        assert!(
            generic_provisioner.injects.lock().unwrap().is_empty(),
            "a Generic (requires_provisioning: false) agent must NOT inject"
        );
        assert!(
            generic.handle.is_none(),
            "a Generic spawn carries no provisioning handle"
        );

        // Two distinct sessions now live in the registry — no real PTY was opened.
        assert_eq!(sessions.summaries().len(), 2);
        assert_ne!(outcome.id, generic.id);
    }

    // WU-9.7: close kills the PTY, THEN retracts the stored scope, THEN removes — in
    // that order. A shared log records the sequence so the ordering is asserted.
    #[test]
    fn close_session_impl_kills_pty_then_retracts_and_removes() {
        let sessions = SessionRegistry::default();
        let runners = AgentRunnerRegistry::with_builtin();
        let provisioner = RecordingProvisioner::default();

        // Spawn a cooperative session so it carries a provisioning handle.
        let outcome = spawn_session_impl(
            &claude_spec(),
            "/repo",
            "Fix the auth bug",
            80,
            24,
            &sessions,
            &runners,
            &provisioner,
            ProvisioningScope::Global,
            spectty_core::Timestamp(0),
        )
        .expect("spawn ok");
        let id = outcome.id.clone();
        let handle = outcome.handle.expect("cooperative spawn has a handle");

        // Ordered event log: kill must be recorded BEFORE retract, and the session
        // must still be present in the registry at kill time (removal is last).
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let order_kill = Arc::clone(&order);
        let session_present_at_kill = {
            let id = id.clone();
            let sessions = &sessions;
            move |_: &SessionId| -> Result<(), String> {
                order_kill.lock().unwrap().push("kill");
                assert!(
                    sessions.get(&id).is_some(),
                    "the session must still exist when the PTY is killed (remove is last)"
                );
                Ok(())
            }
        };

        close_session_impl(
            &id,
            Some(&handle),
            &sessions,
            &provisioner,
            session_present_at_kill,
        )
        .expect("close ok");

        // retract was called for the EXACT injected scope.
        assert_eq!(
            *provisioner.retracts.lock().unwrap(),
            vec![ProvisioningScope::Global],
            "close must retract the session's stored scope"
        );
        // kill ran (recorded), then retract, then remove — the session is gone.
        assert_eq!(*order.lock().unwrap(), vec!["kill"]);
        assert!(
            sessions.get(&id).is_none(),
            "close must remove the session from the registry last"
        );
    }

    // Fix #1: a failure on ANY post-insert spawn step must clean up the orphan — the
    // already-inserted Session is removed AND the already-injected provisioning is
    // retracted — so a failed spawn never leaks a phantom session or a `spectty_*`
    // entry in the user's real `~/.claude.json`.
    #[test]
    fn spawn_session_cleans_up_when_pty_spawn_fails() {
        let sessions = SessionRegistry::default();
        let runners = AgentRunnerRegistry::with_builtin();
        let provisioner = RecordingProvisioner::default();

        // Steps 1–6: a cooperative spawn injects + inserts (the leak surface).
        let outcome = spawn_session_impl(
            &claude_spec(),
            "/repo",
            "Fix the auth bug",
            80,
            24,
            &sessions,
            &runners,
            &provisioner,
            ProvisioningScope::Global,
            spectty_core::Timestamp(0),
        )
        .expect("spawn ok");
        let id = outcome.id.clone();
        assert!(
            sessions.get(&id).is_some(),
            "session inserted before pty open"
        );

        // The post-insert PTY/thread/registry work fails (e.g. PtyAdapter::spawn errs).
        let result: Result<(), String> =
            finish_spawn_impl(outcome, &sessions, &provisioner, || {
                Err("pty open failed".to_string())
            });

        assert!(
            result.is_err(),
            "a failed post-insert step surfaces the error"
        );
        assert!(
            sessions.get(&id).is_none(),
            "a failed spawn must REMOVE the orphaned session (no phantom in list_sessions)"
        );
        assert_eq!(
            *provisioner.retracts.lock().unwrap(),
            vec![ProvisioningScope::Global],
            "a failed spawn must RETRACT the injected provisioning (no leaked spectty_* key)"
        );
    }

    // Fix #1: a Generic (no provisioning) spawn that fails post-insert still removes the
    // orphan but does NOT call retract (nothing was injected).
    #[test]
    fn spawn_session_cleanup_removes_generic_session_without_retract() {
        let sessions = SessionRegistry::default();
        let runners = AgentRunnerRegistry::with_builtin();
        let provisioner = RecordingProvisioner::default();

        let outcome = spawn_session_impl(
            &generic_spec(),
            "/repo",
            "scratch shell",
            80,
            24,
            &sessions,
            &runners,
            &provisioner,
            ProvisioningScope::Global,
            spectty_core::Timestamp(0),
        )
        .expect("spawn ok");
        let id = outcome.id.clone();

        let result: Result<(), String> =
            finish_spawn_impl(outcome, &sessions, &provisioner, || {
                Err("thread wiring failed".to_string())
            });

        assert!(result.is_err());
        assert!(
            sessions.get(&id).is_none(),
            "a failed Generic spawn must still remove the orphaned session"
        );
        assert!(
            provisioner.retracts.lock().unwrap().is_empty(),
            "a Generic spawn injected nothing, so cleanup must NOT retract"
        );
    }

    // Fix #2 (the #1 load-bearing property): the SHARED `Arc<SessionRegistry>` the
    // signal thread holds and the read path are the SAME registry. A status update
    // applied through the thread's Arc clone is visible through the original Arc. A
    // future refactor that accidentally forks the registry (deep clone) breaks this.
    #[test]
    fn status_update_via_thread_arc_is_visible_through_read_arc() {
        use spectty_core::entities::agent_status::Observed;

        let reg = Arc::new(SessionRegistry::default());
        let id = reg.mint_id();
        reg.insert(Session {
            id: id.clone(),
            workspace: WorkspaceId("/repo".to_string()),
            agent: generic_spec(),
            status: AgentStatus::Starting,
            title: "shared-registry probe".to_string(),
            created_at: spectty_core::Timestamp(0),
        });

        // What `spawn_session` hands the signal thread: an `Arc::clone`, NOT a deep copy.
        let thread_clone = Arc::clone(&reg);
        let changed = thread_clone.apply_observed(&id, Observed::Working);
        assert_eq!(
            changed,
            Some(AgentStatus::Running),
            "Starting + Working transitions to Running"
        );

        // Visible through the ORIGINAL read Arc — same underlying registry.
        assert_eq!(
            reg.get(&id).expect("session present").status,
            AgentStatus::Running,
            "a write via the signal thread's Arc must be visible through the read Arc"
        );
    }
}
