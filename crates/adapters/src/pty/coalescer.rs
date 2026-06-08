//! Pure size-OR-time output coalescer for PTY byte streams.
//!
//! A raw PTY read loop can produce many tiny reads (one per keystroke echo) or
//! one large burst (a screen repaint). Forwarding every read straight to the UI
//! Channel would either flood the IPC boundary with tiny messages or block on an
//! oversized one. The [`Coalescer`] batches bytes on a HYBRID policy:
//!
//! - flush when the buffer reaches `max_chunk` bytes (size threshold), splitting
//!   an oversized push so a single returned chunk never exceeds `max_chunk`;
//! - flush whatever is buffered once `flush_interval` has elapsed since the last
//!   flush (time threshold), so a trickle of bytes still reaches the UI promptly;
//! - never emit an empty chunk.
//!
//! Time is INJECTED via [`Instant`] parameters rather than read from the clock,
//! so every flush decision is deterministic and unit-testable without sleeping.

use std::time::{Duration, Instant};

/// Hybrid size-OR-time batcher. Pure: no PTY, no thread, no I/O.
///
/// The buffer is reused across pushes; the only per-flush allocation is the
/// returned chunk. All flush methods are `#[must_use]` because a dropped chunk
/// is lost output.
#[derive(Debug)]
pub struct Coalescer {
    buf: Vec<u8>,
    max_chunk: usize,
    flush_interval: Duration,
    last_flush: Instant,
}

impl Coalescer {
    /// Create a coalescer that flushes at `max_chunk` bytes or after
    /// `flush_interval` has elapsed since `now` (the initial `last_flush`).
    #[must_use]
    pub fn new(max_chunk: usize, flush_interval: Duration, now: Instant) -> Self {
        Self {
            buf: Vec::with_capacity(max_chunk),
            max_chunk,
            flush_interval,
            last_flush: now,
        }
    }

    /// Append `data` and, if the buffer now meets the `max_chunk` size
    /// threshold, return exactly `max_chunk` bytes (remainder stays buffered).
    /// Returns `None` if the size threshold is not reached.
    #[must_use]
    pub fn push(&mut self, data: &[u8], now: Instant) -> Option<Vec<u8>> {
        self.buf.extend_from_slice(data);
        if self.buf.len() >= self.max_chunk {
            let chunk = self.split_chunk();
            self.last_flush = now;
            return Some(chunk);
        }
        None
    }

    /// Flush whatever is buffered if `flush_interval` has elapsed since the last
    /// flush. Returns `None` if the interval has not elapsed or the buffer is
    /// empty (never an empty chunk).
    #[must_use]
    pub fn drain_due(&mut self, now: Instant) -> Option<Vec<u8>> {
        if self.buf.is_empty() {
            return None;
        }
        if now.duration_since(self.last_flush) >= self.flush_interval {
            self.last_flush = now;
            return Some(self.take_buffer());
        }
        None
    }

    /// Flush all remaining bytes unconditionally (used on EOF). Returns `None`
    /// when the buffer is empty so EOF on an empty buffer emits nothing.
    #[must_use]
    pub fn drain_all(&mut self) -> Option<Vec<u8>> {
        if self.buf.is_empty() {
            return None;
        }
        Some(self.take_buffer())
    }

    /// Split off exactly `max_chunk` bytes from the front of the buffer, keeping
    /// the remainder buffered for the next flush.
    fn split_chunk(&mut self) -> Vec<u8> {
        let rest = self.buf.split_off(self.max_chunk);
        // `self.buf` now holds the first `max_chunk` bytes; swap it out as the
        // returned chunk and restore the remainder as the live buffer so the
        // buffer's spare capacity is preserved for reuse.
        std::mem::replace(&mut self.buf, rest)
    }

    /// Take the entire buffer, leaving an empty (capacity-retaining) buffer.
    fn take_buffer(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: usize = 8;
    const INTERVAL: Duration = Duration::from_millis(10);

    #[test]
    fn coalescer_flushes_when_size_threshold_reached() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(MAX, INTERVAL, t0);

        // Push exactly MAX + 2 bytes in one go.
        let chunk = c.push(b"ABCDEFGHij", t0);

        assert_eq!(
            chunk.as_deref(),
            Some(&b"ABCDEFGH"[..]),
            "size flush must return exactly max_chunk bytes"
        );
    }

    #[test]
    fn coalescer_splits_oversized_push_at_max_chunk() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(MAX, INTERVAL, t0);

        // First push is far larger than max_chunk: only one max_chunk chunk
        // comes out, the remainder stays buffered.
        let first = c.push(b"0123456789abcdef", t0);
        assert_eq!(
            first.as_deref(),
            Some(&b"01234567"[..]),
            "oversized push returns one exact max_chunk chunk"
        );

        // The remaining 8 bytes are buffered; an EOF drain returns them.
        let rest = c.drain_all();
        assert_eq!(
            rest.as_deref(),
            Some(&b"89abcdef"[..]),
            "remainder beyond max_chunk stays buffered"
        );
    }

    #[test]
    fn coalescer_does_not_flush_below_size_and_time() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(MAX, INTERVAL, t0);

        // 3 bytes < MAX, and time has not advanced past the interval.
        let chunk = c.push(b"abc", t0);

        assert_eq!(
            chunk, None,
            "below the size threshold and within the interval there is no flush"
        );
    }

    #[test]
    fn coalescer_drain_due_flushes_after_interval() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(MAX, INTERVAL, t0);

        // Buffer a few bytes (under the size threshold, so no flush yet).
        assert_eq!(c.push(b"hi", t0), None);

        // Before the interval elapses: nothing.
        let early = c.drain_due(t0 + Duration::from_millis(5));
        assert_eq!(early, None, "time flush must not fire before the interval");

        // At/after the interval: the buffered bytes are returned.
        let due = c.drain_due(t0 + INTERVAL);
        assert_eq!(
            due.as_deref(),
            Some(&b"hi"[..]),
            "time flush returns buffered bytes once the interval elapses"
        );
    }

    #[test]
    fn coalescer_drain_all_flushes_remainder_on_eof_and_empty_yields_none() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(MAX, INTERVAL, t0);

        assert_eq!(
            c.push(b"tail", t0),
            None,
            "below threshold buffers silently"
        );

        let remainder = c.drain_all();
        assert_eq!(
            remainder.as_deref(),
            Some(&b"tail"[..]),
            "drain_all flushes the EOF remainder"
        );

        // Empty buffer must never produce a chunk from either drain path.
        assert_eq!(c.drain_all(), None, "drain_all on an empty buffer is None");
        assert_eq!(
            c.drain_due(t0 + INTERVAL * 10),
            None,
            "drain_due on an empty buffer is None even long after the interval"
        );
    }
}
