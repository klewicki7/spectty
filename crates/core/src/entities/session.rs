use serde::{Deserialize, Serialize};

use crate::entities::agent_spec::AgentSpec;
use crate::entities::agent_status::AgentStatus;
use crate::entities::workspace::WorkspaceId;
use crate::ports::clock::Timestamp;

/// Identity of a [`Session`].
///
/// M2 (D13): the SessionId is also the PtyId — `SessionRegistry::mint_id` is the
/// sole minter and the two registries (Core aggregate + OS-handle) are keyed by
/// this same string in lockstep.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

/// An agent session within a workspace.
///
/// M2 grows the M0 placeholder toward the domain model: a Session now names the
/// `agent` it runs and the `created_at` instant it was minted. The Worktree, Spec,
/// Cost (beyond a future skeleton), Diff, and Checkpoint fields remain deferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub workspace: WorkspaceId,
    /// Which agent this Session runs and at what tier (M2).
    pub agent: AgentSpec,
    pub status: AgentStatus,
    pub title: String,
    /// When the Session was minted, via the injected `ClockPort` (M2).
    pub created_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::agent_spec::{AgentKind, AgentTier};

    fn sample_session() -> Session {
        Session {
            id: SessionId("s-1".to_string()),
            workspace: WorkspaceId("/repo".to_string()),
            agent: AgentSpec {
                kind: AgentKind("claude-code".to_string()),
                command: None,
                tier: AgentTier::Cooperative,
            },
            status: AgentStatus::Starting,
            title: "Fix the auth bug".to_string(),
            created_at: Timestamp(1_700_000),
        }
    }

    #[test]
    fn session_exposes_m2_fields_and_round_trips() {
        let session = sample_session();
        // The M2 fields are present and addressable.
        assert_eq!(session.id, SessionId("s-1".to_string()));
        assert_eq!(session.workspace, WorkspaceId("/repo".to_string()));
        assert_eq!(session.agent.kind, AgentKind("claude-code".to_string()));
        assert_eq!(session.status, AgentStatus::Starting);
        assert_eq!(session.title, "Fix the auth bug");
        assert_eq!(session.created_at, Timestamp(1_700_000));

        let json = serde_json::to_string(&session).expect("serialize");
        let back: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(session, back);
    }
}
