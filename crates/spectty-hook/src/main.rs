//! `spectty-hook` — standalone hook sidecar for Spectty lifecycle signals.
//!
//! Claude Code invokes this binary as a hook command on lifecycle events:
//! ```
//! spectty-hook --event <Name>
//! ```
//! where `<Name>` ∈ { Submit, Stop, Permission, SessionEnd, StopFailure }.
//!
//! The binary:
//! 1. Reads `$SPECTTY_SESSION_ID` (exits non-zero if absent).
//! 2. Resolves the runtime dir (exits non-zero if absent).
//! 3. Reads the prior `ts` from the state file (default 0).
//! 4. Writes `{event, ts: prior+1, session_id}` atomically (`.tmp` → rename).
//! 5. Drains and ignores stdin (Claude passes hook JSON; D23 says we never parse it).
//!
//! Depends on serde/serde_json ONLY — NOT spectty-core, NOT tauri (D25).

mod handler;
mod runtime_dir;

use std::path::PathBuf;

use handler::handle_event;
use runtime_dir::spectty_runtime_dir;

/// All errors that can cause a non-zero exit.
#[derive(Debug)]
pub(crate) enum RunError {
    MissingSessionId,
    MissingRuntimeDir,
    MissingEventArg,
    UnknownEvent(String),
    WriteError(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSessionId => {
                write!(f, "SPECTTY_SESSION_ID is not set")
            }
            Self::MissingRuntimeDir => {
                write!(f, "spectty runtime dir cannot be resolved (HOME unset?)")
            }
            Self::MissingEventArg => {
                write!(f, "missing required argument: --event <Name>")
            }
            Self::UnknownEvent(name) => {
                write!(f, "unrecognized event name: {name}")
            }
            Self::WriteError(msg) => {
                write!(f, "failed to write state file: {msg}")
            }
        }
    }
}

fn main() {
    // Drain stdin first: Claude passes hook JSON on stdin (D23). We ignore it
    // but must drain to avoid SIGPIPE from Claude's writer.
    drain_stdin();

    std::process::exit(match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("spectty-hook: {e}");
            1
        }
    });
}

/// Drain and discard all of stdin.
///
/// Claude Code writes hook JSON to the sidecar's stdin (D23). We never parse it
/// (SPECTTY_SESSION_ID is our correlation key), but we MUST consume it to avoid
/// a broken-pipe signal when Claude tries to write.
fn drain_stdin() {
    use std::io::Read;
    let _ = std::io::stdin().lock().read_to_end(&mut Vec::new());
}

/// Core logic: parse args, resolve env, write state file.
///
/// Returns `Ok(())` on success, `Err(RunError)` for any non-zero-exit condition.
/// Separated from `main` so unit tests can call it without spawning a process.
pub(crate) fn run() -> Result<(), RunError> {
    run_with(
        std::env::args().collect::<Vec<_>>().as_slice(),
        &mut |path| {
            std::fs::read_to_string(path).ok().and_then(|s| {
                serde_json::from_str::<serde_json::Value>(&s)
                    .ok()
                    .and_then(|v| v.get("ts").and_then(|t| t.as_u64()))
            })
        },
    )
}

/// Testable inner core: accepts injected args + runtime-dir resolver.
///
/// The `read_prior_ts` closure abstracts the filesystem read so tests can
/// inject a no-op or a controlled value.
pub(crate) fn run_with(
    args: &[String],
    read_prior_ts: &mut impl FnMut(&str) -> Option<u64>,
) -> Result<(), RunError> {
    // ── 1. Parse --event <Name> ───────────────────────────────────────────────
    let event_name = parse_event_arg(args).ok_or(RunError::MissingEventArg)?;

    // ── 2. Read SPECTTY_SESSION_ID ────────────────────────────────────────────
    let session_id = std::env::var("SPECTTY_SESSION_ID").map_err(|_| RunError::MissingSessionId)?;

    // ── 3. Resolve runtime dir ────────────────────────────────────────────────
    let runtime_dir: PathBuf = spectty_runtime_dir().ok_or(RunError::MissingRuntimeDir)?;

    // The runtime dir must exist — the provisioner creates it before injecting hooks.
    // If it does not exist, fail fast (the binary should not create it; that is
    // spawn_session's responsibility, cf. WU-8.2).
    if !runtime_dir.exists() {
        return Err(RunError::MissingRuntimeDir);
    }

    // ── 4. Build state-file paths ─────────────────────────────────────────────
    let state_path = runtime_dir.join(format!("spectty-{session_id}.state"));
    let state_tmp = runtime_dir.join(format!("spectty-{session_id}.state.tmp"));

    let state_path_str = state_path.to_string_lossy().into_owned();
    let state_tmp_str = state_tmp.to_string_lossy().into_owned();

    // ── 5. Call the pure handler ──────────────────────────────────────────────
    handle_event(
        event_name,
        &session_id,
        || {
            let ts = read_prior_ts(&state_path_str);
            Ok(ts)
        },
        |json| {
            // Atomic write: write to `.tmp` then rename over the final path.
            std::fs::write(&state_tmp_str, json)?;
            std::fs::rename(&state_tmp_str, &state_path_str)
        },
    )
    .map_err(|e| match e {
        handler::HandleError::UnknownEvent(name) => RunError::UnknownEvent(name),
        handler::HandleError::WriteError(msg) => RunError::WriteError(msg),
    })
}

/// Parse `--event <Name>` from an argv slice.
///
/// Returns `Some(name)` when found, `None` when the flag or its value is missing.
fn parse_event_arg(args: &[String]) -> Option<&str> {
    args.windows(2)
        .find(|pair| pair[0] == "--event")
        .map(|pair| pair[1].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 4.3: missing SPECTTY_SESSION_ID → RunError::MissingSessionId ─────────
    #[test]
    fn spectty_hook_rejects_missing_session_id() {
        // Remove the env var for this test (process-level — tests must not run in
        // parallel with env mutation; cargo test is single-threaded per test binary).
        std::env::remove_var("SPECTTY_SESSION_ID");

        let args: Vec<String> = vec![
            "spectty-hook".to_string(),
            "--event".to_string(),
            "Stop".to_string(),
        ];
        let result = run_with(&args, &mut |_| None);

        assert!(
            matches!(result, Err(RunError::MissingSessionId)),
            "missing SPECTTY_SESSION_ID must return MissingSessionId, got: {result:?}"
        );
    }

    // ── 4.4: non-existent runtime dir → RunError::MissingRuntimeDir ──────────
    #[test]
    fn spectty_hook_rejects_missing_runtime_dir() {
        // Point $HOME at a fresh temp directory so spectty_runtime_dir() resolves
        // to a path that does NOT exist (no `Library/Application Support/...` subdir).
        let tmp_home = std::env::temp_dir().join("spectty_hook_test_missing_runtime");
        std::fs::create_dir_all(&tmp_home).expect("create fake home");

        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", tmp_home.to_string_lossy().as_ref());
        std::env::set_var("SPECTTY_SESSION_ID", "test-session-4-4");

        let args: Vec<String> = vec![
            "spectty-hook".to_string(),
            "--event".to_string(),
            "Stop".to_string(),
        ];
        let result = run_with(&args, &mut |_| None);

        // Restore env
        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        // Clean up temp dir (best-effort)
        let _ = std::fs::remove_dir_all(&tmp_home);

        assert!(
            matches!(result, Err(RunError::MissingRuntimeDir)),
            "non-existent runtime dir must return MissingRuntimeDir, got: {result:?}"
        );
    }
}
