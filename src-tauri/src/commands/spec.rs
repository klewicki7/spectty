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
use spectty_core::entities::diff::DiffExplanation;
use spectty_core::ports::{PersistenceError, PersistencePort};
use spectty_core::{ApprovalState, SpecContract};
use tauri::State;

use crate::diff_pipeline::DiffPipelines;
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

/// `get_diff_explanation(session_id)` → the current [`DiffExplanation`] for the session, or
/// `None` when the session has no diff pipeline or it has not produced an explanation yet
/// (M4-REQ-16, D29). The VibeLens panel uses the on-demand read on (re-)attach and as the
/// manual-refresh fallback; the live updates arrive via `diff_updated`. Reads from the
/// in-process pipeline cache — never blocks on git or VibeLens.
#[tauri::command]
pub fn get_diff_explanation(
    session_id: String,
    pipelines: State<'_, DiffPipelines>,
) -> Result<Option<DiffExplanation>, String> {
    Ok(pipelines.current_explanation(&session_id))
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

// ── M4 WU-5: the plan-approval resolver (D31/D33) ──────────────────────────────────────
//
// `spectty_approval` (MCP side) upserts a pending request to `spectty/{session_id}/approval`
// and long-polls the same key for a `resolution`. The `approve_prompt` command (here) writes
// the user's decision THROUGH the Core [`ApprovalState`] gate and upserts the resolved
// document so the blocked MCP caller unblocks. The document shape is byte-shared with the MCP
// resolver (which cannot import Core — serde+http only, D16), so this struct and the MCP-side
// struct MUST stay wire-compatible; the canonical-key helper and the `decision` enum encoding
// are the contract surface.

/// One pending-or-resolved approval request, persisted at `spectty/{session_id}/approval`
/// (D31). `resolution == None` means PENDING (the agent's `spectty_approval` is blocked
/// long-polling); a `Some` decision is the resolution the UI wrote via [`approve_prompt`].
///
/// The `decision` is encoded as a Core [`ApprovalState`] so the gate rule is never
/// re-implemented (ADR-0007): the only decisions a real approval can carry are the
/// non-`Pending` variants, but `ApprovalState`'s own serde keeps the wire string canonical
/// and shared with the MCP resolver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// The action this approval gates, unique within the session — the `(session_id,
    /// action_id)` pair is the resolver key (D31).
    pub action_id: String,
    /// Human-readable description of what is being approved (from the `spectty_approval`
    /// arguments).
    #[serde(default)]
    pub description: String,
    /// The agent-declared risk level (`low|medium|high`); free-form, surfaced to the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    /// The choices the UI offers; the status path derives `quick_actions` from these.
    #[serde(default)]
    pub options: Vec<String>,
    /// The user's decision once resolved, or `None` while still pending. A resolved request
    /// is removed from "pending" purely by this field becoming `Some`.
    #[serde(default)]
    pub resolution: Option<ApprovalState>,
}

impl ApprovalRequest {
    /// `true` while the request is still awaiting the user (the agent is blocked).
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.resolution.is_none()
    }
}

/// The canonical engram key for a session's approval document (D31/D5).
#[must_use]
pub fn approval_key(session_id: &str) -> String {
    format!("spectty/{session_id}/approval")
}

/// Read the current [`ApprovalRequest`] for `session_id`, or `None` when none is stored or
/// the stored blob does not deserialize (a corrupt blob degrades to "no request"). `Err`
/// only on a backend transport failure.
pub fn get_approval_impl(
    port: &dyn PersistencePort,
    session_id: &str,
) -> Result<Option<ApprovalRequest>, PersistenceError> {
    let Some(content) = port.get(&approval_key(session_id))? else {
        return Ok(None);
    };
    Ok(serde_json::from_str::<ApprovalRequest>(&content).ok())
}

