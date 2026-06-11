//! The living `SpecContract` — the agent's plan, progress, and approval state as a
//! PURE Core aggregate (D32/D33, ADR-0007).
//!
//! M4 introduces the "Living Spec Pane": a cooperative agent pushes its plan and
//! progress through the `spectty_spec` MCP effect, the adapter serializes a
//! `SpecContract` into engram, and the poll loop surfaces it to the UI. Everything in
//! this module is `serde + thiserror` ONLY — no I/O, no time, no agent name. The
//! serialization to engram is the ADAPTER's job; this file owns the SHAPE and the
//! legal-transition / approval-gate RULES.
//!
//! ## The two business invariants (the testable surface)
//!
//! 1. [`TaskState::transition`] is a ONE-WAY state machine — `pending → {in_progress,
//!    skipped}`, `in_progress → {done, skipped}`, `done` terminal. It mirrors
//!    [`agent_status::transition`](crate::entities::agent_status::transition): a single
//!    pure authority so no caller can illegally jump a task forward or backward.
//! 2. The plan-approval gate ([`SpecContract::may_begin_edits`] /
//!    [`SpecContract::apply_progress`]) is a Core rule, not an adapter convention: a
//!    task may move to `InProgress` ONLY once the plan is `Approved`. Adapters READ this
//!    rule (ADR-0007); they never re-implement it.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Lifecycle state of a single planned task (D32).
///
/// The legal transitions are owned by the pure [`TaskState::transition`] method — the
/// SINGLE authority — so no caller can illegally jump a task. Mirrors the
/// [`AgentStatus`](crate::AgentStatus) discipline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Not yet started.
    Pending,
    /// Actively being worked.
    InProgress,
    /// Finished cleanly (TERMINAL).
    Done,
    /// Intentionally not done (TERMINAL).
    Skipped,
}

impl TaskState {
    /// PURE one-way transition. Returns the next state for a legal move, or `self`
    /// UNCHANGED for an illegal one — an illegal move is IGNORED, never an error.
    ///
    /// Legal edges (and ONLY these):
    /// - `Pending → InProgress`
    /// - `Pending → Skipped`
    /// - `InProgress → Done`
    /// - `InProgress → Skipped`
    ///
    /// `Done` and `Skipped` are TERMINAL: any move out of them is ignored. Backward moves
    /// (`InProgress → Pending`, `Done → InProgress`) and illegal jumps (`Pending → Done`)
    /// leave the state unchanged. The function is total, deterministic, and side-effect
    /// free.
    ///
    /// # Why infallible (Finding 2, PR-2 review)
    ///
    /// The binding spec scenarios mandate that an illegal backward transition is "ignored,
    /// not an error — the task MUST remain done", explicitly mirroring the infallible
    /// [`agent_status::transition`](crate::entities::agent_status::transition). Progress
    /// updates arrive from an EXTERNAL agent, so an illegal move must degrade gracefully
    /// (stay put) rather than error. The spec is authoritative over the earlier D32/D33
    /// `Result` sketch; see the design amendment under D32/D33.
    #[must_use]
    pub fn transition(self, to: TaskState) -> TaskState {
        use TaskState::{Done, InProgress, Pending, Skipped};
        match (self, to) {
            (Pending, InProgress | Skipped) => to,
            (InProgress, Done | Skipped) => to,
            // Everything else — terminal source, backward move, or illegal jump: IGNORED.
            _ => self,
        }
    }
}

/// The plan-approval lifecycle (D32/D33).
///
/// A freshly submitted plan starts [`ApprovalState::Pending`]; the user resolves it to
/// one of the other variants. `Approved` is the ONLY state that satisfies the
/// edit gate ([`SpecContract::may_begin_edits`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalState {
    /// Awaiting the user's decision (the default for a submitted plan).
    Pending,
    /// The user approved the plan — the gate opens.
    Approved,
    /// The user rejected the plan outright.
    Rejected,
    /// The user wants changes (steering notes) before approving.
    Adjusted,
}

impl Default for ApprovalState {
    /// A submitted plan starts `Pending` (D33).
    fn default() -> Self {
        Self::Pending
    }
}

/// A single planned task in a [`SpecContract`] (D32).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecTask {
    /// Stable identifier the agent assigns; the gate addresses tasks by this id.
    pub id: String,
    /// Human-readable task title.
    pub title: String,
    /// Current lifecycle state. The FROZEN MCP schema and the spec deltas mandate the wire
    /// field name `status`; Core keeps the idiomatic field name `state` and renames on the
    /// wire so schema-faithful agent payloads deserialize and serialization stays
    /// schema-authoritative (Finding 1, PR-2 review).
    #[serde(rename = "status")]
    pub state: TaskState,
    /// Optional free-form notes the agent attaches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// A progress entry: which task advanced and to what state (D32). Kept distinct from the
