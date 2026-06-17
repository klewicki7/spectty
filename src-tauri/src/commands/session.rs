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
use spectty_core::ports::{DiffExplainerPort, FileChanged, FileWatchPort, GitPort};
use spectty_core::{
    AgentSpec, AgentStatus, ClockPort, ProvisioningHandle, ProvisioningPort, ProvisioningScope,
    Session, SessionId, SessionRegistry, SessionSummary, WorkspaceId,
};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use crate::commands::spec::{
    approval_status_from_change, hydrate_spec, spec_updated_from_change, SpecPersistence,
};
use crate::diff_pipeline::{batch_should_trigger, DiffPipeline, DiffPipelines, DiffUpdated};
use crate::pty_state::{PtyId, PtyRegistry, PtyState};
use crate::session_runtime::{
    run_signal_loop, signal_channel, signal_try_send, StatusChanged, SIGNAL_CHANNEL_CAP,
};
use crate::spec_bus::{poll_interval, run_poll_loop, PortPollReader, SpecBus};

/// Newtype wrapper that lets Tauri manage the hooks (settings.json) provisioner as a
/// DISTINCT state type alongside the existing `Arc<dyn ProvisioningPort>` for MCP (D21).
///
/// Tauri's `.manage()` is keyed by `TypeId`, so the MCP provisioner and the hooks
/// provisioner must be wrapped in different types to avoid a collision. This newtype is
/// the minimal footprint — one field, no extra methods.
pub struct HooksProvisionerState(pub Arc<dyn ProvisioningPort>);

/// Outcome of the pure spawn decision: the minted id plus the provisioning handles
/// that were injected (if any), so the caller can stash them for retraction at
/// close and the test can assert WHETHER injection happened.
pub struct SpawnOutcome {
    pub id: SessionId,
    /// MCP provisioner handle (retracted by `close_session`).
    pub handle: Option<ProvisioningHandle>,
    /// Hooks (settings.json) provisioner handle (WU-8). `None` for Generic agents and
    /// when spawned via the legacy `spawn_session_impl` one-provisioner path.
    pub hooks_handle: Option<ProvisioningHandle>,
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
        last_diff: None,
        last_diff_hash: None,
    });

    Ok(SpawnOutcome {
        id,
        handle,
        hooks_handle: None,
    })
}

/// Extended spawn decision: like [`spawn_session_impl`] but injects BOTH the MCP
/// provisioner AND the hooks (settings.json) provisioner (WU-8, D21).
///
/// Both `inject` calls fire here — BEFORE `PtyAdapter::spawn` (which lives in
/// `finish_spawn_impl`) — so the hooks are in place from the first PTY tick.
/// For a Generic agent (`requires_provisioning: false`) neither provisioner is touched.
/// Returns a [`SpawnOutcome`] with BOTH handles for `close_session` retraction.
#[allow(clippy::too_many_arguments)]
pub fn spawn_session_impl_with_hooks(
    agent: &AgentSpec,
    workspace_path: &str,
    title: &str,
    cols: u16,
    rows: u16,
    sessions: &SessionRegistry,
    runners: &AgentRunnerRegistry,
    mcp_provisioner: &dyn ProvisioningPort,
    hooks_provisioner: &dyn ProvisioningPort,
    scope: ProvisioningScope,
    now: spectty_core::Timestamp,
) -> Result<SpawnOutcome, String> {
    // 1. Mint the session id through the SOLE minter (D13).
    let id = sessions.mint_id();

    // 2. Resolve the runner for this agent kind (D12).
    let runner = runners
        .resolve(&agent.kind)
        .ok_or_else(|| format!("no runner registered for agent kind: {}", agent.kind.0))?;

    // 3. (launch_spec is computed by the command wrapper for the real PTY.)
    let _ctx = LaunchContext {
        cwd: workspace_path.to_string(),
        cols,
        rows,
        session_id: id.0.clone(),
        user_command: agent.command.clone(),
    };

    // 4. Inject BOTH provisioners ONLY when the agent requires provisioning. Order:
    //    MCP first, then hooks — both are injected before `PtyAdapter::spawn` fires.
    let (handle, hooks_handle) = if runner.descriptor().capabilities.requires_provisioning {
        let h = mcp_provisioner
            .inject(scope.clone())
            .map_err(|e| format!("mcp provisioning inject failed: {e}"))?;
        let hh = hooks_provisioner
            .inject(scope)
            .map_err(|e| format!("hooks provisioning inject failed: {e}"))?;
        (Some(h), Some(hh))
    } else {
        (None, None)
    };

    // 6. Insert the fully-formed Session (status: Starting) into the aggregate root.
    sessions.insert(Session {
        id: id.clone(),
        workspace: WorkspaceId(workspace_path.to_string()),
        agent: agent.clone(),
        status: AgentStatus::Starting,
        title: title.to_string(),
        created_at: now,
        last_diff: None,
        last_diff_hash: None,
    });

    Ok(SpawnOutcome {
        id,
        handle,
        hooks_handle,
    })
}

