use serde::{Deserialize, Serialize};

/// Identity of a [`Workspace`].
///
/// M0: behaviorless placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceId(pub String);

/// A workspace rooted at a filesystem path.
///
/// M0: behaviorless placeholder — no domain behavior. Worktree management and
/// git integration are deferred (M2/M4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub root: String,
}
