//! Living-spec bridge commands and the spec event pipeline (M4 WU-4, D29/D38).
//!
//! The MCP `spectty_spec` effect upserts a serialized [`SpecContract`] to
//! `spectty/{session_id}/spec`; the PR-1 [`SpecBus`](crate::spec_bus) poll loop detects
//! the change and hands a [`Change`](crate::spec_bus::Change) to an injected closure.
//! This module turns that change into a `spec_updated` Tauri event and exposes the
//! on-demand [`get_spec`] command plus the restart-hydrate seam (D38).
//!
//! ## Why the logic is pure
//!
//! Every decision here — deserialize the payload, build the event, read the current
//! contract, reconstruct on re-attach — is a free function over owned types or a
//! `&dyn PersistencePort`. No `AppHandle`, no thread, no clock. The `#[tauri::command]`
//! and the SpecBus emit-closure are thin shells over these functions, exactly mirroring
//! how `commands/session.rs` keeps the lifecycle logic testable against fakes.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use spectty_core::ports::{PersistenceError, PersistencePort};
use spectty_core::SpecContract;
use tauri::State;

use crate::spec_bus::Change;

/// Managed Tauri state: the shared persistence port the spec pipeline reads through. In
/// production this is an `EngramAdapter`; tests drive the pure `*_impl` functions directly.
pub struct SpecPersistence(pub Arc<dyn PersistencePort>);

/// `get_spec(session_id)` → the current [`SpecContract`] for the session, or `None` when
/// none is stored / the stored blob is corrupt. A backend transport failure surfaces as
/// `Err(String)`. Thin shell over [`get_spec_impl`].
#[tauri::command]
pub fn get_spec(
    session_id: String,
    persistence: State<'_, SpecPersistence>,
) -> Result<Option<SpecContract>, String> {
    get_spec_impl(persistence.0.as_ref(), &session_id).map_err(|e| e.to_string())
}

/// The `spec_updated` event payload (D29): the session whose spec changed plus the new
/// [`SpecContract`]. Emitted via the Tauri v2 `Emitter` on an ACTUAL change only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecUpdated {
    /// The session this spec belongs to.
    pub session_id: String,
    /// The full living contract.
    pub spec: SpecContract,
}

/// Extract the `{session_id}` segment from a canonical `spectty/{session_id}/spec` key.
/// Returns `None` for a non-canonical key (the change is then ignored rather than
/// mis-attributed).
#[must_use]
pub fn session_id_from_spec_key(topic_key: &str) -> Option<String> {
    let mut parts = topic_key.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("spectty"), Some(sid), Some("spec"), None) if !sid.is_empty() => {
            Some(sid.to_string())
        }
        _ => None,
    }
}

/// Turn a SpecBus [`Change`] into a [`SpecUpdated`] event, or `None` if the change is not
/// a usable spec update (non-canonical key, or a payload that does not deserialize into a
/// [`SpecContract`]). A malformed payload is DROPPED — never crashes the loop (D29).
#[must_use]
pub fn spec_updated_from_change(change: &Change) -> Option<SpecUpdated> {
    let session_id = session_id_from_spec_key(&change.topic_key)?;
    let spec: SpecContract = serde_json::from_str(&change.content).ok()?;
    Some(SpecUpdated { session_id, spec })
}

/// Read the CURRENT [`SpecContract`] for `session_id` through the persistence port (the
/// `get_spec` command body). Returns:
/// - `Ok(Some(spec))` when a well-formed contract is stored,
/// - `Ok(None)` when the key is absent OR the stored blob does not deserialize (a
///   corrupt blob degrades to "no spec" rather than an error — the UI shows empty),
/// - `Err(_)` only on a backend transport failure.
pub fn get_spec_impl(
    port: &dyn PersistencePort,
    session_id: &str,
) -> Result<Option<SpecContract>, PersistenceError> {
    let key = format!("spectty/{session_id}/spec");
    let Some(content) = port.get(&key)? else {
        return Ok(None);
    };
    Ok(serde_json::from_str::<SpecContract>(&content).ok())
}

