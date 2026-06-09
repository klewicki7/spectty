//! Consume-once state-file reader (WU-3, D22).
//!
//! [`StateFileReader`] holds the path of the per-session `.state` file and the
//! last consumed monotonic counter (`ts`). On each `poll()` call it reads the
//! file, parses the [`HookState`], and returns `Some(event)` ONLY when `ts >
//! last_ts` (strictly greater). If the file is absent, unchanged, or carries an
//! older `ts`, `poll` returns `None`.
//!
//! The `read` closure injected into [`StateFileReader::poll`] is the seam that
//! keeps this struct testable without real filesystem access.

use super::state::{parse_state_file, HookEvent};

/// Consume-once reader for a per-session hook state file.
///
/// Generic over a `read` closure so the filesystem is swappable in tests.
pub struct StateFileReader {
    /// Absolute path: `{runtime_dir}/spectty-{session_id}.state`.
    path: String,
    /// The last `ts` whose event was returned. `None` means "never seen" (treated
    /// as 0 for the strict-greater comparison so the very first event fires).
    last_ts: Option<u64>,
}

impl StateFileReader {
    /// Build a reader for `{runtime_dir}/spectty-{session_id}.state`.
    pub fn new(runtime_dir: &str, session_id: &str) -> Self {
        Self {
            path: format!("{runtime_dir}/spectty-{session_id}.state"),
            last_ts: None,
        }
    }

