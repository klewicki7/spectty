//! `spectty` Tauri bridge — the single crate allowed to depend on the Tauri
//! runtime. It wires `spectty-core` + `spectty-adapters` to the desktop shell.
//!
//! M1 scope: the `ping`/`pong` liveness proof PLUS the live PTY bridge —
//! `pty_spawn`/`send_input`/`pty_resize`/`pty_kill` backed by a registry-shaped
//! `PtyRegistry` state. The persistence port and adapters remain linked into the
//! dependency graph; session machinery proper arrives in later milestones.

pub mod commands;
pub mod pty_state;
pub mod session_runtime;
pub mod spec_bus;

use std::sync::Arc;

use commands::session::HooksProvisionerState;
use pty_state::PtyRegistry;
use spectty_adapters::{
    AgentRunnerRegistry, ClaudeJsonProvisioner, ClaudeSettingsProvisioner, HookCommandEntry,
    McpServerEntry, RealConfigFile, SystemClock, PERMISSION_PROMPT_MATCHER,
};
use spectty_core::{ClockPort, ProvisioningPort, SessionRegistry};

/// Resolve the path to the bundled `spectty-mcp` sidecar binary.
///
/// The provisioner writes this path into the agent's MCP config
/// (`McpServerEntry.command`) so a cooperative agent launches the REAL stub server,
/// not the `/usr/local/bin/spectty-mcp` fixture used in the adapter unit tests.
/// Tauri ships the sidecar next to the main executable, so we derive it from
/// [`std::env::current_exe`]'s parent — the canonical runtime resolution (the
/// tauri-v2 skill's "never hardcode paths" rule). Falls back to a bare
/// `spectty-mcp` (PATH lookup) if the exe path cannot be determined.
fn spectty_mcp_command() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("spectty-mcp")))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "spectty-mcp".to_string())
}

/// Resolve the path to the bundled `spectty-hook` sidecar binary.
///
/// Mirrors `spectty_mcp_command()` — same runtime resolution from `current_exe()`'s
/// parent. The `ClaudeSettingsProvisioner` embeds this path in every hook command
/// entry so the hooks invoke the REAL bundled binary at runtime.
/// Falls back to a bare `spectty-hook` (PATH lookup) if the exe path is unavailable.
pub(crate) fn spectty_hook_command() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("spectty-hook")))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "spectty-hook".to_string())
}

/// Resolve the Spectty sidecar runtime directory.
///
/// This MUST return the SAME path as `crates/spectty-hook/src/runtime_dir.rs`
/// `spectty_runtime_dir()` — pinned by the WU-9 path-agreement integration test.
///
/// Formula: `{data_local_dir}/app.spectty.desktop/runtime` where `data_local_dir` is:
/// - macOS:   `$HOME/Library/Application Support`
/// - Linux:   `$XDG_DATA_HOME` OR `$HOME/.local/share`
/// - Windows: `%LOCALAPPDATA%`
///
/// Returns empty string when the platform directory cannot be determined (e.g. `$HOME`
/// unset). The hook reader's `poll()` silently returns `None` when the path doesn't
/// exist, so an empty-string runtime dir is a graceful no-op.
/// Public visibility is required so the WU-9 path-agreement integration test
/// in `src-tauri/tests/hook_integration.rs` can compare this against
/// `spectty_hook::spectty_runtime_dir()` in one call site (D25 pin).
pub fn spectty_runtime_dir() -> String {
    #[cfg(target_os = "macos")]
    let base = std::env::var("HOME").ok().map(|home| {
        std::path::PathBuf::from(home)
            .join("Library")
            .join("Application Support")
    });

    #[cfg(target_os = "linux")]
    let base = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        Some(std::path::PathBuf::from(xdg))
    } else {
        std::env::var("HOME")
            .ok()
            .map(|home| std::path::PathBuf::from(home).join(".local").join("share"))
    };

    #[cfg(target_os = "windows")]
    let base = std::env::var("LOCALAPPDATA")
        .ok()
        .map(std::path::PathBuf::from);

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let base: Option<std::path::PathBuf> = None;

    base.map(|b| {
        b.join("app.spectty.desktop")
            .join("runtime")
            .to_string_lossy()
            .into_owned()
    })
    .unwrap_or_default()
}