/// Restart-hydrate seam (D38): on spawn / re-attach, read the persisted spec ONCE and
/// build the initial [`SpecUpdated`] so the UI restores immediately (exit criterion 6),
/// without waiting for the first poll interval. Engram-down or absent / corrupt → `None`
/// (degrade to last-known / empty, never crash).
#[must_use]
pub fn hydrate_spec(port: &dyn PersistencePort, session_id: &str) -> Option<SpecUpdated> {
    let spec = get_spec_impl(port, session_id).ok().flatten()?;
    Some(SpecUpdated {
        session_id: session_id.to_string(),
        spec,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use spectty_adapters::InMemoryPersistenceAdapter;
    use spectty_core::{ApprovalState, SpecTask, TaskState};

    use super::*;

    fn sample_contract() -> SpecContract {
        SpecContract {
            intent: "fix the auth bug".to_string(),
            proposal: Some("plan".to_string()),
            tasks: vec![SpecTask {
                id: "t1".to_string(),
                title: "write test".to_string(),
                state: TaskState::InProgress,
                notes: None,
            }],
            progress: Vec::new(),
            approval: ApprovalState::Approved,
            steering_notes: Vec::new(),
            dev_override: false,
        }
    }

    fn change(topic_key: &str, content: &str) -> Change {
        Change {
            topic_key: topic_key.to_string(),
            content: content.to_string(),
            updated_at: "2026-06-11 00:00:01".to_string(),
        }
    }

    // WU-4.6: the canonical spec key yields its session id; other shapes do not.
    #[test]
    fn session_id_is_extracted_from_canonical_spec_key_only() {
        assert_eq!(
            session_id_from_spec_key("spectty/42/spec").as_deref(),
            Some("42")
        );
        assert_eq!(session_id_from_spec_key("spectty/42/progress"), None);
        assert_eq!(session_id_from_spec_key("spectty//spec"), None);
        assert_eq!(session_id_from_spec_key("other/42/spec"), None);
    }

    // WU-4.3: a poll Change for a canonical key with a valid contract becomes EXACTLY ONE
    // spec_updated carrying the deserialized SpecContract.
    #[test]
    fn poll_change_becomes_spec_updated_with_deserialized_contract() {
        let contract = sample_contract();
        let payload = serde_json::to_string(&contract).unwrap();
        let event = spec_updated_from_change(&change("spectty/42/spec", &payload))
            .expect("a valid canonical change must produce an event");

        assert_eq!(event.session_id, "42");
        assert_eq!(event.spec, contract);
        assert_eq!(event.spec.tasks[0].state, TaskState::InProgress);
    }

    // WU-4.3 (triangulation): a malformed payload is DROPPED (no event), never a panic.
    #[test]
    fn poll_change_with_malformed_payload_is_dropped() {
        assert!(spec_updated_from_change(&change("spectty/42/spec", "{not a contract")).is_none());
        // A non-canonical key is also dropped even with a valid contract.
        let payload = serde_json::to_string(&sample_contract()).unwrap();
        assert!(spec_updated_from_change(&change("spectty/42/progress", &payload)).is_none());
    }

    // WU-4.6: get_spec reads the stored contract; absent → Ok(None); corrupt → Ok(None).
    #[test]
    fn get_spec_reads_stored_contract_and_degrades_gracefully() {
        let port = InMemoryPersistenceAdapter::new();

        // Absent key.
        assert_eq!(get_spec_impl(&port, "42").unwrap(), None);

        // Stored, well-formed.
        let contract = sample_contract();
        port.upsert("spectty/42/spec", serde_json::to_string(&contract).unwrap())
            .unwrap();
        assert_eq!(get_spec_impl(&port, "42").unwrap(), Some(contract));

        // Corrupt blob degrades to None (UI shows empty, not an error).
        port.upsert("spectty/99/spec", "garbage".to_string())
            .unwrap();
        assert_eq!(get_spec_impl(&port, "99").unwrap(), None);
    }

    // WU-4.4: restart hydrate reconstructs the initial spec_updated from the persisted
    // key; absent → None (degrade to empty, no crash).
    #[test]
    fn restart_hydrate_emits_initial_spec_updated() {
        let port: Arc<dyn PersistencePort> = Arc::new(InMemoryPersistenceAdapter::new());
        let contract = sample_contract();
        port.upsert("spectty/42/spec", serde_json::to_string(&contract).unwrap())
            .unwrap();

        let event = hydrate_spec(port.as_ref(), "42").expect("re-attach must hydrate the spec");
        assert_eq!(event.session_id, "42");
        assert_eq!(event.spec, contract);

        // A session with no persisted spec hydrates to nothing (degrade to empty).
        assert!(hydrate_spec(port.as_ref(), "no-such").is_none());
    }
}
