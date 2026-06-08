//! `pty` — the M1 PTY adapter layer.
//!
//! This module owns the *real* PTY backend (`portable-pty`) plus the pure,
//! testable pieces around it. It is the ADAPTER side of the hexagon: nothing
//! here is a `spectty-core` port, and `spectty-core` never depends on any of it.
//!
//! Layout:
//! - [`Coalescer`]: a pure, allocation-disciplined size-OR-time output batcher.
//!   It has no knowledge of PTYs or threads — time is injected so the flush
//!   logic is deterministic under test.
//! - `config` / `transport` / `adapter` (added in WU-3): spawn configuration,
//!   the in-adapter `PtyTransport` fake seam, and the `portable-pty`-backed
//!   `PtyAdapter`.

pub mod coalescer;

pub use coalescer::Coalescer;
