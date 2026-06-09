//! Hook state-file types and pure mapping functions (WU-3).
//!
//! ALL functions here are PURE: no I/O, no side effects. The impure shell (file
//! reads) lives in [`super::reader`].
//!
//! The state-file JSON shape (sidecar ↔ reader contract):
//! ```json
//! { "event": "Stop", "ts": 7, "session_id": "42" }
//! ```
//!
//! `ts` is a MONOTONIC COUNTER owned by the sidecar (WU-4), NOT a wall-clock time
//! (D22). The reader uses strict-greater-than to consume each event exactly once.

use serde::Deserialize;
use spectty_core::{Observed, ProvisioningError};

/// Lifecycle events the Spectty hook sidecar can report.
///
/// Variant names match the `--event <Name>` CLI argument and the `"event"` field in
/// the state-file JSON. These are Spectty's vocabulary, NOT Claude Code's raw hook
/// names (e.g. `Stop` here corresponds to Claude's `Stop` hook, but `Submit` maps to
/// Claude's `UserPromptSubmit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum HookEvent {
    /// Claude's `UserPromptSubmit` hook: the user sent a prompt → agent is working.
    Submit,
    /// Claude's `Stop` hook: the agent turn ended cleanly → agent is ready/idle.
    Stop,
    /// Claude's `Notification` hook (permission-prompt matcher): awaiting human input.
    Permission,
    /// Claude's `SessionEnd` hook: the session finished.
    SessionEnd,
    /// Claude's `Stop` / `SubagentStop` failure: the agent errored.
    StopFailure,
}

/// The atomic state file the `spectty-hook` sidecar writes.
///
/// `ts` is a sidecar-owned monotonic counter (D22): the sidecar reads the prior
/// `ts` from an existing state file (default 0) and writes `ts + 1`. The reader
/// consumes an event only when `ts > last_ts` (strictly greater).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookState {
    /// Which lifecycle event fired.
    pub event: HookEvent,
    /// Monotonic counter (WU-4 increments this; NOT wall-clock).
    pub ts: u64,
    /// `$SPECTTY_SESSION_ID` — correlates this state file with the running session.
    pub session_id: String,
}

/// Wire shape for `serde_json::from_str` — matches the JSON exactly.
#[derive(Deserialize)]
struct HookStateWire {
    event: HookEvent,
    ts: u64,
    session_id: String,
}

/// Parse a state-file JSON string into a [`HookState`].
///
/// Returns [`ProvisioningError::Parse`] for malformed JSON or an unrecognized
/// `event` string.
pub fn parse_state_file(json: &str) -> Result<HookState, ProvisioningError> {
    let wire: HookStateWire =
        serde_json::from_str(json).map_err(|e| ProvisioningError::Parse(e.to_string()))?;
    Ok(HookState {
        event: wire.event,
        ts: wire.ts,
        session_id: wire.session_id,
    })
}

