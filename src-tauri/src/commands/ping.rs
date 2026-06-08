// Tauri v2 moved `emit` onto the `Emitter` trait. In v1 you called
// `window.emit(...)`; in v2 `AppHandle` (and `Window`/`WebviewWindow`) gain
// `emit` ONLY when the `Emitter` trait is in scope. Importing it here is what
// makes `app.emit(...)` resolve — this is the GUARD against accidentally reaching
// for the removed v1 `Window::emit` signature.
use tauri::{AppHandle, Emitter};

/// The M0 liveness proof: the frontend invokes `ping`, the backend emits a
/// `pong` event. This exercises the full invoke -> command -> emit -> listen
/// round-trip with no domain logic and no persistence involved.
#[tauri::command]
pub fn ping(app: AppHandle) -> Result<(), String> {
    app.emit("pong", "pong from spectty backend")
        .map_err(|err| err.to_string())
}
