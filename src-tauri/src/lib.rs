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

use pty_state::PtyRegistry;
use spectty_core::SessionRegistry;

/// Build and run the Tauri application.
///
/// Split out of `main.rs` so the same entrypoint can later be reused for the
/// mobile targets that Tauri v2 supports. `generate_handler!` registers every
/// `#[tauri::command]` the frontend may `invoke`; `.manage()` installs the
/// registry-shaped PTY state shared across those commands.
pub fn run() {
    tauri::Builder::default()
        .manage(PtyRegistry::default())
        // The Core `SessionRegistry` is the SOLE id minter (D13): `pty_spawn` mints
        // through it so the OS-handle `PtyRegistry` and the Core aggregate registry
        // share one id space in lockstep. The session commands + the rest of the M2
        // wiring (`AgentRunnerRegistry`/`ProvisioningPort`/`ClockPort`) land in PR5b.
        .manage(SessionRegistry::default())
        .invoke_handler(tauri::generate_handler![
            commands::ping::ping,
            commands::pty::pty_spawn,
            commands::pty::send_input,
            commands::pty::pty_resize,
            commands::pty::pty_kill,
        ])
        .run(tauri::generate_context!())
        .expect("error while running spectty application");
}
