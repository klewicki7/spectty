//! [`DiffExplainerPort`] — the Core seam for turning a diff into a [`DiffExplanation`]
//! (D35).
//!
//! The diff pipeline (PR-5) hands a non-empty `git diff HEAD` to this port and receives a
//! [`DiffExplanation`] (a one-line summary + per-file rationale) to store on the Session and
//! surface to the UI. The concrete implementation (PR-5 `VibeLensMcpAdapter`) builds the
//! explanation and pushes it to the VibeLens MCP server for display; the trait is the
//! swappable abstraction so the explanation source is not baked into Core.
//!
//! This port is a PURE, **SYNC** trait — the stdio/JSON-RPC subprocess work lives in the
//! adapter, bridged via the dedicated-runtime / `block_on` discipline (D26). Core gains NO
//! `tokio`/MCP/`reqwest` dependency, keeping the R6 quarantine green.
//!
//! NOTE (Tasks-phase check): `async-trait` is NOT a Core dependency, so this trait MUST use
//! a sync signature. The design's `#[async_trait]` sketch is overridden by its own NOTE.

use std::path::Path;

use thiserror::Error;

use crate::entities::diff::DiffExplanation;

/// Errors from a [`DiffExplainerPort`] implementation.
///
/// The diff pipeline (PR-5) treats every variant as a "degrade, retain the previous
/// explanation, surface an unavailable/parse-error state, do NOT crash the session" signal
/// (diff-pipeline spec). The variants distinguish the failure modes for logging/surfacing.
#[derive(Debug, Error)]
pub enum ExplainError {
    /// The explainer backend was unreachable / timed out / errored (e.g. the VibeLens
    /// subprocess could not be reached).
    #[error("diff explainer unavailable: {0}")]
    Unavailable(String),
    /// The explainer responded but its output could not be parsed into a [`DiffExplanation`].
    #[error("diff explainer parse error: {0}")]
    Parse(String),
}

/// Port for explaining a unified diff as a [`DiffExplanation`] (D35).
///
/// `explain` takes the raw unified `git diff HEAD` text plus the `workspace` path (for the
/// adapter to attribute the review to the right project) and returns the per-file rationale.
/// Every failure is an [`ExplainError`] the pipeline degrades on — it NEVER panics.
///
/// `&self` + `Send + Sync` so the adapter can be shared across concurrent Sessions behind
/// `Arc<dyn DiffExplainerPort>`, mirroring the other Core ports.
pub trait DiffExplainerPort: Send + Sync {
    /// Explain the unified `diff` for `workspace`, yielding a [`DiffExplanation`].
    fn explain(&self, diff: &str, workspace: &Path) -> Result<DiffExplanation, ExplainError>;
}