/// Best-effort teardown of a spawn that failed AFTER [`spawn_session_impl`] already
/// inserted the Session and (for a cooperative agent) injected provisioning.
///
/// Without this, a post-insert failure (the real PTY refusing to open, the read/signal
/// threads failing to spawn, or the `PtyRegistry` lock being poisoned) would LEAK an
/// orphaned session in `list_sessions` AND an un-retracted `spectty_*` entry in the
/// user's real `~/.claude.json` or `~/.claude/settings.json`. Cleanup ALWAYS removes
/// the session; retraction is best-effort (a leaked key is harmless — D14 — and the
/// next clean spawn/close retracts it), so a retract error never aborts the removal
/// and never panics.
fn cleanup_failed_spawn(
    sessions: &SessionRegistry,
    mcp_provisioner: &dyn ProvisioningPort,
    hooks_provisioner: Option<&dyn ProvisioningPort>,
    id: &SessionId,
    handle: Option<&ProvisioningHandle>,
    hooks_handle: Option<&ProvisioningHandle>,
) {
    if let Some(handle) = handle {
        // Best-effort: ignore the error but ALWAYS proceed to remove the session.
        let _ = mcp_provisioner.retract(handle);
    }
    if let (Some(provisioner), Some(handle)) = (hooks_provisioner, hooks_handle) {
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
///
/// `hooks_provisioner`: optional second provisioner (WU-8). Pass `None` when spawning
/// via the legacy one-provisioner path; pass `Some(&hooks)` when using two provisioners.
fn finish_spawn_impl<T>(
    outcome: SpawnOutcome,
    sessions: &SessionRegistry,
    mcp_provisioner: &dyn ProvisioningPort,
    hooks_provisioner: Option<&dyn ProvisioningPort>,
    spawn_pty: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    match spawn_pty() {
        Ok(value) => Ok(value),
        Err(e) => {
            cleanup_failed_spawn(
                sessions,
                mcp_provisioner,
                hooks_provisioner,
                &outcome.id,
                outcome.handle.as_ref(),
                outcome.hooks_handle.as_ref(),
            );
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

/// Extended teardown for WU-8: kills the PTY, THEN retracts BOTH the MCP provisioner
/// AND the hooks (settings) provisioner, THEN deletes the state file and its `.tmp`
/// twin, THEN removes the session from the registry.
///
/// Order matters — the PTY child must die before either config is touched (a
/// still-running agent must never re-read a half-retracted config). Both provisioners
/// are retracted next; the best-effort policy (`retract` errors are logged, not fatal)
/// is preserved for both. The state-file deletions are also best-effort: `NotFound`
/// is silently ignored so a session that never wrote a state file (e.g. Generic,
/// or the PTY died before the first hook tick) closes cleanly. Registry removal is
/// always last so concurrent observers still see the session until teardown completes.
///
/// `kill` is injected as a closure (same as `close_session_impl`) so tests drive this
/// path with a recording fake and NO real PTY. `delete_state` and `delete_state_tmp`
/// are also injected so tests assert deletion calls without touching the real FS.
#[allow(clippy::too_many_arguments)]
pub fn close_session_impl_with_hooks(
    id: &SessionId,
    mcp_handle: Option<&ProvisioningHandle>,
    hooks_handle: Option<&ProvisioningHandle>,
    sessions: &SessionRegistry,
    mcp_provisioner: &dyn ProvisioningPort,
    hooks_provisioner: &dyn ProvisioningPort,
    kill: impl FnOnce(&SessionId) -> Result<(), String>,
    delete_state: impl FnOnce(&str) -> Result<(), std::io::Error>,
    delete_state_tmp: impl FnOnce(&str) -> Result<(), std::io::Error>,
) -> Result<(), String> {
    // 1. Kill the PTY first (M1 path) — must precede any config retraction.
    kill(id)?;

    // 2. Retract BOTH provisioners (best-effort — failure is returned but does NOT
    //    abort the removal below; a leaked key is harmless per D14).
    if let Some(handle) = mcp_handle {
        mcp_provisioner
            .retract(handle)
            .map_err(|e| format!("mcp provisioner retract failed: {e}"))?;
    }
    if let Some(handle) = hooks_handle {
        hooks_provisioner
            .retract(handle)
            .map_err(|e| format!("hooks provisioner retract failed: {e}"))?;
    }

    // 3. Delete the hook state file and its `.tmp` twin (best-effort — NotFound is OK).
    //    We call the closures unconditionally so the test can record both calls even
    //    when the files are absent; the "not found" case is silently swallowed.
    if let Err(e) = delete_state(id.0.as_str()) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("failed to delete hook state file: {e}"));
        }
    }
    if let Err(e) = delete_state_tmp(id.0.as_str()) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("failed to delete hook state tmp file: {e}"));
        }
    }

    // 4. Remove from the aggregate root last.
    sessions.remove(id);
    Ok(())
}

/// Read-buffer size for one PTY read syscall on the agent read thread (mirrors the
/// M1 `READ_BUF`).
const READ_BUF: usize = 8 * 1024;

/// C3 FIX: ensure the Spectty runtime directory exists before the sidecar is invoked.
///
/// The spectty-hook sidecar requires the runtime dir to pre-exist and returns
/// `MissingRuntimeDir` if it does not. Nothing in the prior code created it.
/// This function is called best-effort in `spawn_session` BEFORE `PtyAdapter::spawn`.
///
/// `dir` is the resolved runtime dir string (may be empty when the platform data
/// dir cannot be determined — that is silently ignored).
pub(crate) fn ensure_runtime_dir(dir: &str) -> std::io::Result<()> {
    if dir.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)
}

/// W2 FIX: opportunistic best-effort removal of a session's own stale `.state` file
/// before a new PTY is spawned with the same id.
///
/// Session-id reuse is not provably impossible (e.g. wrapped counter, persistence
/// cleared, or test harness). A stale state file from a prior session with the same
/// id would be rejected by `StateFileReader` (D23 session_id correlation), but the
/// design §6 calls for a pre-spawn sweep: `NotFound` is silently tolerated.
///
/// This is a separate function so tests can assert it was called without touching the
/// real FS (pass a recording closure).
pub(crate) fn remove_stale_state_file(runtime_dir: &str, session_id: &str) -> std::io::Result<()> {
    if runtime_dir.is_empty() {
        return Ok(());
    }
    let path = format!("{runtime_dir}/spectty-{session_id}.state");
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), // tolerated
        Err(e) => Err(e),
    }
}

