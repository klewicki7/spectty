//! Diff-explanation adapters (D36/G2).
//!
//! [`vibelens`] holds [`VibeLensMcpAdapter`](vibelens::VibeLensMcpAdapter): the
//! [`DiffExplainerPort`](spectty_core::ports::DiffExplainerPort) implementation that BUILDS
//! a `DiffExplanation` locally and PUSHES it to the VibeLens MCP server as a display sink
//! (per the G2 finding — VibeLens is a write-only presentation surface, not an explanation
//! source).

pub mod vibelens;