/// Map a [`HookEvent`] to the [`Observed`] variant that drives `transition()`.
///
/// This is a PURE const-table function (D24): hooks feed the SAME `observe_and_diff`
/// path as PTY bytes; `transition()` is UNCHANGED. No I/O, no side effects.
///
/// | HookEvent    | Observed         | Transition covered          |
/// |---|---|---|
/// | Submit       | Working          | Idle/Starting → Running     |
/// | Stop         | Ready            | Running → Idle (PRIMARY FIX)|
/// | Permission   | NeedsInput       | Running → AwaitingInput     |
/// | SessionEnd   | Finished         | * → Completed               |
/// | StopFailure  | Failed           | * → Error                   |
pub fn event_to_observed(event: HookEvent) -> Observed {
    match event {
        HookEvent::Submit => Observed::Working,
        HookEvent::Stop => Observed::Ready,
        HookEvent::Permission => Observed::NeedsInput,
        HookEvent::SessionEnd => Observed::Finished,
        HookEvent::StopFailure => Observed::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_state_file ──────────────────────────────────────────────────────

    #[test]
    fn parse_state_file_valid_stop_event() {
        let json = r#"{"event":"Stop","ts":7,"session_id":"42"}"#;
        let state = parse_state_file(json).expect("valid JSON must parse");
        assert_eq!(state.event, HookEvent::Stop);
        assert_eq!(state.ts, 7);
        assert_eq!(state.session_id, "42");
    }

    #[test]
    fn parse_state_file_valid_submit_event() {
        let json = r#"{"event":"Submit","ts":1,"session_id":"s-abc"}"#;
        let state = parse_state_file(json).expect("valid");
        assert_eq!(state.event, HookEvent::Submit);
        assert_eq!(state.ts, 1);
    }

    #[test]
    fn parse_state_file_valid_permission_event() {
        let json = r#"{"event":"Permission","ts":3,"session_id":"s-1"}"#;
        let state = parse_state_file(json).expect("valid");
        assert_eq!(state.event, HookEvent::Permission);
    }

    #[test]
    fn parse_state_file_valid_session_end_event() {
        let json = r#"{"event":"SessionEnd","ts":10,"session_id":"s-1"}"#;
        let state = parse_state_file(json).expect("valid");
        assert_eq!(state.event, HookEvent::SessionEnd);
    }

    #[test]
    fn parse_state_file_valid_stop_failure_event() {
        let json = r#"{"event":"StopFailure","ts":2,"session_id":"s-1"}"#;
        let state = parse_state_file(json).expect("valid");
        assert_eq!(state.event, HookEvent::StopFailure);
    }

    #[test]
    fn parse_state_file_malformed_json_returns_parse_error() {
        let err = parse_state_file("{not valid json").expect_err("malformed must error");
        assert!(
            matches!(err, ProvisioningError::Parse(_)),
            "malformed JSON must be a Parse error, got: {err:?}"
        );
    }

    #[test]
    fn parse_state_file_unrecognized_event_returns_parse_error() {
        // serde's derived Deserialize will reject unknown enum variants.
        let json = r#"{"event":"UnknownHook","ts":1,"session_id":"s-1"}"#;
        let err = parse_state_file(json).expect_err("unknown event must error");
        assert!(
            matches!(err, ProvisioningError::Parse(_)),
            "unrecognized event must be a Parse error, got: {err:?}"
        );
    }

    #[test]
    fn parse_state_file_missing_field_returns_parse_error() {
        // Missing `session_id` → serde error.
        let json = r#"{"event":"Stop","ts":5}"#;
        let err = parse_state_file(json).expect_err("missing field must error");
        assert!(matches!(err, ProvisioningError::Parse(_)));
    }

    // ── event_to_observed (table) ─────────────────────────────────────────────

    #[test]
    fn event_to_observed_submit_maps_to_working() {
        assert_eq!(event_to_observed(HookEvent::Submit), Observed::Working);
    }

    #[test]
    fn event_to_observed_stop_maps_to_ready() {
        // PRIMARY FIX: Stop hook → Ready → transition(Running, Ready) = Idle
        assert_eq!(event_to_observed(HookEvent::Stop), Observed::Ready);
    }

    #[test]
    fn event_to_observed_permission_maps_to_needs_input() {
        assert_eq!(event_to_observed(HookEvent::Permission), Observed::NeedsInput);
    }

    #[test]
    fn event_to_observed_session_end_maps_to_finished() {
        assert_eq!(event_to_observed(HookEvent::SessionEnd), Observed::Finished);
    }

    #[test]
    fn event_to_observed_stop_failure_maps_to_failed() {
        assert_eq!(event_to_observed(HookEvent::StopFailure), Observed::Failed);
    }

    /// Full 5-row table test: every event maps to its design-specified Observed
    /// variant. This is the DATA-completeness pin — adding a new event without
    /// updating this table will cause it to fail.
    #[test]
    fn event_to_observed_full_table() {
        let table = [
            (HookEvent::Submit, Observed::Working),
            (HookEvent::Stop, Observed::Ready),
            (HookEvent::Permission, Observed::NeedsInput),
            (HookEvent::SessionEnd, Observed::Finished),
            (HookEvent::StopFailure, Observed::Failed),
        ];
        for (event, expected) in table {
            assert_eq!(
                event_to_observed(event),
                expected,
                "event_to_observed({event:?}) must be {expected:?}"
            );
        }
    }
}
