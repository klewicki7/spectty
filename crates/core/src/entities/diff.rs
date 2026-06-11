//! The `DiffExplanation` — VibeLens's per-file rationale for a working-tree diff as a
//! PURE Core entity (D34).
//!
//! M4 introduces "VibeLens": when the workspace changes, the diff pipeline (PR-5) runs
//! `git diff HEAD`, builds a `DiffExplanation` (a one-line summary plus a per-file
//! rationale), stores it on the [`Session`](crate::Session), and surfaces it to the UI.
//! This module owns the SHAPE only — `serde` ONLY, no I/O, no git, no MCP client. Building
//! the explanation and talking to VibeLens are the ADAPTER's job (PR-5).
//!
//! ## The empty form
//!
//! [`DiffExplanation::empty()`] is the canonical "there is nothing to explain" value: an
//! empty file list and an empty summary. The pipeline yields it for a truly empty diff
//! WITHOUT calling the explainer at all (diff-pipeline spec), so `empty()` is the
//! cheap, allocation-light sentinel the dedup path returns.

use serde::{Deserialize, Serialize};

/// The rationale for a single changed file in a working-tree diff (D34).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileExplanation {
    /// Workspace-relative path of the changed file.
    pub path: String,
    /// Why the change was made / what it does — the per-file "why" VibeLens renders.
    pub rationale: String,
}

/// The explanation of a whole working-tree diff: a one-line `summary` plus a per-file
/// rationale list (D34).
///
/// PURE: `serde` only — no I/O, no git, no MCP. The diff pipeline (PR-5) builds this from
/// `git diff HEAD`, stores it on the [`Session`](crate::Session) via
/// [`update_diff`](crate::Session::update_diff), and the UI renders it. The empty form
/// ([`empty`](Self::empty)) represents "no diff to explain".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffExplanation {
    /// Per-file rationale entries (one per changed file).
    pub files: Vec<FileExplanation>,
    /// A one-line, human-readable summary of the whole change.
    pub summary: String,
}

impl DiffExplanation {
    /// The canonical "nothing to explain" value: no files, empty summary.
    ///
    /// The diff pipeline returns this for a truly empty diff WITHOUT invoking the
    /// explainer (diff-pipeline spec: "a truly empty diff MUST yield `DiffExplanation::
    /// empty()` with NO MCP call").
    #[must_use]
    pub fn empty() -> Self {
        Self {
            files: Vec::new(),
            summary: String::new(),
        }
    }

    /// Whether this explanation carries nothing to render — equivalent to [`empty`](Self::empty).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.summary.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // WU-6.1: empty() is well-formed (no files, empty summary) and round-trips byte-stably.
    #[test]
    fn diff_explanation_empty_is_well_formed() {
        let empty = DiffExplanation::empty();
        assert!(empty.files.is_empty(), "empty() must carry no files");
        assert!(
            empty.summary.is_empty(),
            "empty() must carry an empty summary"
        );
        assert!(empty.is_empty(), "empty() must report is_empty()");

        let json = serde_json::to_string(&empty).expect("serialize empty");
        let back: DiffExplanation = serde_json::from_str(&json).expect("deserialize empty");
        assert_eq!(empty, back, "empty() must round-trip unchanged");
    }

    // WU-6.1 (cont.): a populated explanation round-trips byte-stably and is NOT empty.
    #[test]
    fn diff_explanation_populated_round_trips() {
        let expl = DiffExplanation {
            files: vec![
                FileExplanation {
                    path: "src/lib.rs".to_string(),
                    rationale: "added the public API".to_string(),
                },
                FileExplanation {
                    path: "README.md".to_string(),
                    rationale: "documented the new flag".to_string(),
                },
            ],
            summary: "introduce the public flag and document it".to_string(),
        };
        assert!(!expl.is_empty(), "a populated explanation is not empty");

        let json = serde_json::to_string(&expl).expect("serialize");
        let back: DiffExplanation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(expl, back, "DiffExplanation must round-trip");

        let json2 = serde_json::to_string(&back).expect("re-serialize");
        assert_eq!(json, json2, "serialization must be byte-stable");
    }
}
