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
///
/// ## Environment variables
///
/// - `SPECTTY_SESSION_ID` (required): correlation key; written into the state file.
/// - `SPECTTY_RUNTIME_DIR` (optional, DEBUG BUILDS ONLY): override the resolved
///   runtime directory. Used by the WU-9 integration test to point the binary at
///   a PID-unique temp dir without touching the real `~/Library/Application
///   Support` path. Gated behind `debug_assertions` because the hook inherits the
///   agent's full shell environment and NO reader honors this var — a leaked
///   value in a release build would make the writer and `StateFileReader`
///   silently diverge (the exact D25 failure WU-9 fences off). `cfg(test)` would
///   NOT work here: the integration test spawns the compiled binary, for which
///   `cfg(test)` is false; `debug_assertions` is true under `cargo test`/`cargo
///   build` and false in release artifacts.
pub(crate) fn run() -> Result<(), RunError> {
    let args = std::env::args().collect::<Vec<_>>();
    let session_id = std::env::var("SPECTTY_SESSION_ID").ok();
    #[cfg(debug_assertions)]
    let runtime_dir = std::env::var("SPECTTY_RUNTIME_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(spectty_runtime_dir);
    #[cfg(not(debug_assertions))]
    let runtime_dir = spectty_runtime_dir();
    run_with(
        args.as_slice(),
        session_id.as_deref(),
        runtime_dir.as_deref(),
        &mut |path| {
            std::fs::read_to_string(path).ok().and_then(|s| {
                serde_json::from_str::<serde_json::Value>(&s)
                    .ok()
                    .and_then(|v| v.get("ts").and_then(|t| t.as_u64()))
            })
        },
    )
}

/// Testable inner core: accepts injected args, session_id, runtime_dir, and a
/// prior-ts reader closure.
///
/// All environment-sourced values (`SPECTTY_SESSION_ID`, `HOME`/`LOCALAPPDATA`,
/// `XDG_DATA_HOME`) are resolved by the caller and passed in explicitly. Tests
/// can therefore inject arbitrary values without mutating process-global env,
/// which would race against parallel test threads.
///
/// - `session_id`: `None` → `RunError::MissingSessionId`
/// - `runtime_dir`: `None` → `RunError::MissingRuntimeDir` (resolver returned None)
/// - `runtime_dir` pointing to a non-existent path → `RunError::MissingRuntimeDir`
/// - `read_prior_ts`: closure called with the canonical state-file path; returns
///   `Some(ts)` if a prior file exists, `None` on absence or parse error.
pub(crate) fn run_with(
    args: &[String],
    session_id: Option<&str>,
    runtime_dir: Option<&std::path::Path>,
    read_prior_ts: &mut impl FnMut(&str) -> Option<u64>,
) -> Result<(), RunError> {
    // ── 1. Parse --event <Name> ───────────────────────────────────────────────
    let event_name = parse_event_arg(args).ok_or(RunError::MissingEventArg)?;

    // ── 2. Resolve SPECTTY_SESSION_ID ────────────────────────────────────────
    let session_id = session_id.ok_or(RunError::MissingSessionId)?;

    // ── 3. Resolve runtime dir ────────────────────────────────────────────────
    let runtime_dir: PathBuf = runtime_dir
        .ok_or(RunError::MissingRuntimeDir)?
        .to_path_buf();

    // The runtime dir must exist — the provisioner creates it before injecting hooks.
    // If it does not exist, fail fast (the binary should not create it; that is
    // spawn_session's responsibility, cf. WU-8.2).
    if !runtime_dir.exists() {
        return Err(RunError::MissingRuntimeDir);
    }

    // ── 4. Build state-file paths ─────────────────────────────────────────────
    let state_path = runtime_dir.join(format!("spectty-{session_id}.state"));
    // Include the PID to make the tmp name unique per process invocation.
    // Two concurrent hook processes for the same session each write to their own
    // tmp file; the last rename wins (monotonic counter means last-writer-wins is
    // safe and self-correcting for the reader).
    let pid = std::process::id();
    let state_tmp = runtime_dir.join(format!("spectty-{session_id}.{pid}.state.tmp"));

    let state_path_str = state_path.to_string_lossy().into_owned();
    let state_tmp_str = state_tmp.to_string_lossy().into_owned();

    // ── 5. Call the pure handler ──────────────────────────────────────────────
    handle_event(
        event_name,
        session_id,
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
    //
    // Uses the DI seam (run_with session_id: None) instead of mutating the
    // process-global env. No env mutation = no race with parallel test threads.
    #[test]
    fn spectty_hook_rejects_missing_session_id() {
        let args: Vec<String> = vec![
            "spectty-hook".to_string(),
            "--event".to_string(),
            "Stop".to_string(),
        ];
        // Pass session_id: None to simulate absent SPECTTY_SESSION_ID.
        let result = run_with(&args, None, None, &mut |_| None);

        assert!(
            matches!(result, Err(RunError::MissingSessionId)),
            "missing SPECTTY_SESSION_ID must return MissingSessionId, got: {result:?}"
        );
    }

    // ── WU-10.5: Slice 2 event names are accepted by run_with ────────────────
    //
    // These tests confirm that `Permission`, `SessionEnd`, and `StopFailure`
    // are valid `--event` values through the full `run_with` path. They use a
    // real tmpdir (created inline) to avoid the MissingRuntimeDir gate so the
    // parse path is exercised; the actual file write is not inspected here
    // (that behaviour is covered by handler.rs tests).

    #[test]
    fn spectty_hook_accepts_permission_event() {
        let dir = std::env::temp_dir().join("spectty_hook_test_permission_event");
        let _ = std::fs::create_dir_all(&dir);
        let args: Vec<String> = vec![
            "spectty-hook".to_string(),
            "--event".to_string(),
            "Permission".to_string(),
        ];
        let result = run_with(&args, Some("test-permission"), Some(&dir), &mut |_| None);
        assert!(
            result.is_ok(),
            "Permission must be accepted as a valid event, got: {result:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spectty_hook_accepts_session_end_event() {
        let dir = std::env::temp_dir().join("spectty_hook_test_session_end_event");
        let _ = std::fs::create_dir_all(&dir);
        let args: Vec<String> = vec![
            "spectty-hook".to_string(),
            "--event".to_string(),
            "SessionEnd".to_string(),
        ];
        let result = run_with(&args, Some("test-session-end"), Some(&dir), &mut |_| None);
        assert!(
            result.is_ok(),
            "SessionEnd must be accepted as a valid event, got: {result:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spectty_hook_accepts_stop_failure_event() {
        let dir = std::env::temp_dir().join("spectty_hook_test_stop_failure_event");
        let _ = std::fs::create_dir_all(&dir);
        let args: Vec<String> = vec![
            "spectty-hook".to_string(),
            "--event".to_string(),
            "StopFailure".to_string(),
        ];
        let result = run_with(&args, Some("test-stop-failure"), Some(&dir), &mut |_| None);
        assert!(
            result.is_ok(),
            "StopFailure must be accepted as a valid event, got: {result:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 4.4: non-existent runtime dir → RunError::MissingRuntimeDir ──────────
    //
    // Uses the DI seam (run_with runtime_dir: Some(non_existent_path)) instead
    // of mutating $HOME / $XDG_DATA_HOME. No env mutation = no race.
    #[test]
    fn spectty_hook_rejects_missing_runtime_dir() {
        let args: Vec<String> = vec![
            "spectty-hook".to_string(),
            "--event".to_string(),
            "Stop".to_string(),
        ];
        // Pass a path that does not exist on disk.
        let nonexistent = std::path::Path::new("/tmp/spectty_hook_test_no_such_runtime_dir_xyz");
        let result = run_with(
            &args,
            Some("test-session-4-4"),
            Some(nonexistent),
            &mut |_| None,
        );

        assert!(
            matches!(result, Err(RunError::MissingRuntimeDir)),
            "non-existent runtime dir must return MissingRuntimeDir, got: {result:?}"
        );
    }
}
