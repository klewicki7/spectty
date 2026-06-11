use serde::{Deserialize, Serialize};

use crate::entities::agent_spec::AgentSpec;
use crate::entities::agent_status::AgentStatus;
use crate::entities::diff::DiffExplanation;
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
/// Cost (beyond a future skeleton), and Checkpoint fields remain deferred.
///
/// M4 (D34) adds the diff-dedup state for VibeLens: [`last_diff`](Self::last_diff) holds the
/// most recent [`DiffExplanation`] and [`last_diff_hash`](Self::last_diff_hash) its diff
/// hash, so the pipeline can skip a redundant explanation when the working-tree diff has not
/// changed. Both are `Option` (a fresh session has no diff yet) and `#[serde(default)]` so a
/// pre-M4 persisted Session payload still deserializes.
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
    /// The most recent diff explanation surfaced to the UI, if any (M4 / D34).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_diff: Option<DiffExplanation>,
    /// The hash of the diff that produced [`last_diff`](Self::last_diff); the pipeline
    /// compares against it to dedup redundant explanations (M4 / D34).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_diff_hash: Option<u64>,
}

impl Session {
    /// Store a freshly computed diff explanation and its hash (D34).
    ///
    /// The pipeline calls this only after confirming `hash` differs from
    /// [`last_diff_hash`](Self::last_diff_hash) (hash-dedup), so this method unconditionally
    /// overwrites both fields. It is a pure mutation — no I/O, no eventing.
    pub fn update_diff(&mut self, explanation: DiffExplanation, hash: u64) {
        self.last_diff = Some(explanation);
        self.last_diff_hash = Some(hash);
    }
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
            last_diff: None,
            last_diff_hash: None,
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
        // A fresh session has no diff yet (M4).
        assert_eq!(session.last_diff, None);
        assert_eq!(session.last_diff_hash, None);

        let json = serde_json::to_string(&session).expect("serialize");
        let back: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(session, back);
    }

    // WU-6.2: update_diff stores the explanation + hash; the same-hash dedup decision is
    // observable on the aggregate (the pipeline compares the new hash against last_diff_hash).
    #[test]
    fn session_update_diff_stores_hash() {
        use crate::entities::diff::{DiffExplanation, FileExplanation};

        let mut session = sample_session();
        let explanation = DiffExplanation {
            files: vec![FileExplanation {
                path: "src/lib.rs".to_string(),
                rationale: "exported the flag".to_string(),
            }],
            summary: "export the flag".to_string(),
        };

        session.update_diff(explanation.clone(), 0xDEAD_BEEF);
        assert_eq!(session.last_diff, Some(explanation));
        assert_eq!(session.last_diff_hash, Some(0xDEAD_BEEF));

        // Same-hash detection: the aggregate exposes the prior hash so the pipeline can skip
        // a redundant explanation when the diff has not changed.
        assert_eq!(
            session.last_diff_hash,
            Some(0xDEAD_BEEF),
            "an unchanged hash must be observable so the pipeline can dedup"
        );
    }

    // M4 (D34): a pre-M4 persisted Session payload (no diff fields) MUST still deserialize —
    // last_diff / last_diff_hash default to None via #[serde(default)].
    #[test]
    fn pre_m4_session_payload_without_diff_fields_deserializes() {
        let payload = r#"{
            "id":"s-9","workspace":"/repo",
            "agent":{"kind":"claude-code","command":null,"tier":"Cooperative"},
            "status":"Starting","title":"legacy","created_at":1700000
        }"#;
        let session: Session =
            serde_json::from_str(payload).expect("a pre-M4 payload must deserialize");
        assert_eq!(session.last_diff, None);
        assert_eq!(session.last_diff_hash, None);
    }

    // WU-6.3: the three new Core ports are object-safe, SYNC, and Send + Sync. This is a
    // COMPILE-TIME proof — if any port grew an async (async-trait) signature or stopped being
    // object-safe, this would fail to compile, pinning the Core-quarantine port shape (M4-REQ-12).
    #[test]
    fn core_ports_are_object_safe_and_sync() {
        use crate::ports::{DiffExplainerPort, FileWatchPort, GitPort};

        fn _g(_: &dyn GitPort) {}
        fn _e(_: &dyn DiffExplainerPort) {}
        fn _w(_: &dyn FileWatchPort) {}

        // Behind Arc<dyn _> + Send + Sync (the shape the pipeline shares across sessions).
        fn _assert_send_sync<T: Send + Sync + ?Sized>() {}
        _assert_send_sync::<dyn GitPort>();
        _assert_send_sync::<dyn DiffExplainerPort>();
        _assert_send_sync::<dyn FileWatchPort>();
    }
}
