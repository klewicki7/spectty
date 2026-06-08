use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::entities::agent_spec::AgentKind;
use crate::entities::agent_status::{transition, AgentStatus, Observed};
use crate::entities::session::{Session, SessionId};

/// UI-facing projection for `list_sessions` (data-flow.md `SessionSummary`).
///
/// A serde value type carrying only the columns the session list renders, so the
/// full `Session` aggregate never has to cross the IPC boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub title: String,
    pub status: AgentStatus,
    pub agent_kind: AgentKind,
}

impl From<&Session> for SessionSummary {
    fn from(session: &Session) -> Self {
        SessionSummary {
            id: session.id.clone(),
            title: session.title.clone(),
            status: session.status,
            agent_kind: session.agent.kind.clone(),
        }
    }
}

/// Owns the `Session` aggregates. `&self` interior mutability (the `PersistencePort`
/// convention, D19) so a single registry shares across command handlers AND the
/// read-loop signal thread as one `tauri::State` behind `Arc`/`State`. The `Mutex`
/// is the ONLY mutability and is fully encapsulated here.
///
/// This is the SOLE `SessionId` minter (D13): `src-tauri`'s M1 `next_pty_id` is
/// retired and `SessionId == PtyId`, so the Core aggregate registry and the
/// OS-handle `PtyRegistry` key off the same string in lockstep with no cross-map.
///
/// The registry holds ONLY domain state — no `portable-pty` writer, no child
/// handle, no `tauri` type ever enters Core.
#[derive(Default)]
pub struct SessionRegistry {
    inner: Mutex<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    sessions: HashMap<SessionId, Session>,
    next_id: u64,
}

impl SessionRegistry {
    /// Mint a fresh, monotonic `SessionId` through a shared reference (migrates the
    /// M1 `next_pty_id` counter into the aggregate root, D13).
    pub fn mint_id(&self) -> SessionId {
        let mut inner = self.inner.lock().expect("session registry mutex poisoned");
        let id = inner.next_id;
        inner.next_id += 1;
        SessionId(id.to_string())
    }

    /// Insert a fully-formed `Session` (built after `launch_spec` + provisioning
    /// succeed).
    pub fn insert(&self, session: Session) {
        let mut inner = self.inner.lock().expect("session registry mutex poisoned");
        inner.sessions.insert(session.id.clone(), session);
    }

    /// Apply an observed signal through the pure [`transition`] INSIDE the lock
    /// (D19) — the diff is computed atomically with respect to a concurrent
    /// `remove`, avoiding a check-then-act race. Returns `Some(new)` only when the
    /// status actually CHANGED (so the caller emits `status_changed`), `None` on a
    /// legal no-op, a terminal-absorbing observation, or an absent session.
    #[must_use]
    pub fn apply_observed(&self, id: &SessionId, observed: Observed) -> Option<AgentStatus> {
        let mut inner = self.inner.lock().expect("session registry mutex poisoned");
        let session = inner.sessions.get_mut(id)?;
        let next = transition(session.status, observed);
        if next == session.status {
            return None;
        }
        session.status = next;
        Some(next)
    }

    /// Look up a `Session` by id, returning a clone of its current domain state.
    pub fn get(&self, id: &SessionId) -> Option<Session> {
        let inner = self.inner.lock().expect("session registry mutex poisoned");
        inner.sessions.get(id).cloned()
    }

    /// Project every held session into a `SessionSummary` for the session list.
    pub fn summaries(&self) -> Vec<SessionSummary> {
        let inner = self.inner.lock().expect("session registry mutex poisoned");
        inner.sessions.values().map(SessionSummary::from).collect()
    }