/// Resolve the pending approval for `(session_id, action_id)` with `decision` (the
/// `approve_prompt` command body, D31/D33). The decision flows THROUGH the Core
/// [`ApprovalState`] — the gate rule is never re-implemented here (ADR-0007).
///
/// Semantics (spec `spectty-approval.md` M4-REQ-11):
/// - matching pending request → write `resolution = Some(decision)` back to the same key so
///   the blocked MCP long-poll observes it, return `Ok(true)`;
/// - unknown `(session_id, action_id)` (no request, or a different `action_id`, or already
///   resolved) → NO-OP, no write, no error, return `Ok(false)`;
/// - a backend transport failure → `Err` (the command surfaces it as a `String`).
pub fn resolve_approval_impl(
    port: &dyn PersistencePort,
    session_id: &str,
    action_id: &str,
    decision: ApprovalState,
) -> Result<bool, PersistenceError> {
    let Some(mut request) = get_approval_impl(port, session_id)? else {
        // No pending request for this session at all — no-op.
        return Ok(false);
    };
    // Only the addressed, still-pending request resolves; anything else is a no-op so a
    // stale/duplicate `approve_prompt` cannot clobber a different action or re-resolve.
    if request.action_id != action_id || !request.is_pending() {
        return Ok(false);
    }
    request.resolution = Some(decision);
    let payload =
        serde_json::to_string(&request).map_err(|e| PersistenceError::Backend(e.to_string()))?;
    port.upsert(&approval_key(session_id), payload)?;
    Ok(true)
}

/// The decision the UI sends from the plan-approval gate. A thin wire enum mapped onto the
/// Core [`ApprovalState`] so the command boundary speaks the UI's vocabulary while the stored
/// resolution stays the canonical Core state (ADR-0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// The user approved the plan — opens the edit gate.
    Approve,
    /// The user rejected the plan outright.
    Reject,
    /// The user wants changes (steering) before approving.
    Adjust,
}

impl From<ApprovalDecision> for ApprovalState {
    fn from(decision: ApprovalDecision) -> Self {
        match decision {
            ApprovalDecision::Approve => ApprovalState::Approved,
            ApprovalDecision::Reject => ApprovalState::Rejected,
            ApprovalDecision::Adjust => ApprovalState::Adjusted,
        }
    }
}

