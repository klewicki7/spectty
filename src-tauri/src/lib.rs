//! `spectty` Tauri bridge — the single crate allowed to depend on the Tauri
//! runtime. It wires `spectty-core` + `spectty-adapters` to the desktop shell.
//!
//! M0 scope: only the `ping`/`pong` liveness proof. The persistence port and
//! adapters are linked into the dependency graph (proving the boundary holds)
//! but are not yet driven by a command — that arrives with the session work in
//! later milestones.

pub mod commands;

/// Build and run the Tauri application.
///
/// Split out of `main.rs` so the same entrypoint can later be reused for the
/// mobile targets that Tauri v2 supports. `generate_handler!` registers every
/// `#[tauri::command]` the frontend may `invoke`.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::ping::ping])
        .run(tauri::generate_context!())
        .expect("error while running spectty application");
}
