//! [`GitPort`] — the Core seam for reading a workspace's working-tree diff (D35).
//!
//! The diff pipeline (PR-5) calls [`GitPort::diff_head`] to obtain the unified `git diff
//! HEAD` for a workspace, hashes it for dedup, and (on change) hands it to the explainer.
//! This port is a PURE, **SYNC** trait — no `git2`, no `std::process`, no async leaks into
//! Core. The async/process work is bridged INSIDE the adapter (the dedicated-runtime /
//! `block_on` discipline chosen for `EngramAdapter`, D26), so Core gains NO new dependency
//! and the `cargo deny ... check bans` quarantine (R6) stays green.
//!
//! NOTE (Tasks-phase check): `async-trait` is NOT a Core dependency, so this trait MUST use
//! a sync signature. The design's `#[async_trait]` sketch is overridden by its own NOTE.

use std::path::Path;

use thiserror::Error;

/// Errors from a [`GitPort`] implementation.
///
/// An empty repository is NOT an error: [`GitPort::diff_head`] diffs against the empty
/// tree and returns the add-all diff (or an empty string for a truly empty working tree).
/// This enum is reserved for genuine git failures (not a repo, IO failure, ...).
#[derive(Debug, Error)]
pub enum GitError {
    /// The underlying git operation failed (not a repository, IO error, ...).
    #[error("git error: {0}")]
    Backend(String),
}

/// Port for retrieving the working-tree diff of a workspace against `HEAD` (D35).
///
/// `diff_head` returns the unified `git diff HEAD` text for `workspace`. An empty
/// repository (no commits) MUST diff against the empty tree rather than error; a truly
/// empty working tree (no changes) returns an empty string, which the pipeline maps to
/// [`DiffExplanation::empty()`](crate::entities::diff::DiffExplanation::empty). The string
/// payload keeps git's wire format out of Core — the adapter owns parsing/formatting.
///
/// `&self` + `Send + Sync` so a single adapter can be shared across concurrent Sessions
/// behind `Arc<dyn GitPort>`, mirroring [`PersistencePort`](crate::ports::PersistencePort).
pub trait GitPort: Send + Sync {
    /// Return the unified `git diff HEAD` for `workspace`.
    ///
    /// Empty repo → diff vs the empty tree (no error). Truly empty working tree → `Ok("")`.
    fn diff_head(&self, workspace: &Path) -> Result<String, GitError>;
}
