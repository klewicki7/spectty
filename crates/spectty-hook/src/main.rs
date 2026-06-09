//! `spectty-hook` — standalone hook sidecar for the Spectty lifecycle signals.
//!
//! WU-1 skeleton: crate registered in workspace, binary target declared.
//! Full implementation (--event <Name>, state-file write) lands in WU-4 (PR-1b-i).
//!
//! Depends on serde/serde_json ONLY — NOT spectty-core, NOT tauri (D25).

fn main() {
    // WU-4 (PR-1b-i): read $SPECTTY_SESSION_ID + --event <Name>; atomic write state.
    eprintln!("spectty-hook: not yet implemented (WU-4)");
    std::process::exit(1);
}
