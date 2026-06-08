use serde::{Deserialize, Serialize};

use crate::ports::clock::Timestamp;

/// A normalized, decoded view of recent PTY output for
/// `AgentRunner::detect_status`.
///
/// Core serde type (it crosses the port boundary). It carries NO `Instant` and NO
/// raw ANSI — the producer in the adapter layer strips ANSI and windows the text
/// before constructing this. Time is expressed via the serde-safe [`Timestamp`]
/// plus a precomputed `idle_ms`, so a runner's `detect_status` is a PURE function
/// of the signal and never touches a clock (D10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputSignal {
    /// ANSI-stripped printable text of the last N chars (bounded rolling window).
    pub text_window: String,
    /// True while bytes are arriving within the quiesce window; drives idle
    /// heuristics.
    pub is_active: bool,
    /// Child exit code once the process has exited (`None` while running).
    pub exit_code: Option<i32>,
    /// Timestamp of the most recent byte (ClockPort-derived, serde-safe).
    pub last_byte_at: Timestamp,
    /// Elapsed millis since the last byte AS OF signal construction. This is the
    /// field a `GenericRunner` reads for idle-timeout — precomputed at the producer
    /// so `detect_status` stays a pure function of the signal (no clock access
    /// inside the Core port impl).
    pub idle_ms: u64,
}

/// A pre-canned answer the UI can offer for a known prompt (skeleton in M2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickAction {
    pub id: String,
    pub label: String,
    pub bytes: Vec<u8>,
}

/// A token/cost delta (skeleton in M2 — `parse_cost` returns `None`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CostDelta {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_usd: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_signal() -> OutputSignal {
        OutputSignal {
            text_window: "claude> ".to_string(),
            is_active: false,
            exit_code: None,
            last_byte_at: Timestamp(1_000),
            idle_ms: 250,
        }
    }

    #[test]
    fn output_signal_round_trips_and_carries_no_instant() {
        let signal = sample_signal();
        let json = serde_json::to_string(&signal).expect("serialize");
        let back: OutputSignal = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(signal, back);
        // The time field is a plain millis integer (Timestamp), never an Instant:
        // an Instant could not serialize at all, so a successful round-trip proves
        // the seam. Make the millis representation explicit too.
        let value: serde_json::Value = serde_json::from_str(&json).expect("value");
        assert_eq!(value["last_byte_at"], serde_json::json!(1_000));
    }

    #[test]
    fn output_signal_constructible_without_pty() {
        // No PTY, no adapter, no clock — just plain owned data.
        let signal = OutputSignal {
            text_window: String::new(),
            is_active: true,
            exit_code: Some(0),
            last_byte_at: Timestamp(0),
            idle_ms: 0,
        };
        assert!(signal.is_active);
        assert_eq!(signal.exit_code, Some(0));
    }

    #[test]
    fn quick_action_round_trips_through_serde() {
        let action = QuickAction {
            id: "yes".to_string(),
            label: "Approve".to_string(),
            bytes: vec![b'y', b'\n'],
        };
        let json = serde_json::to_string(&action).expect("serialize");
        let back: QuickAction = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(action, back);
    }

    #[test]
    fn cost_delta_round_trips_through_serde() {
        let delta = CostDelta {
            input_tokens: 10,
            output_tokens: 20,
            estimated_usd: 0.0,
        };
        let json = serde_json::to_string(&delta).expect("serialize");
        let back: CostDelta = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(delta, back);
    }
}