/// Build the production hook event list registered with `ClaudeSettingsProvisioner`.
///
/// # Why exactly 4 events (NOT 5)
///
/// The original Slice 2 implementation registered a 5th entry using Claude Code's
/// `SubagentStop` hook to drive `StopFailure → Failed → Error`. This was WRONG:
///
/// Per the official Claude Code hook documentation, `SubagentStop` fires whenever
/// **any** subagent finishes — success OR failure. Its payload carries no failure
/// discriminator. Our sidecar drains-and-ignores stdin by design (D23), so it cannot
/// inspect the payload to distinguish a failed subagent from a successful one.
/// Wiring `SubagentStop → StopFailure → Failed → Error` would flip every healthy
/// session to `Error` on normal agentic work (e.g. every tool-call subagent completing).
///
/// **DEVIATION (PR-4)**: `StopFailure` has no reliable hook source in Slice 2.
/// The `SubagentStop` entry is removed from the production event list. `Error` remains
/// reachable via non-hook paths (future work when Claude Code exposes a
/// failure-discriminating event, or via a scraping/exit-code heuristic). The
/// `HookEvent::StopFailure` enum variant, its `event_to_observed` mapping, its
/// `parse_state_file` name, and the sidecar's `--event StopFailure` acceptance are
/// all kept as-is (already on main since PR-1b; harmless, forward-compatible).
///
/// Production list: UserPromptSubmit + Stop + Notification(permission_prompt) + SessionEnd.
pub(crate) fn production_hook_events(
    hook_cmd: &str,
) -> Vec<(String, HookCommandEntry, Option<String>)> {
    vec![
        // ── Slice 1 ──────────────────────────────────────────────────────────────
        (
            "UserPromptSubmit".to_string(),
            HookCommandEntry {
                command: hook_cmd.to_string(),
                args: vec!["--event".to_string(), "Submit".to_string()],
            },
            None,
        ),
        (
            "Stop".to_string(),
            HookCommandEntry {
                command: hook_cmd.to_string(),
                args: vec!["--event".to_string(), "Stop".to_string()],
            },
            None,
        ),
        // ── Slice 2 ──────────────────────────────────────────────────────────────
        (
            // Permission prompt: Claude's `Notification` hook with the empirical
            // `permission_prompt` matcher so ONLY permission-request notifications
            // invoke the sidecar (unrelated notifications are filtered out).
            "Notification".to_string(),
            HookCommandEntry {
                command: hook_cmd.to_string(),
                args: vec!["--event".to_string(), "Permission".to_string()],
            },
            Some(PERMISSION_PROMPT_MATCHER.to_string()),
        ),
        (
            // Session end: Claude's `SessionEnd` hook; no matcher needed.
            "SessionEnd".to_string(),
            HookCommandEntry {
                command: hook_cmd.to_string(),
                args: vec!["--event".to_string(), "SessionEnd".to_string()],
            },
            None,
        ),
        // NOTE: SubagentStop/StopFailure is intentionally absent — see fn doc above.
    ]
}

/// Resolve the GLOBAL Claude config path (`~/.claude.json`) for the provisioner's
/// default scope. Falls back to a bare `.claude.json` when `$HOME` is unset.
fn home_claude_json() -> String {
    std::env::var("HOME")
        .map(|home| format!("{home}/.claude.json"))
        .unwrap_or_else(|_| ".claude.json".to_string())
}

/// Resolve the home directory for the settings provisioner's Global scope path.
///
/// The settings provisioner builds `{home}/.claude/settings.json` for Global scope.
/// This MUST be the resolved `$HOME` value — NOT the literal `~` — to avoid EROFS
/// on macOS's sealed read-only root when the app is launched via Finder (cwd = `/`).
/// Mirrors `home_claude_json()` at the composition root (same resolution pattern).
///
/// Falls back to `.` when `$HOME` is unset so the path degrades to
/// `./.claude/settings.json` rather than crashing. In practice, `$HOME` is always
/// set on macOS/Linux for GUI apps launched via Finder or launchd.
fn home_settings_json() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