/// task list so an agent can stream incremental progress without resending every task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskProgress {
    /// The task this progress refers to.
    pub task_id: String,
    /// The state the task reached.
    pub state: TaskState,
}

/// The living plan-and-progress aggregate (D32).
///
/// PURE: `serde + thiserror` only, no I/O / time / agent name. The adapter serializes
/// this to `spectty/{session_id}/spec`; the UI renders it. The approval gate
/// ([`may_begin_edits`](Self::may_begin_edits) / [`apply_progress`](Self::apply_progress))
/// is the Core business rule (ADR-0007) adapters read but never re-implement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecContract {
    /// The one-line intent / goal of the plan. The FROZEN MCP schema does NOT list `intent`
    /// as required on its `spec` object, so a schema-valid payload may omit it; Core
    /// defaults it to the empty string rather than failing to deserialize (Finding 1).
    #[serde(default)]
    pub intent: String,
    /// The full proposal prose, if the agent supplied one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<String>,
    /// The planned tasks.
    pub tasks: Vec<SpecTask>,
    /// Incremental progress entries.
    #[serde(default)]
    pub progress: Vec<TaskProgress>,
    /// The plan-approval state. Defaults to [`ApprovalState::Pending`].
    #[serde(default)]
    pub approval: ApprovalState,
    /// Free-form steering notes the user attaches when adjusting the plan.
    #[serde(default)]
    pub steering_notes: Vec<String>,
    /// DEV-ONLY escape hatch: when `true`, [`may_begin_edits`](Self::may_begin_edits)
    /// returns `true` regardless of `approval`, so a developer can bypass the gate while
    /// iterating. NEVER set by a real approval flow (those go through `approval`), and it
    /// is distinguishable from a genuine `Approved` — the gate checks `approval` FIRST.
    #[serde(default)]
    pub dev_override: bool,
}

impl SpecContract {
    /// The plan-approval gate (D33). Edits to the workspace may begin ONLY when the plan
    /// is [`ApprovalState::Approved`] — OR when the dev-override flag is set (iteration
    /// escape hatch, never a real approval).
    #[must_use]
    pub fn may_begin_edits(&self) -> bool {
        matches!(self.approval, ApprovalState::Approved) || self.dev_override
    }

    /// Advance a task to `to`, applying the one-way [`TaskState::transition`] rule AND the
    /// plan-approval gate (D33): moving a task to [`TaskState::InProgress`] while the plan
    /// is not yet approved is rejected with [`SpecError::GateNotApproved`].
    ///
    /// The gate violation IS an error; an illegal task-state move is NOT (Finding 2): it is
    /// ignored, leaving the task unchanged and recording no progress. A genuine advance
    /// updates the task in place and appends a [`TaskProgress`] entry. An unknown `task_id`
    /// is [`SpecError::UnknownTask`].
    pub fn apply_progress(&mut self, task_id: &str, to: TaskState) -> Result<(), SpecError> {
        // Gate FIRST: beginning work (→ InProgress) requires an approved plan. The gate is
        // a separate, hard rule (D33) — its violation stays an error, distinct from the
        // infallible task-state machine below.
        if to == TaskState::InProgress && !self.may_begin_edits() {
            return Err(SpecError::GateNotApproved);
        }
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| SpecError::UnknownTask(task_id.to_string()))?;
        let next = task.state.transition(to);
        // An illegal move is ignored (next == current): no state change, no progress entry.
        if next == task.state {
            return Ok(());
        }
        task.state = next;
        self.progress.push(TaskProgress {
            task_id: task_id.to_string(),
            state: next,
        });
        Ok(())
    }
}

