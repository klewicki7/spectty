use serde::{Deserialize, Serialize};

/// Lifecycle status of an agent session.
///
/// The six-variant state machine of the domain model. The legal transitions
/// between these states are owned by the pure [`transition`] function (M2) — the
/// SINGLE authority — so no detector can illegally jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Starting,
    Idle,
    Running,
    AwaitingInput,
    Completed,
    Error,
}

/// What a per-agent detector observed from one [`OutputSignal`](crate::OutputSignal).
///
/// Distinct from [`AgentStatus`] (D8) so the legal-transition policy lives in ONE
/// pure place ([`transition`]) rather than being smeared across every detector. A
/// runner reports what it SAW; the Core decides the resulting status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// Agent reached a ready/quiescent prompt.
    Ready,
    /// Agent is actively producing task output.
    Working,
    /// Agent is blocked on a human prompt (permission / question).
    NeedsInput,
    /// Agent finished cleanly (exit 0 OR Generic idle-timeout elapsed).
    Finished,
    /// Agent failed (non-zero exit / crash).
    Failed,
}

/// PURE legal-transition policy. Given the current status and an observed signal,
/// return the next status. Illegal/no-op observations return `current` unchanged.
///
/// This is the SINGLE authority for the state machine in `domain-model.md`:
///
/// ```text
/// Starting ─ready→ Idle ─task→ Running ─prompt→ AwaitingInput ─input→ Running
///                                       └ done→ Completed ; any ─fail→ Error
/// ```
///
/// Terminal states (`Completed`/`Error`) are absorbing: no resurrection without a
/// new Session. The function is total and deterministic — NO I/O, NO time access,
/// NO agent name.
#[must_use]
pub fn transition(current: AgentStatus, observed: Observed) -> AgentStatus {
    use AgentStatus::{AwaitingInput, Completed, Error, Idle, Running, Starting};
    use Observed::{Failed, Finished, NeedsInput, Ready, Working};
    match (current, observed) {
        // Terminal states are absorbing (no resurrection without a new Session).
        (Completed | Error, _) => current,
        // Any non-terminal state may fail (the universal `→ Error` edge).
        (_, Failed) => Error,
        // `Finished` reaches `Completed` ONLY from a state the spec authorises as a
        // `Completed` source: `Running` (spec: `Running → Completed`) and `Idle` (the
        // idle-timeout of roadmap exit-criterion 5). It is NOT a blanket edge: a
        // `Finished` from `Starting` (the named "illegal jump rejected" scenario) or from
        // `AwaitingInput` is an illegal jump that leaves `current` unchanged below.
        (Running | Idle, Finished) => Completed,
        // The forward progress edges.
        (Starting, Ready) => Idle,
        (Starting, Working) => Running, // skipped Idle (immediate task)
        (Idle, Working) => Running,
        (Running, NeedsInput) => AwaitingInput,
        // M3 PRIMARY FIX (D24): a `Stop` hook fires `Ready` when the agent's turn ends
        // cleanly; the AUTHORITATIVE hook path must transition Running → Idle so the
        // bypass-permissions "stuck Running" bug is resolved. PTY scraping's "quiescent
        // burst" Ready now shares this edge — if the PTY emits Ready, the agent has also
        // reached a prompt (i.e., it IS idle). Both paths converge correctly.
        (Running, Ready) => Idle,
        (AwaitingInput, Working) => Running,
        (AwaitingInput, Ready) => Running, // input consumed, output resumed
        // No legal change → no event. This covers every illegal jump, including
        // `(Starting, Finished)`, `(Starting, NeedsInput)`, `(Idle, NeedsInput)`, and
        // `(AwaitingInput, Finished)` — none are spec-enumerated edges.
        _ => current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_status_exposes_all_six_variants() {
        // Naming all six variants compiles only if each is present.
        let all = [
            AgentStatus::Starting,
            AgentStatus::Idle,
            AgentStatus::Running,
            AgentStatus::AwaitingInput,
            AgentStatus::Completed,
            AgentStatus::Error,
        ];
        assert_eq!(all.len(), 6);
    }

    #[test]
    fn transition_running_awaiting_running_round_trip() {
        // Exit-criterion 3: the permission-prompt round trip.
        let awaiting = transition(AgentStatus::Running, Observed::NeedsInput);
        assert_eq!(awaiting, AgentStatus::AwaitingInput);
        let back = transition(AgentStatus::AwaitingInput, Observed::Working);
        assert_eq!(back, AgentStatus::Running);
    }

    #[test]
    fn transition_no_op_observation_leaves_current() {
        // A Working observation while already Running is a legal NO-OP (the agent is
        // already in the state the observation maps to).
        assert_eq!(
            transition(AgentStatus::Running, Observed::Working),
            AgentStatus::Running
        );
        // A Ready observation while already Idle is a legal NO-OP.
        assert_eq!(
            transition(AgentStatus::Idle, Observed::Ready),
            AgentStatus::Idle
        );
    }

    // M3 PRIMARY FIX (D24): a Stop hook fires Ready when the agent's turn ends cleanly.
    // `transition(Running, Ready)` MUST now reach Idle — the bypass-permissions fix.
    #[test]
    fn transition_running_ready_reaches_idle_m3_fix() {
        assert_eq!(
            transition(AgentStatus::Running, Observed::Ready),
            AgentStatus::Idle,
            "M3 PRIMARY FIX: Running + Ready MUST reach Idle (Stop hook drives turn end)"
        );
    }

    #[test]
    fn transition_illegal_jump_starting_to_completed_is_rejected() {
        // Spec.md NAMED scenario "An illegal jump is rejected and leaves current
        // unchanged": `transition(Starting, Completed-observation)` skips Idle/Running and
        // MUST return `Starting` unchanged. `Completed` is observed as `Finished`. The spec
        // enumerates `Completed` reachable only from an ACTIVE state (Running, plus the
        // Idle idle-timeout of exit-criterion 5) — NEVER from `Starting`.
        assert_eq!(
            transition(AgentStatus::Starting, Observed::Finished),
            AgentStatus::Starting
        );
    }

    #[test]
    fn transition_idle_timeout_completes_from_idle() {
        // Roadmap exit-criterion 5: a Generic session reaches `Idle`, then the idle-timeout
        // (surfaced as `Observed::Finished`) transitions it to `Completed`. This is the ONE
        // non-`Running` source of `Completed` the spec authorises.
        assert_eq!(
            transition(AgentStatus::Idle, Observed::Finished),
            AgentStatus::Completed
        );
    }

    #[test]
    fn transition_running_to_completed_on_finish() {
        // Spec: `Running → Completed` is the primary clean-finish edge.
        assert_eq!(
            transition(AgentStatus::Running, Observed::Finished),
            AgentStatus::Completed
        );
    }

    #[test]
    fn transition_awaiting_input_finish_is_rejected() {
        // The spec enumerates `Completed` only from `Running` (+ the Idle idle-timeout).
        // It does NOT list `AwaitingInput → Completed`; a `Finished` while a human is being
        // awaited is not a legal spec edge, so it MUST leave `AwaitingInput` unchanged.
        assert_eq!(
            transition(AgentStatus::AwaitingInput, Observed::Finished),
            AgentStatus::AwaitingInput
        );
    }

    #[test]
    fn transition_any_state_to_error() {
        for current in [
            AgentStatus::Starting,
            AgentStatus::Idle,
            AgentStatus::Running,
            AgentStatus::AwaitingInput,
            AgentStatus::Completed,
            AgentStatus::Error,
        ] {
            assert_eq!(
                transition(current, Observed::Failed),
                expected_failed(current),
                "Failed observation from {current:?}",
            );
        }
    }

    /// Non-terminal states go to Error on Failed; terminal states absorb it.
    fn expected_failed(current: AgentStatus) -> AgentStatus {
        match current {
            AgentStatus::Completed | AgentStatus::Error => current,
            _ => AgentStatus::Error,
        }
    }

    #[test]
    fn transition_starting_ready_reaches_idle() {
        // Exit-criterion 1.
        assert_eq!(
            transition(AgentStatus::Starting, Observed::Ready),
            AgentStatus::Idle
        );
    }

    #[test]
    fn transition_terminal_states_are_absorbing() {
        let observations = [
            Observed::Ready,
            Observed::Working,
            Observed::NeedsInput,
            Observed::Finished,
            Observed::Failed,
        ];
        for observed in observations {
            assert_eq!(
                transition(AgentStatus::Completed, observed),
                AgentStatus::Completed,
                "Completed must absorb {observed:?}",
            );
            assert_eq!(
                transition(AgentStatus::Error, observed),
                AgentStatus::Error,
                "Error must absorb {observed:?}",
            );
        }
    }

    #[test]
    fn transition_covers_the_full_legal_table() {
        use AgentStatus::{AwaitingInput, Completed, Error, Idle, Running, Starting};
        use Observed::{Failed, Finished, NeedsInput, Ready, Working};

        // (current, observed) -> expected next, every cell of the SPEC-FAITHFUL legal
        // table (spec.md is the RFC-2119 normative authority; where the design §3.4 prose
        // table or code block dissents, the spec wins — see the per-cell notes below).
        let table = [
            // Starting row. The spec enumerates only `Starting → Idle` (+ universal
            // `→ Error`). `NeedsInput` before any task has begun is NOT a legal edge → NO-OP
            // (Starting). `Finished` from `Starting` is the named "illegal jump rejected"
            // scenario → NO-OP (Starting), NOT Completed: `Completed` is reachable only from
            // an active state (Running, or the Idle idle-timeout of exit-criterion 5).
            ((Starting, Ready), Idle),
            ((Starting, Working), Running),
            ((Starting, NeedsInput), Starting),
            ((Starting, Finished), Starting),
            ((Starting, Failed), Error),
            // Idle row. The spec lists `AwaitingInput` reachable ONLY from `Running`, so
            // `(Idle, NeedsInput)` is a NO-OP (Idle), not `AwaitingInput` (the design code
            // block over-reached here). `(Idle, Finished)` → Completed is the idle-timeout
            // path of roadmap exit-criterion 5.
            ((Idle, Ready), Idle),
            ((Idle, Working), Running),
            ((Idle, NeedsInput), Idle),
            ((Idle, Finished), Completed),
            ((Idle, Failed), Error),
            // Running row. M3 D24 PRIMARY FIX: Running+Ready → Idle (Stop hook).
            // Spec row updated: Running → {Idle, AwaitingInput, Completed, Error}.
            ((Running, Ready), Idle),
            ((Running, Working), Running),
            ((Running, NeedsInput), AwaitingInput),
            ((Running, Finished), Completed),
            ((Running, Failed), Error),
            // AwaitingInput row. Spec: `AwaitingInput → Running` (after input). A `Finished`
            // here is NOT spec-enumerated → NO-OP (AwaitingInput), not Completed.
            ((AwaitingInput, Ready), Running),
            ((AwaitingInput, Working), Running),
            ((AwaitingInput, NeedsInput), AwaitingInput),
            ((AwaitingInput, Finished), AwaitingInput),
            ((AwaitingInput, Failed), Error),
            // Completed row (absorbing).
            ((Completed, Ready), Completed),
            ((Completed, Working), Completed),
            ((Completed, NeedsInput), Completed),
            ((Completed, Finished), Completed),
            ((Completed, Failed), Completed),
            // Error row (absorbing).
            ((Error, Ready), Error),
            ((Error, Working), Error),
            ((Error, NeedsInput), Error),
            ((Error, Finished), Error),
            ((Error, Failed), Error),
        ];

        for ((current, observed), expected) in table {
            assert_eq!(
                transition(current, observed),
                expected,
                "transition({current:?}, {observed:?})",
            );
        }
    }
}
