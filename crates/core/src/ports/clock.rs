use serde::{Deserialize, Serialize};

/// Monotonic-ish elapsed time in milliseconds since an opaque process epoch.
///
/// PURE, serde-safe, and comparable across the port boundary (unlike
/// `std::time::Instant`, which is neither `Serialize` nor meaningful once it
/// crosses the IPC seam). M2 uses millis since the [`ClockPort`]'s epoch (D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// Saturating elapsed millis from `self` to `later`. Returns `0` when `later`
    /// precedes `self` (clock skew / out-of-order stamps never underflow).
    #[must_use]
    pub fn elapsed_ms_until(self, later: Timestamp) -> u64 {
        later.0.saturating_sub(self.0)
    }
}

/// Time source, injected for testability (the domain model lists `ClockPort`).
///
/// The Core reads time ONLY through this trait; the concrete `SystemClock` lives
/// in the adapter layer so `std::time` never leaks across the boundary (D10).
pub trait ClockPort: Send + Sync {
    /// The current instant as a serde-safe [`Timestamp`].
    fn now(&self) -> Timestamp;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_round_trips_through_serde() {
        let ts = Timestamp(123_456);
        let json = serde_json::to_string(&ts).expect("serialize");
        let back: Timestamp = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ts, back);
    }

    #[test]
    fn timestamp_serializes_as_plain_millis_integer() {
        let json = serde_json::to_string(&Timestamp(42)).expect("serialize");
        assert_eq!(json, "42");
    }

    #[test]
    fn elapsed_ms_until_computes_forward_delta() {
        assert_eq!(Timestamp(100).elapsed_ms_until(Timestamp(350)), 250);
    }

    #[test]
    fn elapsed_ms_until_saturates_to_zero_on_reverse_order() {
        assert_eq!(Timestamp(350).elapsed_ms_until(Timestamp(100)), 0);
    }

    #[test]
    fn timestamp_orders_by_millis() {
        assert!(Timestamp(1) < Timestamp(2));
    }
}
