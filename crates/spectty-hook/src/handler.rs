//! Pure event-handler seam for `spectty-hook`.
//!
//! [`handle_event`] is a pure function — it does NOT perform any I/O. The caller
//! (`main`) injects:
//!
//! - `read_prior`: a closure that reads the current state file; returns `Ok(Some(ts))`
//!   when a prior file exists, `Ok(None)` when absent, `Err` on I/O error (ts treated
//!   as 0 — non-fatal).
//! - `write_atomic`: a closure that atomically writes the new state JSON; returns `Ok`
//!   or `Err`.
//!
//! This seam lets the unit tests (4.1, 4.2, 4.5) exercise the core logic without
//! spawning a process or touching the filesystem.

use serde::Serialize;

/// The five lifecycle event names the sidecar accepts via `--event <Name>`.
///
/// Variant names are Spectty's vocabulary and map 1:1 to `HookEvent` in
/// `crates/adapters/src/hook/state.rs` (same string, same JSON round-trip).
/// The sidecar has its own copy (D25: no dep on spectty-adapters or spectty-core).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HookEvent {
    Submit,
    Stop,
    Permission,
    SessionEnd,
    StopFailure,
}

impl HookEvent {
    /// Parse the CLI `--event` argument into a [`HookEvent`].
    ///
    /// Returns `None` for any unrecognized string.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "Submit" => Some(Self::Submit),
            "Stop" => Some(Self::Stop),
            "Permission" => Some(Self::Permission),
            "SessionEnd" => Some(Self::SessionEnd),
            "StopFailure" => Some(Self::StopFailure),
            _ => None,
        }
    }
}

/// Wire shape written to the state file by the sidecar.
///
/// Matches the JSON schema consumed by `crates/adapters/src/hook/state.rs`
/// `parse_state_file`.
#[derive(Serialize)]
struct StateFileWire<'a> {
    event: HookEvent,
    ts: u64,
    session_id: &'a str,
}

/// Errors returned by [`handle_event`].
#[derive(Debug, PartialEq, Eq)]
pub enum HandleError {
    /// The `--event` argument was not a recognized [`HookEvent`] name.
    UnknownEvent(String),
    /// The write-atomic closure failed.
    WriteError(String),
}

/// Pure core logic: compute and write the next state file.
///
/// - Reads the prior `ts` via `read_prior` (returns `Some(ts)` on an existing
///   file, `None` when absent; I/O errors default to `ts = 0`).
/// - Serializes `{event, ts: prior + 1, session_id}` and calls `write_atomic`.
///
/// # Errors
/// Returns [`HandleError::UnknownEvent`] when `event_name` is not recognized.
/// Returns [`HandleError::WriteError`] when `write_atomic` fails.
pub fn handle_event(
    event_name: &str,
    session_id: &str,
    mut read_prior: impl FnMut() -> std::io::Result<Option<u64>>,
    mut write_atomic: impl FnMut(&str) -> std::io::Result<()>,
) -> Result<(), HandleError> {
    let event = HookEvent::parse(event_name)
        .ok_or_else(|| HandleError::UnknownEvent(event_name.to_string()))?;

    // `read_prior` returns:
    //   Ok(Some(ts)) — a valid prior state file with a ts field
    //   Ok(None)     — no prior file (first write for this session)
    //   Err(_)       — I/O error OR the file is corrupt / unparseable JSON
    //
    // Both the absent-file and corrupt-file cases collapse to ts = 0 here,
    // so next_ts starts at 1. This is intentional: corruption is rare and
    // self-correcting — once the sidecar writes ts=1, the monotonic counter
    // climbs again and the reader (StateFileReader) advances past its last_ts
    // on the next event. The reader is strict-greater, so a reset to 1 only
    // stalls delivery until the next genuine event fires (a single event latency).
    let prior_ts: u64 = read_prior().unwrap_or(None).unwrap_or(0);
    let next_ts = prior_ts + 1;

    let wire = StateFileWire {
        event,
        ts: next_ts,
        session_id,
    };
    let json = serde_json::to_string(&wire).expect("StateFileWire is always serializable");

    write_atomic(&json).map_err(|e| HandleError::WriteError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // ── 4.1: handle_event writes correct ts and event ────────────────────────

    /// Prior file exists with ts=6 → write receives ts=7.
    #[test]
    fn spectty_hook_handle_event_writes_state_file_with_prior_ts() {
        let written = std::cell::RefCell::new(None::<String>);

        let result = handle_event(
            "Stop",
            "session-abc",
            || Ok(Some(6)),
            |json| {
                *written.borrow_mut() = Some(json.to_string());
                Ok(())
            },
        );

        assert!(result.is_ok(), "handle_event must succeed: {result:?}");

        let json_str = written
            .borrow()
            .clone()
            .expect("write must have been called");
        let value: serde_json::Value =
            serde_json::from_str(&json_str).expect("written JSON must be valid");

        assert_eq!(value["event"], "Stop");
        assert_eq!(value["ts"], 7);
        assert_eq!(value["session_id"], "session-abc");
    }

    /// No prior file (None) → ts defaults to 0 → writes ts=1.
    #[test]
    fn spectty_hook_handle_event_absent_prior_file_starts_at_ts_1() {
        let written = std::cell::RefCell::new(None::<String>);

        let result = handle_event(
            "Submit",
            "s-1",
            || Ok(None),
            |json| {
                *written.borrow_mut() = Some(json.to_string());
                Ok(())
            },
        );

        assert!(result.is_ok());
        let json_str = written.borrow().clone().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(value["ts"], 1);
        assert_eq!(value["event"], "Submit");
    }

    /// Read I/O error → ts defaults to 0 → writes ts=1.
    #[test]
    fn spectty_hook_handle_event_read_error_defaults_to_ts_1() {
        let written = std::cell::RefCell::new(None::<String>);

        let result = handle_event(
            "Stop",
            "s-2",
            || Err(std::io::Error::other("disk error")),
            |json| {
                *written.borrow_mut() = Some(json.to_string());
                Ok(())
            },
        );

        assert!(result.is_ok());
        let value: serde_json::Value =
            serde_json::from_str(&written.borrow().clone().unwrap()).unwrap();
        assert_eq!(value["ts"], 1);
    }

    // ── 4.2: unknown event name returns an error ──────────────────────────────

    #[test]
    fn spectty_hook_unknown_event_returns_error() {
        let write_called = Cell::new(false);

        let result = handle_event(
            "NotARealEvent",
            "s-3",
            || Ok(None),
            |_| {
                write_called.set(true);
                Ok(())
            },
        );

        assert_eq!(
            result,
            Err(HandleError::UnknownEvent("NotARealEvent".to_string())),
            "unrecognized event name must return UnknownEvent error"
        );
        assert!(
            !write_called.get(),
            "write must NOT be called on unknown event"
        );
    }

    // ── 4.5: all five event names are accepted ────────────────────────────────

    /// Table test: every valid HookEvent name must succeed.
    #[test]
    fn spectty_hook_accepts_all_five_event_names() {
        let valid_events = ["Submit", "Stop", "Permission", "SessionEnd", "StopFailure"];

        for event_name in valid_events {
            let result = handle_event(event_name, "s-table", || Ok(None), |_| Ok(()));
            assert!(
                result.is_ok(),
                "event '{event_name}' must be accepted, got: {result:?}"
            );
        }
    }
}