/// Build and run the Tauri application.
///
/// Split out of `main.rs` so the same entrypoint can later be reused for the
/// mobile targets that Tauri v2 supports. `generate_handler!` registers every
/// `#[tauri::command]` the frontend may `invoke`; `.manage()` installs the
/// registry-shaped PTY state shared across those commands.
pub fn run() {
    // The Spectty MCP entry the provisioner injects into a cooperative agent's config
    // (Layer-1 registration). `command` points at the bundled `spectty-mcp` sidecar
    // resolved at runtime (NOT the adapter test fixture).
    let mcp_entry = McpServerEntry {
        command: spectty_mcp_command(),
        args: vec!["--stdio".to_string()],
        env: vec![],
    };
    let mcp_provisioner: Arc<dyn ProvisioningPort> = Arc::new(ClaudeJsonProvisioner::new(
        RealConfigFile,
        home_claude_json(),
        mcp_entry,
    ));

    // The hooks provisioner injects the Spectty hook entries into
    // `{home}/.claude/settings.json` (Global) / `.claude/settings.json` (Project).
    // `home_settings_json()` resolves `$HOME` at startup — NOT the literal `~` — to
    // avoid EROFS on macOS's sealed root (Finder launch cwd = `/`).
    // Production event list built by `production_hook_events()` — see its doc comment.
    //
    // Managed as `HooksProvisionerState` (distinct Tauri state type) so it does not
    // collide with `Arc<dyn ProvisioningPort>` (D21).
    let hook_cmd = spectty_hook_command();
    let hooks_events = production_hook_events(&hook_cmd);
    let hooks_provisioner: Arc<dyn ProvisioningPort> = Arc::new(ClaudeSettingsProvisioner::new(
        RealConfigFile,
        home_settings_json(),
        hook_cmd,
        hooks_events,
    ));
    let clock: Arc<dyn ClockPort> = Arc::new(SystemClock::new());

    tauri::Builder::default()
        .manage(PtyRegistry::default())
        // The Core `SessionRegistry` is the SOLE id minter (D13): both `pty_spawn` and
        // `spawn_session` mint through it so the OS-handle `PtyRegistry` and the Core
        // aggregate registry share one id space in lockstep. Managed as `Arc` so the
        // session signal thread can hold a shared reference across the session lifetime.
        .manage(Arc::new(SessionRegistry::default()))
        // String→runner resolver (D12): `claude-code` (cooperative, provisioned) +
        // `generic` (idle-timeout, no provisioning).
        .manage(AgentRunnerRegistry::with_builtin())
        // The MCP provisioner as `Arc<dyn ProvisioningPort>` (the EXACT managed type the
        // session commands' `State<Arc<dyn ProvisioningPort>>` resolves).
        .manage(mcp_provisioner)
        // The hooks provisioner as `HooksProvisionerState` (distinct state type — D21).
        .manage(HooksProvisionerState(hooks_provisioner))
        .manage(clock)
        .invoke_handler(tauri::generate_handler![
            commands::ping::ping,
            commands::pty::pty_spawn,
            commands::pty::send_input,
            commands::pty::pty_resize,
            commands::pty::pty_kill,
            commands::session::spawn_session,
            commands::session::close_session,
            commands::session::list_sessions,
            commands::session::get_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running spectty application");
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── C1 REGRESSION PIN ────────────────────────────────────────────────────
    //
    // DEVIATION (PR-4): SubagentStop was removed from the production event list.
    //
    // WHY: Claude Code's `SubagentStop` hook fires whenever ANY subagent finishes —
    // success OR failure. Its payload has NO failure discriminator, and our sidecar
    // drains-and-ignores stdin by design (D23). Wiring SubagentStop → StopFailure →
    // Failed → Error would flip every healthy session to Error on normal agentic work.
    // `StopFailure` has no reliable hook source in Slice 2; it is deferred until Claude
    // Code exposes a failure-discriminating event. `Error` remains reachable via
    // non-hook paths.
    //
    // RED evidence: before this PR the event list had 5 entries including a
    // ("SubagentStop", …, None) row; this test would have failed on the `len()==4`
    // assertion AND on the `no_subagent_stop` assertion.
    #[test]
    fn production_event_list_excludes_subagent_stop_and_has_exactly_four_entries() {
        let events = production_hook_events("/path/to/spectty-hook");

        // Must have exactly 4 entries: UserPromptSubmit, Stop, Notification, SessionEnd.
        assert_eq!(
            events.len(),
            4,
            "production hook event list must contain exactly 4 entries \
             (UserPromptSubmit + Stop + Notification + SessionEnd); \
             SubagentStop is deferred — see production_hook_events doc"
        );

        // None of the entries may use the SubagentStop Claude hook name.
        let has_subagent_stop = events.iter().any(|(name, _, _)| name == "SubagentStop");
        assert!(
            !has_subagent_stop,
            "SubagentStop MUST NOT be in the production event list: \
             the hook fires on every subagent completion (success AND failure); \
             no failure discriminator is available in the hook payload; \
             wiring it would flip healthy sessions to Error"
        );

        // Positive check: the four expected Claude hook names are present.
        let names: Vec<&str> = events.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(
            names.contains(&"UserPromptSubmit"),
            "UserPromptSubmit must be present"
        );
        assert!(names.contains(&"Stop"), "Stop must be present");
        assert!(
            names.contains(&"Notification"),
            "Notification must be present"
        );
        assert!(names.contains(&"SessionEnd"), "SessionEnd must be present");
    }
}