    /// Remove a session on close, returning the removed `Session` (its agent/title
    /// feed the `session_closed` event).
    pub fn remove(&self, id: &SessionId) -> Option<Session> {
        let mut inner = self.inner.lock().expect("session registry mutex poisoned");
        inner.sessions.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::agent_spec::{AgentSpec, AgentTier};
    use crate::entities::workspace::WorkspaceId;
    use crate::ports::clock::Timestamp;

    fn session_with(id: SessionId, status: AgentStatus) -> Session {
        Session {
            id,
            workspace: WorkspaceId("/repo".to_string()),
            agent: AgentSpec {
                kind: AgentKind("claude-code".to_string()),
                command: None,
                tier: AgentTier::Cooperative,
            },
            status,
            title: "Fix the auth bug".to_string(),
            created_at: Timestamp(1_700_000),
        }
    }

    #[test]
    fn registry_create_then_lookup_returns_same_session() {
        let registry = SessionRegistry::default();
        let id = registry.mint_id();
        registry.insert(session_with(id.clone(), AgentStatus::Starting));

        let found = registry.get(&id).expect("session present after insert");
        assert_eq!(found.id, id);
        assert_eq!(found.workspace, WorkspaceId("/repo".to_string()));
        assert_eq!(found.agent.kind, AgentKind("claude-code".to_string()));
    }

    #[test]
    fn registry_close_removes_from_lookup() {
        let registry = SessionRegistry::default();
        let id = registry.mint_id();
        registry.insert(session_with(id.clone(), AgentStatus::Starting));

        let removed = registry.remove(&id).expect("remove returns the session");
        assert_eq!(removed.id, id);
        assert!(registry.get(&id).is_none(), "lookup absent after close");
    }

    #[test]
    fn registry_mints_distinct_ids_via_shared_ref() {
        // Two mints through `&self` (no `&mut`) yield distinct, monotonic ids.
        let registry = SessionRegistry::default();
        let first = registry.mint_id();
        let second = registry.mint_id();
        assert_ne!(first, second);
        assert_eq!(first, SessionId("0".to_string()));
        assert_eq!(second, SessionId("1".to_string()));
    }

    #[test]
    fn registry_mints_unique_ids_under_concurrent_minters() {
        // Defends the TOCTOU/uniqueness claim under REAL contention (D13: `mint_id` is the
        // sole minter): many threads sharing one `Arc<SessionRegistry>` hammer `mint_id`
        // concurrently, and EVERY minted id MUST be distinct. The `&self` interior-mutex
        // increments `next_id` atomically with respect to the read, so no two threads can
        // observe the same counter value.
        use std::collections::HashSet;
        use std::sync::Arc;
        use std::thread;

        const THREADS: usize = 8;
        const MINTS_PER_THREAD: usize = 250;

        let registry = Arc::new(SessionRegistry::default());
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let registry = Arc::clone(&registry);
                thread::spawn(move || {
                    (0..MINTS_PER_THREAD)
                        .map(|_| registry.mint_id())
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        let mut all_ids = HashSet::new();
        for handle in handles {
            for id in handle.join().expect("minter thread panicked") {
                assert!(
                    all_ids.insert(id),
                    "mint_id produced a duplicate under contention"
                );
            }
        }
        assert_eq!(all_ids.len(), THREADS * MINTS_PER_THREAD);
    }

    #[test]
    fn apply_observed_returns_some_only_on_change() {
        let registry = SessionRegistry::default();
        let id = registry.mint_id();
        registry.insert(session_with(id.clone(), AgentStatus::Starting));

        // Starting + Ready → Idle: a real change, emitted.
        assert_eq!(
            registry.apply_observed(&id, Observed::Ready),
            Some(AgentStatus::Idle)
        );
        assert_eq!(
            registry.get(&id).expect("present").status,
            AgentStatus::Idle
        );

        // Idle + Ready → Idle: a legal no-op, NOT emitted.
        assert_eq!(registry.apply_observed(&id, Observed::Ready), None);
    }

    #[test]
    fn apply_observed_terminal_is_absorbing_and_unemitted() {
        let registry = SessionRegistry::default();
        let id = registry.mint_id();
        registry.insert(session_with(id.clone(), AgentStatus::Completed));

        // A terminal session absorbs every observation and emits nothing.
        assert_eq!(registry.apply_observed(&id, Observed::Working), None);
        assert_eq!(registry.apply_observed(&id, Observed::Failed), None);
        assert_eq!(
            registry.get(&id).expect("present").status,
            AgentStatus::Completed
        );
    }

    #[test]
    fn apply_observed_on_absent_session_is_none() {
        let registry = SessionRegistry::default();
        let absent = SessionId("missing".to_string());
        assert_eq!(registry.apply_observed(&absent, Observed::Ready), None);
    }

    #[test]
    fn registry_holds_no_os_handle() {
        // Structural assertion: the stored entry IS a `Session` of pure domain state.
        // A `Session` is `Serialize`/`Deserialize`, which a `portable-pty` writer or a
        // `tauri` handle never is — so round-tripping the stored value proves the
        // registry holds only domain state (no OS handle could survive serde).
        let registry = SessionRegistry::default();
        let id = registry.mint_id();
        registry.insert(session_with(id.clone(), AgentStatus::Running));

        let stored = registry.get(&id).expect("present");
        let json = serde_json::to_string(&stored).expect("session serializes");
        let back: Session = serde_json::from_str(&json).expect("session deserializes");
        assert_eq!(stored, back);
    }

    #[test]
    fn summaries_project_every_held_session() {
        let registry = SessionRegistry::default();
        let id = registry.mint_id();
        registry.insert(session_with(id.clone(), AgentStatus::Running));

        let summaries = registry.summaries();
        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert_eq!(summary.id, id);
        assert_eq!(summary.status, AgentStatus::Running);
        assert_eq!(summary.agent_kind, AgentKind("claude-code".to_string()));
        assert_eq!(summary.title, "Fix the auth bug");
    }

    #[test]
    fn session_summary_round_trips_through_serde() {
        let summary = SessionSummary {
            id: SessionId("7".to_string()),
            title: "Fix the auth bug".to_string(),
            status: AgentStatus::AwaitingInput,
            agent_kind: AgentKind("claude-code".to_string()),
        };
        let json = serde_json::to_string(&summary).expect("serialize");
        let back: SessionSummary = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(summary, back);
    }
}
