//! `SystemClock` — the production [`ClockPort`](spectty_core::ClockPort) adapter.
//!
//! The Core reads time ONLY through the [`ClockPort`] trait so `std::time` never
//! crosses the hexagonal boundary (D10). This adapter is the single place a real
//! wall clock is touched: it yields a serde-safe [`Timestamp`] of millis elapsed
//! since an opaque process epoch captured at construction.
//!
//! Millis-since-epoch (not Unix epoch) is intentional — the OutputSignal pipeline
//! only ever computes DELTAS (`idle_ms`, "active within the last tick"), so a
//! monotonic-ish, comparable-across-the-boundary value is all that is required and
//! it never has to be reconciled against wall-clock time.

use std::time::Instant;

use spectty_core::{ClockPort, Timestamp};

/// Real time source backing the OutputSignal pipeline's `clock.now()` stamps.
///
/// Captures a process-local epoch at construction and reports millis elapsed since
/// it. Built on [`Instant`] (monotonic), so the stamps never jump backward on an
/// NTP step the way a wall clock can — exactly what the saturating
/// [`Timestamp::elapsed_ms_until`] delta math expects.
#[derive(Debug)]
pub struct SystemClock {
    epoch: Instant,
}

impl SystemClock {
    /// Build a clock whose epoch is "now". All [`now`](Self::now) values are millis
    /// elapsed since this instant.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockPort for SystemClock {
    fn now(&self) -> Timestamp {
        // `as u64` truncation is harmless: the epoch is process-local, so the millis
        // value cannot realistically exceed u64 within a session's lifetime.
        Timestamp(self.epoch.elapsed().as_millis() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_now_is_monotonic_non_decreasing() {
        let clock = SystemClock::new();
        let first = clock.now();
        let second = clock.now();
        // Built on a monotonic `Instant`, so a later read is never earlier.
        assert!(second >= first, "now() must be non-decreasing");
    }

    #[test]
    fn system_clock_starts_near_zero_from_its_epoch() {
        // Millis are measured from the construction epoch, so the very first read is
        // close to zero (and certainly small) rather than a Unix-epoch magnitude.
        let clock = SystemClock::new();
        assert!(
            clock.now().0 < 1_000,
            "the first read must be a small delta from the construction epoch"
        );
    }
}