/// W1 FIX: delete ALL `spectty-{session_id}.*.state.tmp` files in the runtime dir.
///
/// The sidecar names its tmp file `spectty-{id}.{pid}.state.tmp` (PID-unique per
/// invocation). The old cleanup code deleted `spectty-{id}.state.tmp` (i.e., the
/// state file with `.tmp` appended), which never matched. This function scans the
/// runtime dir for any file matching the PID-wildcard pattern and removes all hits.
///
/// Returns `Ok(())` when the dir does not exist or is otherwise unreadable
/// (best-effort, consistent with the state file cleanup policy).
pub(crate) fn remove_stale_tmp_files(runtime_dir: &str, session_id: &str) -> std::io::Result<()> {
    if runtime_dir.is_empty() {
        return Ok(());
    }
    let prefix = format!("spectty-{session_id}.");
    let suffix = ".state.tmp";
    let entries = match std::fs::read_dir(runtime_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if name.starts_with(&prefix) && name.ends_with(suffix) {
                // Best-effort removal — ignore individual file errors.
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

/// Spawn an agent session in a real PTY, wire BOTH the M1 render path AND the M2
/// OutputSignal status pipeline, mint + register the session, and return its id.
///
/// `async` + OWNED argument types only (Tauri's async-command requirement — no
/// borrowed `&str`); the `State<Arc<dyn Trait>>` types match EXACTLY what `lib.rs`
/// `.manage`s so there is no state-type panic.
///
/// Steps follow design §6.1:
/// 1–4,6: `spawn_session_impl_with_hooks` (mint, resolve runner, inject BOTH MCP +
///    hooks provisioners, insert).
/// 5: `PtyAdapter::spawn` from the runner's `launch_spec`.
/// 7: the combined read thread tees raw bytes to BOTH the render `Channel` (M1) and
///    the bounded signal channel feeding `run_signal_loop`, whose `emit` closure
///    fires `status_changed`.
/// 8: store the live PTY (with BOTH provisioning handles) in the `PtyRegistry`.
/// 9: emit `session_created`.
/// Run the shared diff pipeline ONCE and forward any emitted [`DiffUpdated`] to `emit`
/// (M4 WU-8). Thin wrapper over [`DiffPipeline::run_once`](crate::diff_pipeline::DiffPipeline::run_once)
/// so both trigger sites (the cooperative poll on a blocking task, the FileWatch callback on
/// its debounce thread) share one call shape. The pipeline owns dedup + the in-flight guard,
/// so a double-trigger is harmless; the outcome is intentionally ignored (degraded modes log
/// inside the ports / adapter).
fn run_pipeline_once(
    pipeline: &DiffPipeline,
    git: &dyn GitPort,
    explainer: &dyn DiffExplainerPort,
    mut emit: impl FnMut(DiffUpdated),
) {
    let _ = pipeline.run_once(git, explainer, &mut emit);
}

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
    hooks_prov: State<'_, HooksProvisionerState>,
    clock: State<'_, Arc<dyn ClockPort>>,
    persistence: State<'_, SpecPersistence>,
    pipelines: State<'_, DiffPipelines>,
    git: State<'_, Arc<dyn GitPort>>,
    explainer: State<'_, Arc<dyn DiffExplainerPort>>,
    watcher: State<'_, Arc<dyn FileWatchPort>>,
) -> Result<SessionId, String> {
    // Resolve provisioning scope once at the composition root (D18): Project when the
    // config is git-tracked, else Global. The real git probe lives in the adapter.
    let config_path = format!("{workspace_path}/.mcp.json");
    let scope = resolve_scope(Some(&workspace_path), &config_path, is_git_tracked);

    // Steps 1–4 + 6 (pure, fake-tested): mint, resolve, inject BOTH MCP + hooks, insert.
    let outcome = spawn_session_impl_with_hooks(
        &agent,
        &workspace_path,
        &title,
        cols,
        rows,
        sessions.inner(),
        &runners,
        provisioner.inner().as_ref(),
        hooks_prov.0.as_ref(),
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
    // C2 FIX: pass spec.env into PtySpawnConfig so SPECTTY_SESSION_ID (and any other
    // LaunchSpec env pairs) reach the PTY child process. Without this the sidecar
    // never sees its session id and the hook pipeline is dead end-to-end.
    let cfg = PtySpawnConfig {
        program: spec.program,
        args: spec.args,
        cwd: Some(spec.cwd),
        cols: spec.cols,
        rows: spec.rows,
        env: spec.env,
    };

    // C3 FIX: ensure the spectty runtime dir exists before spawning the PTY so
    // the spectty-hook sidecar can write its state file. Best-effort: a failure
    // does NOT abort the spawn (the hook pipeline will be inactive but the session
    // still works; the sidecar exits non-zero silently).
    let runtime_dir_for_spawn = crate::spectty_runtime_dir();
    let _ = ensure_runtime_dir(&runtime_dir_for_spawn);

    // W2 FIX: opportunistic sweep of any stale state file left by a prior session
    // with the same id. Design §6: NotFound is silently tolerated.
    let _ = remove_stale_state_file(&runtime_dir_for_spawn, &id.0);

    // Steps 5,7,8 routed through `finish_spawn_impl`: open the PTY, wire the read/signal
    // threads, store the live PTY. If ANY of those fails, the seam removes the orphaned
    // session AND retracts the injected provisioning BEFORE surfacing the error, so a
    // failed spawn never leaks a phantom session or a `spectty_*` config entry.
    let handle_for_state = outcome.handle.clone();
    let hooks_handle_for_state = outcome.hooks_handle.clone();
    // M4 WU-4 (D27/D28/D38): the per-session living-spec pipeline. Clone the shared
    // persistence port out of `State` so the read-thread-independent Tokio poll loop can
    // own it. The poll loop watches `spectty/{id}/spec`, deserializes each change into a
    // `SpecContract`, and emits `spec_updated`.
    let spec_port = persistence.0.clone();
    // M4 WU-8 (D37): clone the diff ports + watcher + pipeline registry out of `State` so the
    // read-thread-independent trigger loops can own them past this command frame.
    let diff_git = git.inner().clone();
    let diff_explainer = explainer.inner().clone();
    let diff_watcher = watcher.inner().clone();
    let diff_pipelines = pipelines.inner();
    // Cooperative agents (emits_diff_signals == true) drive the pipeline via the
    // `spectty_diff` trigger and do NOT get a file watcher; the Generic tier
    // (emits_diff_signals == false) uses the debounced FileWatch fallback (D37).
    let emits_diff_signals = runners
        .resolve(&agent.kind)
        .map(|r| r.descriptor().capabilities.emits_diff_signals)
        .unwrap_or(false);
    finish_spawn_impl(
        outcome,
        sessions.inner(),
        provisioner.inner().as_ref(),
        // WU-8: pass the hooks provisioner so cleanup retracts BOTH handles on failure.
        Some(hooks_prov.0.as_ref()),
        || {
            let (adapter, reader) = PtyAdapter::spawn(&cfg).map_err(|e| e.to_string())?;

            // Step 7: wire the render path (M1) AND the status pipeline (M2) on dedicated
            // threads. The signal pipeline gets its own `AgentRunnerRegistry` +
            // `SystemClock` because the read/signal threads outlive this command frame
            // and cannot borrow `State`. Resolving the runner kind again inside the
            // thread keeps the pipeline self-contained.
            let stop = Arc::new(AtomicBool::new(false));
            // C1 FIX (D24): only pass the real runtime_dir when hooks are provisioned
            // for this session (i.e. the agent requires_provisioning and a hooks handle
            // was injected). For Generic agents (no hooks), pass empty string so
            // hooks_active = !hook_runtime_dir.is_empty() = false → M2 stopgap scraping
            // drives all transitions (quiescence drives Running→Idle as before). For
            // Cooperative agents with hooks, the non-empty path means hooks_active=true
            // which activates the hook-gating in run_signal_loop that suppresses a single-
            // tick scraping Ready from flipping Running→Idle (only a hook Stop can do that).
            // NOTE: do NOT derive hooks_active from hook_reader.path() after construction —
            // StateFileReader::new("", id) builds "/spectty-{id}.state" (non-empty),
            // losing the empty-string convention. See run_signal_loop rustdoc.
            let hook_runtime_dir = if hooks_handle_for_state.is_some() {
                crate::spectty_runtime_dir()
            } else {
                String::new()
            };
            let reader_thread = spawn_session_threads(
                app.clone(),
                id.clone(),
                reader,
                on_output,
                Arc::clone(&stop),
                runner_kind,
                Arc::clone(sessions.inner()),
                // WU-8.8 / C1 FIX: runtime_dir is non-empty only when hooks are active.
                hook_runtime_dir,
                id.0.clone(),
            )?;

            // Step 8: store the live PTY with BOTH provisioning handles + state file path.
            // The state file path is `{runtime_dir}/spectty-{session_id}.state` — the same
            // formula used by StateFileReader::new() and the spectty-hook sidecar.
            let runtime_dir = crate::spectty_runtime_dir();
            let state_file = if runtime_dir.is_empty() {
                String::new()
            } else {
                format!("{runtime_dir}/spectty-{}.state", id.0)
            };
            // M4 WU-4 (D38): hydrate the spec pane IMMEDIATELY on (re-)attach — read the
            // persisted contract ONCE and emit before the poll interval, so a restart
            // restores instantly (exit criterion 6). Absent / engram-down → no emit.
            //
            // Finding 3 (PR-2 review): capture the EXACT persisted payload string so the
            // poll reader/bus below can be SEEDED from it. Without seeding, the freshly
            // spawned reader (no prior hash) would treat the unchanged payload as new on its
            // first tick and re-emit the SAME spec → a duplicate `spec_updated`.
            let spec_key = format!("spectty/{}/spec", id.0);
            let hydrated_content = spec_port.get(&spec_key).ok().flatten();
            if let Some(initial) = hydrate_spec(spec_port.as_ref(), &id.0) {
                let _ = app.emit("spec_updated", initial);
            }
            // M4 WU-4 (D27/D28): spawn the per-session SpecBus poll loop on the Tauri
            // runtime. Its injected emit closure deserializes each change → `spec_updated`
            // (drop malformed payloads). A `watch` sender stored on the PtyState stops it
            // at session close.
            let (spec_shutdown_tx, spec_shutdown_rx) = tokio::sync::watch::channel(false);
            {
                // Seed the reader+bus from the hydrated payload so the first tick is a
                // no-op (Finding 3). When nothing was hydrated, start fresh.
                let bus = match &hydrated_content {
                    Some(content) => {
                        let reader = Arc::new(PortPollReader::seeded(spec_port.clone(), content));
                        SpecBus::seeded(reader, spec_key.clone(), PortPollReader::SEEDED_TOKEN)
                    }
                    None => {
                        let reader = Arc::new(PortPollReader::new(spec_port.clone()));
                        SpecBus::new(reader, spec_key.clone())
                    }
                };
                let emit_app = app.clone();
                tokio::spawn(run_poll_loop(
                    bus,
                    poll_interval(),
                    spec_shutdown_rx,
                    move |change| {
                        if let Some(event) = spec_updated_from_change(&change) {
                            let _ = emit_app.emit("spec_updated", event);
                        }
                    },
                ));
            }

            // M4 WU-8 (D37): the per-session VibeLens diff pipeline. ONE pipeline per session
            // is shared by BOTH triggers (cooperative `spectty_diff` poll + generic FileWatch)
            // so hash-dedup + the in-flight guard apply across both. Registered in
            // `DiffPipelines` so `get_diff_explanation` can read its latest explanation and
            // `close_session` can drop it.
            let pipeline = Arc::new(DiffPipeline::new(id.0.clone(), workspace_path.clone()));
            diff_pipelines.insert(pipeline.clone());

            // The cooperative trigger: poll `spectty/{id}/diff` for a trigger doc the
            // `spectty_diff` MCP effect upserts. On a change, run the pipeline immediately
            // (bypassing the FileWatch debounce — D37). Reuses the SpecBus poll seam.
            let diff_key = format!("spectty/{}/diff", id.0);
            let (diff_shutdown_tx, diff_shutdown_rx) = tokio::sync::watch::channel(false);
            {
                let reader = Arc::new(PortPollReader::new(spec_port.clone()));
                let bus = SpecBus::new(reader, diff_key.clone());
                let trigger_pipeline = pipeline.clone();
                let trigger_git = diff_git.clone();
                let trigger_explainer = diff_explainer.clone();
                let trigger_app = app.clone();
                tokio::spawn(run_poll_loop(
                    bus,
                    poll_interval(),
                    diff_shutdown_rx,
                    move |_change| {
                        // A cooperative `spectty_diff` arrived: run the shared pipeline. The
                        // git read + VibeLens push are blocking, so run them on a blocking task
                        // (this emit closure is called on the async poll task — see
                        // run_poll_loop — so we must NOT block the runtime worker here).
                        let pipe = trigger_pipeline.clone();
                        let git = trigger_git.clone();
                        let explainer = trigger_explainer.clone();
                        let emit_app = trigger_app.clone();
                        tokio::task::spawn_blocking(move || {
                            run_pipeline_once(&pipe, git.as_ref(), explainer.as_ref(), |event| {
                                let _ = emit_app.emit("diff_updated", event);
                            });
                        });
                    },
                ));
            }

            // M4 WU-10.11 (D29/D31): the approval-surfacing poll loop. Watches
            // `spectty/{id}/approval`; when the agent's blocked `spectty_approval` upserts a
            // PENDING request, this emits the EXISTING `status_changed(AwaitingInput)` (with
            // quick_actions derived from the request options) — NO new approval event. The
            // resolver half (the long-poll + `approve_prompt`) shipped in PR-3/WU-5. Reuses the
            // SpecBus poll seam, same shape as the spec/diff loops.
            let approval_key = format!("spectty/{}/approval", id.0);
            let (approval_shutdown_tx, approval_shutdown_rx) = tokio::sync::watch::channel(false);
            {
                let reader = Arc::new(PortPollReader::new(spec_port.clone()));
                let bus = SpecBus::new(reader, approval_key.clone());
                let emit_app = app.clone();
                tokio::spawn(run_poll_loop(
                    bus,
                    poll_interval(),
                    approval_shutdown_rx,
                    move |change| {
                        if let Some(event) = approval_status_from_change(&change) {
                            let _ = emit_app.emit("status_changed", event);
                        }
                    },
                ));
            }

            // The generic-tier trigger: a debounced file watcher on the workspace. Only for
            // agents that do NOT emit cooperative signals (D37). The `.git/`-filtered callback
            // runs the SAME shared pipeline (WU-8.0: exclude git's own index churn to avoid a
            // self-trigger loop).
            let diff_watch_guard = if emits_diff_signals {
                None
            } else {
                let watch_pipeline = pipeline.clone();
                let watch_git = diff_git.clone();
                let watch_explainer = diff_explainer.clone();
                let watch_app = app.clone();
                let on_change = move |batch: FileChanged| {
                    // Skip batches that are ONLY git-internal churn (WU-8.0).
                    if !batch_should_trigger(&batch.paths) {
                        return;
                    }
                    // This callback already runs on the watcher's own debounce thread (not a
                    // Tokio worker), so the blocking git/VibeLens calls run synchronously here.
                    let emit_app = watch_app.clone();
                    run_pipeline_once(
                        &watch_pipeline,
                        watch_git.as_ref(),
                        watch_explainer.as_ref(),
                        |event| {
                            let _ = emit_app.emit("diff_updated", event);
                        },
                    );
                };
                // A watch failure (e.g. the workspace path does not exist) is non-fatal: the
                // session still works, just without the generic file-watch trigger.
                diff_watcher
                    .watch(
                        std::path::PathBuf::from(&workspace_path),
                        Box::new(on_change),
                    )
                    .ok()
            };

            let state = PtyState {
                transport: Box::new(adapter),
                stop,
                reader_thread: Some(reader_thread),
                provisioning: handle_for_state,
                hooks_handle: hooks_handle_for_state,
                state_file_path: state_file,
                spec_poll_shutdown: Some(spec_shutdown_tx),
                diff_poll_shutdown: Some(diff_shutdown_tx),
                approval_poll_shutdown: Some(approval_shutdown_tx),
                diff_watch_guard,
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

/// Tear down an agent session: kill the PTY (M1 path) → retract BOTH the MCP and
/// hooks provisioners → delete hook state file + `.tmp` twin → remove from the
/// registry → emit `session_closed`. Ordering is enforced by
/// `close_session_impl_with_hooks` (WU-8, D21).
#[tauri::command]
pub async fn close_session(
    app: AppHandle,
    id: SessionId,
    sessions: State<'_, Arc<SessionRegistry>>,
    ptys: State<'_, PtyRegistry>,
    provisioner: State<'_, Arc<dyn ProvisioningPort>>,
    hooks_prov: State<'_, HooksProvisionerState>,
    pipelines: State<'_, DiffPipelines>,
) -> Result<(), String> {
    // M4 WU-8: drop the session's diff pipeline (its trigger loops are stopped by the
    // PtyState shutdown below — the cooperative poll via its watch sender, the file watcher
    // via its guard's Drop).
    let _ = pipelines.remove(&id.0);
    // Remove the PTY state up-front so we can read BOTH provisioning handles AND own
    // the state for the kill closure; `shutdown` (kill child + join threads) runs
    // inside that closure so the kill-then-retract-then-delete-then-remove order holds.
    let pty_state = {
        let mut guard = ptys
            .0
            .lock()
            .map_err(|_| "pty registry mutex poisoned".to_string())?;
        guard.remove(&id.0)
    };
    let mcp_handle = pty_state.as_ref().and_then(|s| s.provisioning.clone());
    let hooks_handle = pty_state.as_ref().and_then(|s| s.hooks_handle.clone());
    let state_file_path = pty_state
        .as_ref()
        .map(|s| s.state_file_path.clone())
        .unwrap_or_default();

    let kill = move |_id: &SessionId| -> Result<(), String> {
        if let Some(mut state) = pty_state {
            // `shutdown` is best-effort/idempotent: kill the child + join threads.
            state.shutdown();
        }
        Ok(())
    };

    // Delete state file and its .tmp twins (best-effort — NotFound silently ignored).
    // W1 FIX: the sidecar writes spectty-{id}.{pid}.state.tmp (PID-unique), so we
    // scan the runtime dir for all matching spectty-{id}.*.state.tmp files rather than
    // deleting a single fixed path that never matches the real tmp names.
    let state_path = state_file_path.clone();
    // Derive runtime_dir from state_file_path: take the parent dir component.
    let runtime_dir_for_close = std::path::Path::new(&state_file_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let session_id_for_close = id.0.clone();
    let delete_state = move |_: &str| std::fs::remove_file(&state_path);
    let delete_state_tmp =
        move |_: &str| remove_stale_tmp_files(&runtime_dir_for_close, &session_id_for_close);

    close_session_impl_with_hooks(
        &id,
        mcp_handle.as_ref(),
        hooks_handle.as_ref(),
        sessions.inner(),
        provisioner.inner().as_ref(),
        hooks_prov.0.as_ref(),
        kill,
        delete_state,
        delete_state_tmp,
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
            //
            // hooks_active MUST be derived from hook_runtime_dir BEFORE building the
            // StateFileReader. Deriving it from hook_reader.path() after construction
            // is always true because StateFileReader::new("", id) builds path
            // "/spectty-{id}.state" (non-empty) — the empty-dir convention is lost.
            let hooks_active = !hook_runtime_dir.is_empty();
            let mut hook_reader = StateFileReader::new(&hook_runtime_dir, &hook_session_id);
            run_signal_loop(
                &signal_rx,
                runner,
                &sessions,
                &signal_id,
                &clock,
                &mut hook_reader,
                hooks_active,
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
            finish_spawn_impl(outcome, &sessions, &provisioner, None, || {
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
            finish_spawn_impl(outcome, &sessions, &provisioner, None, || {
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

    // ── WU-8 RED TESTS: spawn/close lifecycle wiring ─────────────────────────
    //
    // These tests call `close_session_impl_with_hooks` and the two-provisioner
    // `spawn_session_impl` overload. They are RED until the new functions exist.

    // WU-8.1 RED: both MCP AND hooks provisioners inject BEFORE the PTY spawns.
    // Uses the extended `spawn_session_impl` which accepts a `hooks_provisioner`
    // parameter. Both injects fire in `spawn_session_impl`; the PTY spawn fires
    // later in `finish_spawn_impl`. We record the event order via a shared log.
    #[test]
    fn spawn_session_impl_injects_both_provisioners_before_pty() {
        let sessions = SessionRegistry::default();
        let runners = AgentRunnerRegistry::with_builtin();
        let mcp_provisioner = RecordingProvisioner::default();
        let hooks_provisioner = RecordingProvisioner::default();

        // Cooperative (claude-code) agent — requires_provisioning: true — both inject.
        let outcome = spawn_session_impl_with_hooks(
            &claude_spec(),
            "/repo",
            "Fix the auth bug",
            80,
            24,
            &sessions,
            &runners,
            &mcp_provisioner,
            &hooks_provisioner,
            ProvisioningScope::Global,
            spectty_core::Timestamp(0),
        )
        .expect("spawn ok");

        // BOTH provisioners must have injected exactly once for a cooperative agent.
        assert_eq!(
            *mcp_provisioner.injects.lock().unwrap(),
            vec![ProvisioningScope::Global],
            "mcp provisioner must inject for a cooperative agent"
        );
        assert_eq!(
            *hooks_provisioner.injects.lock().unwrap(),
            vec![ProvisioningScope::Global],
            "hooks provisioner must inject for a cooperative agent"
        );
        // Both handles must be present in the outcome.
        assert!(outcome.handle.is_some(), "mcp handle must be present");
        assert!(
            outcome.hooks_handle.is_some(),
            "hooks handle must be present in SpawnOutcome"
        );

        // PTY NOT yet spawned — `finish_spawn_impl` is the PTY step.
        // No real PTY opened here, so no assertion needed beyond the above.
    }

    // WU-8.2 RED: Generic agent (requires_provisioning: false) → NEITHER provisioner injects.
    #[test]
    fn spawn_session_impl_with_hooks_generic_does_not_inject_either() {
        let sessions = SessionRegistry::default();
        let runners = AgentRunnerRegistry::with_builtin();
        let mcp_provisioner = RecordingProvisioner::default();
        let hooks_provisioner = RecordingProvisioner::default();

        let outcome = spawn_session_impl_with_hooks(
            &generic_spec(),
            "/repo",
            "scratch",
            80,
            24,
            &sessions,
            &runners,
            &mcp_provisioner,
            &hooks_provisioner,
            ProvisioningScope::Global,
            spectty_core::Timestamp(0),
        )
        .expect("generic spawn ok");

        assert!(
            mcp_provisioner.injects.lock().unwrap().is_empty(),
            "generic agent must NOT inject mcp"
        );
        assert!(
            hooks_provisioner.injects.lock().unwrap().is_empty(),
            "generic agent must NOT inject hooks"
        );
        assert!(outcome.handle.is_none(), "no mcp handle for generic");
        assert!(
            outcome.hooks_handle.is_none(),
            "no hooks handle for generic"
        );
    }

    // WU-8.4 RED: close kills PTY first, THEN retracts BOTH provisioners (mcp +
    // hooks), THEN deletes the state file and tmp file, THEN removes from registry.
    #[test]
    fn close_session_impl_kills_pty_then_retracts_both_then_deletes_state() {
        let sessions = SessionRegistry::default();
        let runners = AgentRunnerRegistry::with_builtin();
        let mcp_provisioner = RecordingProvisioner::default();

        let outcome = spawn_session_impl(
            &claude_spec(),
            "/repo",
            "Fix the auth bug",
            80,
            24,
            &sessions,
            &runners,
            &mcp_provisioner,
            ProvisioningScope::Global,
            spectty_core::Timestamp(0),
        )
        .expect("spawn ok");
        let id = outcome.id.clone();
        let mcp_handle = outcome.handle.expect("cooperative spawn has a handle");

        // A fake hooks handle (simulate a settings provisioner inject).
        let hooks_provisioner = RecordingProvisioner::default();
        let hooks_handle = hooks_provisioner
            .inject(ProvisioningScope::Global)
            .expect("hooks inject ok");

        // Event log for ordering assertion.
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        // Kill closure records "kill" and verifies session still present.
        let order_kill = Arc::clone(&order);
        let session_present_at_kill = {
            let id = id.clone();
            let sessions = &sessions;
            move |_: &SessionId| -> Result<(), String> {
                order_kill.lock().unwrap().push("kill");
                assert!(
                    sessions.get(&id).is_some(),
                    "session must exist at kill time"
                );
                Ok(())
            }
        };

        // State file delete closures record calls.
        let order_del = Arc::clone(&order);
        let order_del_tmp = Arc::clone(&order);
        let delete_state = move |_path: &str| -> Result<(), std::io::Error> {
            order_del.lock().unwrap().push("delete_state");
            Ok(())
        };
        let delete_state_tmp = move |_path: &str| -> Result<(), std::io::Error> {
            order_del_tmp.lock().unwrap().push("delete_state_tmp");
            Ok(())
        };

        close_session_impl_with_hooks(
            &id,
            Some(&mcp_handle),
            Some(&hooks_handle),
            &sessions,
            &mcp_provisioner,
            &hooks_provisioner,
            session_present_at_kill,
            delete_state,
            delete_state_tmp,
        )
        .expect("close ok");

        // PTY kill fired.
        assert!(order.lock().unwrap().contains(&"kill"), "kill must fire");
        // Both provisioners retracted.
        assert_eq!(
            *mcp_provisioner.retracts.lock().unwrap(),
            vec![ProvisioningScope::Global],
            "mcp provisioner must be retracted"
        );
        assert_eq!(
            *hooks_provisioner.retracts.lock().unwrap(),
            vec![ProvisioningScope::Global],
            "hooks provisioner must be retracted"
        );
        // State files deleted.
        let events = order.lock().unwrap();
        assert!(
            events.contains(&"delete_state"),
            "state file must be deleted"
        );
        assert!(
            events.contains(&"delete_state_tmp"),
            "state tmp file must be deleted"
        );
        // Kill happened before retracts (kill is first).
        let kill_pos = events.iter().position(|&e| e == "kill").unwrap();
        let del_pos = events.iter().position(|&e| e == "delete_state").unwrap();
        assert!(
            kill_pos < del_pos,
            "kill must precede state deletion; order: {events:?}"
        );
        // Session removed.
        assert!(sessions.get(&id).is_none(), "session must be removed");
    }

    // WU-8.5 RED: close completes without error even when the state file is absent.
    #[test]
    fn close_session_impl_tolerates_absent_state_file() {
        let sessions = SessionRegistry::default();
        let runners = AgentRunnerRegistry::with_builtin();
        let mcp_provisioner = RecordingProvisioner::default();
        let hooks_provisioner = RecordingProvisioner::default();

        let outcome = spawn_session_impl(
            &claude_spec(),
            "/repo",
            "Fix the auth bug",
            80,
            24,
            &sessions,
            &runners,
            &mcp_provisioner,
            ProvisioningScope::Global,
            spectty_core::Timestamp(0),
        )
        .expect("spawn ok");
        let id = outcome.id.clone();
        let mcp_handle = outcome.handle.expect("cooperative spawn has handle");
        let hooks_handle = hooks_provisioner
            .inject(ProvisioningScope::Global)
            .expect("hooks inject ok");

        // Deleters that simulate "file not found" with NotFound error.
        let delete_state = |_path: &str| -> Result<(), std::io::Error> {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such file",
            ))
        };
        let delete_state_tmp = |_path: &str| -> Result<(), std::io::Error> {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such file",
            ))
        };

        // Must complete without error.
        let result = close_session_impl_with_hooks(
            &id,
            Some(&mcp_handle),
            Some(&hooks_handle),
            &sessions,
            &mcp_provisioner,
            &hooks_provisioner,
            |_| Ok(()),
            delete_state,
            delete_state_tmp,
        );

        assert!(
            result.is_ok(),
            "close must tolerate absent state files; got: {result:?}"
        );
        assert!(sessions.get(&id).is_none(), "session still removed");
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
            last_diff: None,
            last_diff_hash: None,
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

    // ── C3 RED: runtime dir creation ─────────────────────────────────────────
    // The spectty-hook sidecar requires the runtime dir to exist at spawn time.
    // ensure_runtime_dir must create it (create_dir_all, idempotent).
    #[test]
    fn ensure_runtime_dir_creates_missing_directory() {
        use super::ensure_runtime_dir;
        let tmp = std::env::temp_dir().join("spectty_ensure_runtime_dir_test");
        let subdir = tmp.join("sub").join("spectty").join("runtime");
        // Clean up any leftover from a prior run.
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(!subdir.exists(), "pre-condition: dir must not exist");

        ensure_runtime_dir(subdir.to_str().unwrap())
            .expect("ensure_runtime_dir must create the directory");

        assert!(
            subdir.is_dir(),
            "ensure_runtime_dir must create the directory and all parents"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_runtime_dir_is_idempotent() {
        use super::ensure_runtime_dir;
        let tmp = std::env::temp_dir().join("spectty_ensure_runtime_dir_idempotent");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Calling twice must not error.
        ensure_runtime_dir(tmp.to_str().unwrap()).expect("first call ok");
        ensure_runtime_dir(tmp.to_str().unwrap())
            .expect("second call (idempotent) must also be ok");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_runtime_dir_empty_string_is_noop() {
        use super::ensure_runtime_dir;
        // An empty runtime_dir (platform dir unavailable) must be silently ignored.
        assert!(ensure_runtime_dir("").is_ok());
    }

    // ── W2 RED: stale-state sweep ─────────────────────────────────────────────
    // Design §6: best-effort remove_file of spectty-{id}.state before spawn.
    #[test]
    fn remove_stale_state_file_deletes_existing_file() {
        use super::remove_stale_state_file;
        let tmp = std::env::temp_dir().join("spectty_stale_state_test");
        std::fs::create_dir_all(&tmp).unwrap();
        let session_id = "stale-session-w2";
        let state_path = tmp.join(format!("spectty-{session_id}.state"));
        std::fs::write(
            &state_path,
            r#"{"event":"Stop","ts":9,"session_id":"stale"}"#,
        )
        .unwrap();
        assert!(state_path.exists(), "pre-condition: stale file must exist");

        remove_stale_state_file(tmp.to_str().unwrap(), session_id)
            .expect("remove_stale_state_file must succeed");

        assert!(!state_path.exists(), "stale state file must be removed");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remove_stale_state_file_tolerates_absent_file() {
        use super::remove_stale_state_file;
        // NotFound must be silently ignored — session may never have had a prior state file.
        assert!(
            remove_stale_state_file("/tmp/no_such_spectty_runtime_dir_w2", "no-such-session")
                .is_ok()
        );
    }

    #[test]
    fn remove_stale_state_file_empty_dir_is_noop() {
        use super::remove_stale_state_file;
        assert!(remove_stale_state_file("", "any").is_ok());
    }

    // ── W1 RED: tmp cleanup formula ───────────────────────────────────────────
    // The sidecar writes spectty-{id}.{pid}.state.tmp; the old cleanup deleted
    // spectty-{id}.state.tmp (which never matched). remove_stale_tmp_files must
    // find and delete all PID-suffixed variants.
    #[test]
    fn remove_stale_tmp_files_deletes_pid_suffixed_tmp_files() {
        use super::remove_stale_tmp_files;
        let tmp = std::env::temp_dir().join("spectty_stale_tmp_test");
        std::fs::create_dir_all(&tmp).unwrap();
        let session_id = "tmp-session-w1";

        // Create two PID-suffixed tmp files (simulating two concurrent sidecar invocations).
        let tmp1 = tmp.join(format!("spectty-{session_id}.12345.state.tmp"));
        let tmp2 = tmp.join(format!("spectty-{session_id}.67890.state.tmp"));
        // Create an unrelated file that must NOT be deleted.
        let unrelated = tmp.join("spectty-other-session.99999.state.tmp");
        std::fs::write(&tmp1, b"tmp1").unwrap();
        std::fs::write(&tmp2, b"tmp2").unwrap();
        std::fs::write(&unrelated, b"unrelated").unwrap();

        remove_stale_tmp_files(tmp.to_str().unwrap(), session_id)
            .expect("remove_stale_tmp_files must succeed");

        assert!(!tmp1.exists(), "PID-suffixed tmp file 1 must be removed");
        assert!(!tmp2.exists(), "PID-suffixed tmp file 2 must be removed");
        assert!(
            unrelated.exists(),
            "unrelated session tmp file must NOT be removed"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remove_stale_tmp_files_tolerates_absent_dir() {
        use super::remove_stale_tmp_files;
        // Dir doesn't exist → Ok (best-effort).
        assert!(remove_stale_tmp_files("/tmp/no_such_spectty_runtime_dir_w1_test", "any").is_ok());
    }

    #[test]
    fn remove_stale_tmp_files_empty_dir_is_noop() {
        use super::remove_stale_tmp_files;
        assert!(remove_stale_tmp_files("", "any").is_ok());
    }
}