    /// The resolved state-file path (for tests and for lifecycle cleanup).
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Read and parse the state file via `read`; return `Some(event)` ONLY if the
    /// file's `ts` is strictly greater than the last consumed `ts`. Advances
    /// `last_ts` on a successful consume.
    ///
    /// - Absent file → `None` (the sidecar hasn't fired yet; that's fine).
    /// - Parse error → `None` (a half-written file; the next poll will retry).
    /// - `ts <= last_ts` → `None` (already consumed this event).
    pub fn poll(
        &mut self,
        read: &dyn Fn(&str) -> std::io::Result<Option<String>>,
    ) -> Option<HookEvent> {
        let contents = read(&self.path).ok().flatten()?;
        let state = parse_state_file(&contents).ok()?;
        let threshold = self.last_ts.unwrap_or(0);
        if state.ts > threshold {
            self.last_ts = Some(state.ts);
            Some(state.event)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // ── StateFileReader::poll consume-once semantics ──────────────────────────

    /// Helper: build a `read` closure that always returns the given JSON string.
    fn fixed_reader(json: &'static str) -> impl Fn(&str) -> std::io::Result<Option<String>> {
        move |_path| Ok(Some(json.to_string()))
    }

    /// Helper: build a `read` closure that always returns `None` (file absent).
    fn absent_reader() -> impl Fn(&str) -> std::io::Result<Option<String>> {
        |_path| Ok(None)
    }

    #[test]
    fn poll_returns_event_on_first_read_with_ts_1() {
        // Initial state: last_ts = None (treated as 0). ts=1 > 0 → Some.
        let mut reader = StateFileReader::new("/tmp", "session-1");
        let state_json = r#"{"event":"Stop","ts":1,"session_id":"session-1"}"#;
        let result = reader.poll(&fixed_reader(state_json));
        assert_eq!(result, Some(HookEvent::Stop));
    }

    #[test]
    fn poll_returns_none_on_second_read_with_same_ts() {
        // After consuming ts=7, the same ts=7 must NOT re-fire (consume-once).
        let state_json = r#"{"event":"Stop","ts":7,"session_id":"session-1"}"#;
        let mut reader = StateFileReader::new("/tmp", "session-1");

        let first = reader.poll(&fixed_reader(state_json));
        assert_eq!(first, Some(HookEvent::Stop), "first poll must return event");

        let second = reader.poll(&fixed_reader(state_json));
        assert_eq!(second, None, "second poll with same ts must return None");
    }

    #[test]
    fn poll_returns_new_event_when_ts_advances() {
        // ts=7 consumed; ts=8 is strictly greater → re-fires.
        let mut reader = StateFileReader::new("/tmp", "session-1");

        let json_ts7 = r#"{"event":"Stop","ts":7,"session_id":"session-1"}"#;
        let first = reader.poll(&fixed_reader(json_ts7));
        assert_eq!(first, Some(HookEvent::Stop));

        let json_ts8 = r#"{"event":"Submit","ts":8,"session_id":"session-1"}"#;
        let second = reader.poll(&fixed_reader(json_ts8));
        assert_eq!(second, Some(HookEvent::Submit), "ts=8 > 7 must re-fire");
    }

    #[test]
    fn poll_returns_none_when_ts_goes_backward() {
        // A state with an OLDER ts than last_ts (e.g. stale file) must NOT fire.
        let mut reader = StateFileReader::new("/tmp", "session-1");

        let json_ts5 = r#"{"event":"Stop","ts":5,"session_id":"session-1"}"#;
        reader.poll(&fixed_reader(json_ts5)); // consume ts=5

        // Now present ts=3 (older) → must NOT fire.
        let json_ts3 = r#"{"event":"Submit","ts":3,"session_id":"session-1"}"#;
        let result = reader.poll(&fixed_reader(json_ts3));
        assert_eq!(result, None, "older ts must not re-fire (stale/rewound file)");
    }

    #[test]
    fn poll_returns_none_when_file_absent() {
        // No state file yet: the sidecar hasn't fired.
        let mut reader = StateFileReader::new("/tmp", "session-1");
        let result = reader.poll(&absent_reader());
        assert_eq!(result, None, "absent file must return None");
    }

    #[test]
    fn poll_returns_none_on_parse_error() {
        // A half-written file with invalid JSON must not panic; just return None.
        let mut reader = StateFileReader::new("/tmp", "session-1");
        let bad_json = move |_: &str| -> std::io::Result<Option<String>> {
            Ok(Some("{not valid".to_string()))
        };
        let result = reader.poll(&bad_json);
        assert_eq!(result, None, "parse error must return None, not panic");
    }

    #[test]
    fn poll_path_is_runtime_dir_slash_spectty_dash_id_dot_state() {
        // Verifies the canonical path formula: {dir}/spectty-{id}.state
        let reader = StateFileReader::new("/var/run/spectty", "abc-123");
        assert_eq!(reader.path(), "/var/run/spectty/spectty-abc-123.state");
    }

    #[test]
    fn poll_ts_equal_to_last_ts_is_not_consumed() {
        // Edge: ts == last_ts (same, not greater) must return None.
        // This catches an off-by-one where `>=` was used instead of `>`.
        let mut reader = StateFileReader::new("/tmp", "session-1");

        let json_ts5 = r#"{"event":"Stop","ts":5,"session_id":"session-1"}"#;
        reader.poll(&fixed_reader(json_ts5)); // consume ts=5

        // Present the SAME ts=5 again.
        let result = reader.poll(&fixed_reader(json_ts5));
        assert_eq!(result, None, "ts == last_ts must NOT re-fire (strict-greater)");
    }

    /// Dynamic `read` closure that returns a sequence of JSON strings. Used to
    /// simulate the sidecar writing new state files between polls.
    #[test]
    fn poll_multiple_events_sequential_consume() {
        // Simulate 3 ticks: ts=1 (Submit), ts=1 (same, no-fire), ts=2 (Stop).
        let tick = Cell::new(0u32);
        let jsons = [
            r#"{"event":"Submit","ts":1,"session_id":"s1"}"#,
            r#"{"event":"Submit","ts":1,"session_id":"s1"}"#,
            r#"{"event":"Stop","ts":2,"session_id":"s1"}"#,
        ];

        let read_fn = |_path: &str| -> std::io::Result<Option<String>> {
            let t = tick.get();
            tick.set(t + 1);
            Ok(Some(jsons[t as usize].to_string()))
        };

        let mut reader = StateFileReader::new("/tmp", "s1");
        assert_eq!(reader.poll(&read_fn), Some(HookEvent::Submit)); // ts=1 fires
        assert_eq!(reader.poll(&read_fn), None);                    // ts=1 again → None
        assert_eq!(reader.poll(&read_fn), Some(HookEvent::Stop));   // ts=2 fires
    }
}
