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

use std::sync::Arc;

use pty_state::PtyRegistry;
use spectty_adapters::{
    AgentRunnerRegistry, ClaudeJsonProvisioner, McpServerEntry, RealConfigFile, SystemClock,
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

/// Resolve the GLOBAL Claude config path (`~/.claude.json`) for the provisioner's
/// default scope. Falls back to a bare `.claude.json` when `$HOME` is unset.
fn home_claude_json() -> String {
    std::env::var("HOME")
        .map(|home| format!("{home}/.claude.json"))
        .unwrap_or_else(|_| ".claude.json".to_string())
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
    let provisioner: Arc<dyn ProvisioningPort> = Arc::new(ClaudeJsonProvisioner::new(
        RealConfigFile,
        home_claude_json(),
        mcp_entry,
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
        // The provisioning + clock ports as `Arc<dyn _>` (the EXACT managed types the
        // session commands' `State<Arc<dyn _>>` resolve — no state-type mismatch).
        .manage(provisioner)
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