/// `approve_prompt(session_id, action_id, decision)` (D31): resolve the pending approval and
/// unblock the agent's `spectty_approval` long-poll. An unknown key is a benign no-op
/// (returns `false`); a backend failure surfaces as `Err(String)`. Thin shell over
/// [`resolve_approval_impl`].
#[tauri::command]
pub fn approve_prompt(
    session_id: String,
    action_id: String,
    decision: ApprovalDecision,
    persistence: State<'_, SpecPersistence>,
) -> Result<bool, String> {
    resolve_approval_impl(
        persistence.0.as_ref(),
        &session_id,
        &action_id,
        decision.into(),
    )
    .map_err(|e| e.to_string())
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

    // Finding 1 (BLOCKER) wire-path: the EXACT seam the review proved broken. A
    // schema-faithful agent payload (`status`, no `intent`) is serialized by the MCP
    // effect (`spec.to_string()` on the parsed agent `Value`), stored as the engram
    // content string, and MUST deserialize back into a `SpecContract` on the app side —
    // both through the poll-change seam (`spec_updated_from_change`) and the on-demand
    // read (`get_spec_impl`). Before the `#[serde(rename = "status")]` + `intent` default
    // fix, this deserialize returned None and the spec was silently dropped.
    #[test]
    fn schema_faithful_agent_payload_survives_the_mcp_to_core_wire_path() {
        // What a compliant agent sends as the `spec` argument of a `spectty_spec` call.
        let agent_spec = serde_json::json!({
            "tasks": [
                { "id": "t1", "title": "write the test", "status": "pending" },
                { "id": "t2", "title": "make it green", "status": "in_progress" }
            ]
        });
        // The MCP effect upserts `spec.to_string()` verbatim (main.rs:227) — reproduce it.
        let wire_content = agent_spec.to_string();

        // Poll-change seam: a canonical change carrying that wire content MUST emit.
        let event = spec_updated_from_change(&change("spectty/42/spec", &wire_content))
            .expect("schema-faithful agent payload MUST survive the MCP→Core wire path");
        assert_eq!(event.session_id, "42");
        assert_eq!(event.spec.tasks.len(), 2);
        assert_eq!(event.spec.tasks[0].state, TaskState::Pending);
        assert_eq!(event.spec.tasks[1].state, TaskState::InProgress);
        assert_eq!(event.spec.intent, "", "absent intent defaults to empty");

        // On-demand read seam: the same stored wire content reads back as a contract.
        let port = InMemoryPersistenceAdapter::new();
        port.upsert("spectty/42/spec", wire_content).unwrap();
        let read = get_spec_impl(&port, "42")
            .unwrap()
            .expect("a stored schema-faithful payload MUST read back as a contract");
        assert_eq!(read.tasks[1].state, TaskState::InProgress);
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

    // ── M4 WU-5: the plan-approval resolver (D31/D33) ─────────────────────────────────

    fn pending_request(action_id: &str) -> ApprovalRequest {
        ApprovalRequest {
            action_id: action_id.to_string(),
            description: "delete the prod database".to_string(),
            risk_level: Some("high".to_string()),
            options: vec!["approve".to_string(), "reject".to_string()],
            resolution: None,
        }
    }

    fn store_pending(port: &InMemoryPersistenceAdapter, session_id: &str, req: &ApprovalRequest) {
        port.upsert(
            &approval_key(session_id),
            serde_json::to_string(req).unwrap(),
        )
        .unwrap();
    }

    // WU-5.3: approve_prompt resolves a pending request and the resolution becomes observable
    // to the (blocked) caller via the same key; the request is no longer pending.
    #[test]
    fn approve_prompt_resolves_and_unblocks_caller() {
        let port = InMemoryPersistenceAdapter::new();
        store_pending(&port, "42", &pending_request("edit-1"));

        let resolved =
            resolve_approval_impl(&port, "42", "edit-1", ApprovalState::Approved).unwrap();
        assert!(resolved, "the matching pending request must resolve");

        // The blocked caller observes the resolution on the same key.
        let after = get_approval_impl(&port, "42")
            .unwrap()
            .expect("the document is still present, now resolved");
        assert_eq!(after.resolution, Some(ApprovalState::Approved));
        assert!(
            !after.is_pending(),
            "a resolved request is no longer pending"
        );
    }

    // WU-5.4: approve_prompt for an unknown key (no request at all, or a different action_id)
    // is a no-op — no write, no error, returns false.
    #[test]
    fn approve_prompt_unknown_key_is_no_op() {
        let port = InMemoryPersistenceAdapter::new();

        // No request stored at all.
        assert!(!resolve_approval_impl(&port, "42", "ghost", ApprovalState::Approved).unwrap());
        assert!(get_approval_impl(&port, "42").unwrap().is_none());

        // A pending request exists, but for a DIFFERENT action_id → still a no-op, and the
        // existing pending request is untouched.
        store_pending(&port, "42", &pending_request("edit-1"));
        assert!(!resolve_approval_impl(&port, "42", "ghost", ApprovalState::Approved).unwrap());
        let still_pending = get_approval_impl(&port, "42").unwrap().unwrap();
        assert!(
            still_pending.is_pending(),
            "resolving an unknown action_id must NOT touch the real pending request"
        );
    }

    // WU-5.4 (triangulation): re-resolving an already-resolved request is a no-op (the
    // decision is not clobbered by a stale/duplicate approve_prompt).
    #[test]
    fn approve_prompt_already_resolved_is_no_op() {
        let port = InMemoryPersistenceAdapter::new();
        store_pending(&port, "42", &pending_request("edit-1"));
        assert!(resolve_approval_impl(&port, "42", "edit-1", ApprovalState::Approved).unwrap());

        // A second decision must not overwrite the first (it is already resolved).
        assert!(!resolve_approval_impl(&port, "42", "edit-1", ApprovalState::Rejected).unwrap());
        let after = get_approval_impl(&port, "42").unwrap().unwrap();
        assert_eq!(
            after.resolution,
            Some(ApprovalState::Approved),
            "an already-resolved decision must be stable"
        );
    }

    // WU-5.5: the decision flows THROUGH the Core ApprovalState (ADR-0007) — the UI's
    // ApprovalDecision maps onto the canonical Core state that is persisted, never a
    // re-implemented string.
    #[test]
    fn approve_prompt_writes_approval_state_via_core_gate() {
        for (decision, expected) in [
            (ApprovalDecision::Approve, ApprovalState::Approved),
            (ApprovalDecision::Reject, ApprovalState::Rejected),
            (ApprovalDecision::Adjust, ApprovalState::Adjusted),
        ] {
            let port = InMemoryPersistenceAdapter::new();
            store_pending(&port, "42", &pending_request("edit-1"));

            resolve_approval_impl(&port, "42", "edit-1", decision.into()).unwrap();
            let after = get_approval_impl(&port, "42").unwrap().unwrap();
            assert_eq!(
                after.resolution,
                Some(expected),
                "{decision:?} must persist as the Core {expected:?} state"
            );
        }

        // The gate predicate honours the resolved state: an Approved plan opens edits, a
        // Rejected one does not — proving the resolution feeds the SAME Core rule the agent
        // is gated on (no parallel approval vocabulary).
        let approved = SpecContract {
            intent: String::new(),
            proposal: None,
            tasks: Vec::new(),
            progress: Vec::new(),
            approval: ApprovalState::from(ApprovalDecision::Approve),
            steering_notes: Vec::new(),
            dev_override: false,
        };
        assert!(approved.may_begin_edits());
        let rejected = SpecContract {
            approval: ApprovalState::from(ApprovalDecision::Reject),
            ..approved.clone()
        };
        assert!(!rejected.may_begin_edits());
    }

    // The persisted approval document round-trips and the canonical key is per-session.
    #[test]
    fn approval_request_round_trips_and_key_is_per_session() {
        let req = pending_request("edit-1");
        let json = serde_json::to_string(&req).unwrap();
        let back: ApprovalRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);

        assert_eq!(approval_key("42"), "spectty/42/approval");
        assert_eq!(approval_key("abc-7"), "spectty/abc-7/approval");
    }

    // ── PR-3 Finding 1 (MAJOR): the CROSS-SEAM wire contract ───────────────────────────
    //
    // The PR-2 blocker class was a two-sided JSON contract where each side only tested its
    // OWN serialization. Here the Tauri side owns BOTH shapes via JSON literals: we replicate
    // the EXACT pending document the MCP `spectty_approval_effect` writes (main.rs:291-297),
    // deserialize it into `ApprovalRequest`, resolve it through `resolve_approval_impl`, and
    // assert the serialized resolved document satisfies the MCP-side parser's contract
    // (`resolution_of`: a non-null `"resolution"` string matching the `action_id`). This is
    // the seam that actually breaks in production if either side drifts.

    /// Replicate, verbatim, the pending document the MCP effect upserts (crates/spectty-mcp/
    /// src/main.rs:291-297): `action_id`, a `description`, a NULLABLE `risk_level`, the
    /// `options[]`, and `resolution: null`. Copied as a JSON literal (not the Rust struct) so
    /// the test fails if the MCP wire shape and `ApprovalRequest` ever diverge.
    fn mcp_pending_doc_literal(action_id: &str) -> String {
        serde_json::json!({
            "action_id": action_id,
            "description": "delete the prod database",
            "risk_level": "high",
            "options": ["approve", "reject", "adjust"],
            "resolution": serde_json::Value::Null,
        })
        .to_string()
    }

    /// Mirror of the MCP-side `resolution_of` contract (crates/spectty-mcp/src/main.rs:381):
    /// the resolved document MUST carry a non-null `"resolution"` string for the addressed
    /// `action_id`, or the blocked long-poll never unblocks. We replicate it here because the
    /// MCP crate cannot be a dev-dependency of src-tauri; the mirror MCP-side test
    /// (`tauri_resolved_doc_unblocks_the_mcp_long_poll`) feeds a Tauri-serialized literal into
    /// the REAL `resolution_of` to close the loop.
    fn mcp_resolution_of(content: &str, action_id: &str) -> Option<String> {
        let doc: serde_json::Value = serde_json::from_str(content).ok()?;
        if doc.get("action_id").and_then(serde_json::Value::as_str) != Some(action_id) {
            return None;
        }
        let resolution = doc.get("resolution")?;
        if resolution.is_null() {
            return None;
        }
        resolution.as_str().map(str::to_string)
    }

    // Finding 1 (MAJOR): the MCP-written pending doc, resolved on the Tauri side, MUST produce
    // a document the MCP `resolution_of` parser accepts — proving the two-sided contract holds
    // end to end (Approved direction).
    #[test]
    fn mcp_pending_doc_resolved_by_tauri_satisfies_the_mcp_parser() {
        let port = InMemoryPersistenceAdapter::new();

        // Store the EXACT pending document the MCP effect wrote (not a Rust-built struct).
        port.upsert(&approval_key("42"), mcp_pending_doc_literal("edit-1"))
            .unwrap();

        // The MCP-written literal MUST deserialize into the Tauri `ApprovalRequest`.
        let parsed = get_approval_impl(&port, "42")
            .unwrap()
            .expect("the MCP pending wire doc MUST deserialize into ApprovalRequest");
        assert!(
            parsed.is_pending(),
            "the MCP doc is pending (resolution:null)"
        );
        assert_eq!(parsed.action_id, "edit-1");
        assert_eq!(parsed.risk_level.as_deref(), Some("high"));
        assert_eq!(parsed.options.len(), 3);

        // Resolve via the real command path (Approve).
        let resolved =
            resolve_approval_impl(&port, "42", "edit-1", ApprovalDecision::Approve.into()).unwrap();
        assert!(resolved, "the matching MCP request must resolve");

        // Serialize the resolved request EXACTLY as the command does, then assert the MCP
        // parser accepts it — contains `"resolution":"Approved"` for the matching action_id.
        let resolved_wire =
            serde_json::to_string(&get_approval_impl(&port, "42").unwrap().unwrap()).unwrap();
        assert!(
            resolved_wire.contains(r#""resolution":"Approved""#),
            "the resolved wire doc MUST carry the canonical Core decision: {resolved_wire}"
        );
        assert_eq!(
            mcp_resolution_of(&resolved_wire, "edit-1").as_deref(),
            Some("Approved"),
            "the MCP-side parser MUST extract the decision the Tauri side wrote"
        );
        // A resolution under the wrong action_id is invisible to the long-poll (no false unblock).
        assert_eq!(mcp_resolution_of(&resolved_wire, "other"), None);
    }

    // Finding 1 (MAJOR), Adjusted + steering direction: the Adjust decision must also survive
    // the cross-seam round-trip and remain readable by the MCP parser as "Adjusted".
    #[test]
    fn mcp_pending_doc_resolved_adjusted_survives_the_seam() {
        let port = InMemoryPersistenceAdapter::new();
        port.upsert(&approval_key("7"), mcp_pending_doc_literal("plan-1"))
            .unwrap();

        let resolved =
            resolve_approval_impl(&port, "7", "plan-1", ApprovalDecision::Adjust.into()).unwrap();
        assert!(resolved);

        let resolved_wire =
            serde_json::to_string(&get_approval_impl(&port, "7").unwrap().unwrap()).unwrap();
        assert!(
            resolved_wire.contains(r#""resolution":"Adjusted""#),
            "Adjust must persist as the canonical Core Adjusted state: {resolved_wire}"
        );
        assert_eq!(
            mcp_resolution_of(&resolved_wire, "plan-1").as_deref(),
            Some("Adjusted"),
            "the MCP parser MUST read back the Adjusted decision"
        );
    }
}