/// Errors from the pure spec rules (D32/D33). `thiserror` only — no I/O concerns.
///
/// Note (Finding 2): an illegal [`TaskState::transition`] is NOT an error — it is ignored
/// (the task keeps its state), mirroring the infallible `AgentStatus::transition`. So this
/// enum carries ONLY the plan-approval gate violation and the unknown-task lookup error,
/// both of which are genuine caller errors distinct from a no-op state move.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SpecError {
    /// A task was moved to `InProgress` while the plan-approval gate was not satisfied.
    #[error("plan not approved: edits may not begin until the plan is Approved")]
    GateNotApproved,
    /// `apply_progress` referenced a task id that is not in the contract.
    #[error("unknown task id: {0}")]
    UnknownTask(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract_with(approval: ApprovalState, tasks: Vec<SpecTask>) -> SpecContract {
        SpecContract {
            intent: "fix the bug".to_string(),
            proposal: Some("a detailed plan".to_string()),
            tasks,
            progress: Vec::new(),
            approval,
            steering_notes: Vec::new(),
            dev_override: false,
        }
    }

    fn task(id: &str, state: TaskState) -> SpecTask {
        SpecTask {
            id: id.to_string(),
            title: format!("task {id}"),
            state,
            notes: None,
        }
    }

    // WU-3.1: the one-directional legal-transition table. Finding 2 (PR-2 review): the
    // binding spec scenarios mandate that an illegal transition is IGNORED, not an error —
    // the task keeps its current state, mirroring the infallible `AgentStatus::transition`.
    #[test]
    fn task_state_transition_legal_table() {
        use TaskState::{Done, InProgress, Pending, Skipped};

        // Legal forward edges advance to the target.
        assert_eq!(Pending.transition(InProgress), InProgress);
        assert_eq!(Pending.transition(Skipped), Skipped);
        assert_eq!(InProgress.transition(Done), Done);
        assert_eq!(InProgress.transition(Skipped), Skipped);

        // Illegal jump: Pending -> Done skips InProgress → IGNORED, stays Pending.
        assert_eq!(Pending.transition(Done), Pending);

        // Backward moves are ignored: the task keeps its current state (spec: "the task
        // MUST remain done", mirroring AgentStatus).
        assert_eq!(InProgress.transition(Pending), InProgress);
        assert_eq!(Done.transition(InProgress), Done);

        // Done and Skipped are TERMINAL: any move out is ignored (stays put).
        for to in [Pending, InProgress, Done, Skipped] {
            if to != Done {
                assert_eq!(
                    Done.transition(to),
                    Done,
                    "Done is terminal; Done -> {to:?} must be ignored (stays Done)"
                );
            }
            if to != Skipped {
                assert_eq!(
                    Skipped.transition(to),
                    Skipped,
                    "Skipped is terminal; Skipped -> {to:?} must be ignored (stays Skipped)"
                );
            }
        }
    }

    // WU-3.2: ApprovalState default + all variants + serde round-trip.
    #[test]
    fn approval_state_default_is_pending() {
        assert_eq!(ApprovalState::default(), ApprovalState::Pending);

        for state in [
            ApprovalState::Pending,
            ApprovalState::Approved,
            ApprovalState::Rejected,
            ApprovalState::Adjusted,
        ] {
            let json = serde_json::to_string(&state).expect("serialize");
            let back: ApprovalState = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(state, back, "ApprovalState::{state:?} must round-trip");
        }
    }

    // WU-3.3: SpecContract serde round-trips byte-stable.
    #[test]
    fn spec_contract_serde_round_trips() {
        let mut contract = contract_with(
            ApprovalState::Approved,
            vec![
                task("t1", TaskState::Done),
                task("t2", TaskState::InProgress),
            ],
        );
        contract.progress.push(TaskProgress {
            task_id: "t1".to_string(),
            state: TaskState::Done,
        });
        contract.steering_notes.push("prefer small PRs".to_string());

        let json = serde_json::to_string(&contract).expect("serialize");
        let back: SpecContract = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(contract, back, "SpecContract must round-trip");

        // Re-serializing the round-tripped value yields byte-identical JSON.
        let json2 = serde_json::to_string(&back).expect("re-serialize");
        assert_eq!(json, json2, "serialization must be byte-stable");
    }

    // WU-3.4: may_begin_edits is true ONLY when Approved (or dev_override).
    #[test]
    fn may_begin_edits_true_only_when_approved() {
        assert!(contract_with(ApprovalState::Approved, vec![]).may_begin_edits());

        for blocked in [
            ApprovalState::Pending,
            ApprovalState::Rejected,
            ApprovalState::Adjusted,
        ] {
            assert!(
                !contract_with(blocked, vec![]).may_begin_edits(),
                "approval {blocked:?} must NOT open the gate"
            );
        }

        // The dev-override escape hatch opens the gate but is distinguishable from a real
        // approval (approval stays Pending).
        let mut overridden = contract_with(ApprovalState::Pending, vec![]);
        overridden.dev_override = true;
        assert!(overridden.may_begin_edits());
        assert_eq!(
            overridden.approval,
            ApprovalState::Pending,
            "dev_override must NOT masquerade as a real approval"
        );
    }

    // WU-3.5: the apply_progress gate blocks InProgress while Pending, allows it once
    // Approved.
    #[test]
    fn apply_progress_blocks_in_progress_while_pending() {
        // Pending plan: moving t1 to InProgress is gated.
        let mut pending =
            contract_with(ApprovalState::Pending, vec![task("t1", TaskState::Pending)]);
        assert_eq!(
            pending.apply_progress("t1", TaskState::InProgress),
            Err(SpecError::GateNotApproved)
        );
        // The task did NOT advance and no progress was recorded.
        assert_eq!(pending.tasks[0].state, TaskState::Pending);
        assert!(pending.progress.is_empty());

        // Approved plan: the SAME call now succeeds and records progress.
        let mut approved = contract_with(
            ApprovalState::Approved,
            vec![task("t1", TaskState::Pending)],
        );
        assert_eq!(approved.apply_progress("t1", TaskState::InProgress), Ok(()));
        assert_eq!(approved.tasks[0].state, TaskState::InProgress);
        assert_eq!(approved.progress.len(), 1);
        assert_eq!(approved.progress[0].task_id, "t1");
        assert_eq!(approved.progress[0].state, TaskState::InProgress);
    }

    // Finding 1 (BLOCKER): the FROZEN MCP schema and the spec deltas mandate the wire
    // field name `status` for a task's lifecycle state. A schema-faithful payload MUST
    // deserialize into a `SpecContract`; before the `#[serde(rename = "status")]` fix it
    // silently failed (None → spec dropped). Also: the schema's `spec` object does NOT
    // require `intent`, so an intent-less but schema-valid payload MUST still deserialize
    // (Core's `intent` is `#[serde(default)]`).
    #[test]
    fn schema_faithful_payload_deserializes_with_status_field() {
        // Exactly the shape a compliant agent emits per the frozen schema.
        let payload = r#"{"tasks":[{"id":"t1","title":"t","status":"pending"}]}"#;
        let contract: SpecContract =
            serde_json::from_str(payload).expect("a schema-faithful payload MUST deserialize");

        assert_eq!(contract.tasks.len(), 1);
        assert_eq!(contract.tasks[0].id, "t1");
        assert_eq!(
            contract.tasks[0].state,
            TaskState::Pending,
            "the wire field `status` MUST map onto the Core `state` field"
        );
        // The schema does not require `intent`; an absent one defaults to empty.
        assert_eq!(
            contract.intent, "",
            "an intent-less but schema-valid payload MUST deserialize (intent defaults to \"\")"
        );
    }

    // Triangulation: each enum variant must round-trip through the `status` wire name.
    #[test]
    fn task_status_wire_name_round_trips_all_variants() {
        for (variant, wire) in [
            (TaskState::Pending, "pending"),
            (TaskState::InProgress, "in_progress"),
            (TaskState::Done, "done"),
            (TaskState::Skipped, "skipped"),
        ] {
            let payload = format!(r#"{{"tasks":[{{"id":"t","title":"t","status":"{wire}"}}]}}"#);
            let contract: SpecContract =
                serde_json::from_str(&payload).expect("schema-faithful payload");
            assert_eq!(contract.tasks[0].state, variant);

            // Serializing back MUST emit `status`, not `state` (schema stays authoritative).
            let json = serde_json::to_string(&contract.tasks[0]).expect("serialize task");
            assert!(
                json.contains(&format!(r#""status":"{wire}""#)),
                "SpecTask MUST serialize the lifecycle field as `status`, got: {json}"
            );
            assert!(
                !json.contains(r#""state""#),
                "SpecTask MUST NOT serialize a `state` field (schema mandates `status`): {json}"
            );
        }
    }

    // Triangulation for apply_progress: an unknown task id is a distinct error; an illegal
    // task-state move is IGNORED (Finding 2) — the task keeps its state and NO progress is
    // recorded (a no-op move is not progress). Gate violations remain errors (tested
    // above); task-state illegality is not an error.
    #[test]
    fn apply_progress_unknown_task_and_illegal_transition() {
        let mut approved =
            contract_with(ApprovalState::Approved, vec![task("t1", TaskState::Done)]);

        // Unknown task id is still a distinct error (the contract knows nothing about it).
        assert_eq!(
            approved.apply_progress("nope", TaskState::Skipped),
            Err(SpecError::UnknownTask("nope".to_string()))
        );

        // t1 is Done (terminal): moving it to Skipped is an illegal move that is IGNORED,
        // not an error. The task stays Done and no progress is recorded.
        assert_eq!(approved.apply_progress("t1", TaskState::Skipped), Ok(()));
        assert_eq!(
            approved.tasks[0].state,
            TaskState::Done,
            "an illegal move must leave the task unchanged (stays Done)"
        );
        assert!(
            approved.progress.is_empty(),
            "a no-op illegal move records no progress"
        );
    }
}
