use serde::{Deserialize, Serialize};

use crate::entities::agent_status::AgentStatus;
use crate::entities::workspace::WorkspaceId;

/// Identity of a [`Session`].
///
/// M0: behaviorless placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

/// An agent session within a workspace.
///
/// M0: behaviorless placeholder. The Worktree, Spec, Cost, Diff, and Checkpoint
/// fields are deferred to later milestones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub workspace: WorkspaceId,
    pub status: AgentStatus,
    pub title: String,
}
