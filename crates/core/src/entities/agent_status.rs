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
        // Any non-terminal state may fail or finish.
        (_, Failed) => Error,
        (_, Finished) => Completed,
        // The forward progress edges.
        (Starting, Ready) => Idle,
        (Starting, Working) => Running, // skipped Idle (immediate task)
        (Idle, Working) => Running,
        (Idle, NeedsInput) => AwaitingInput,
        (Running, NeedsInput) => AwaitingInput,
        (Running, Ready) => Running, // still running, quiescent burst
        (AwaitingInput, Working) => Running,
        (AwaitingInput, Ready) => Running, // input consumed, output resumed
        // No legal change → no event.
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
        // A quiescent `Ready` burst while already Running is a legal NO-OP: the design
        // §3.4 table keeps `current` so the caller emits no `status_changed` event. This
        // is the post-D8 form of "an observation that maps to no state change is ignored".
        assert_eq!(
            transition(AgentStatus::Running, Observed::Working),
            AgentStatus::Running
        );
        assert_eq!(
            transition(AgentStatus::Idle, Observed::Ready),
            AgentStatus::Idle
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

        // (current, observed) -> expected next, every cell of the design §3.4 table.
        let table = [
            // Starting row. NOTE: `NeedsInput` before any task has begun is NOT a legal
            // edge (the spec lists only `Starting→Idle` plus the universal `→Error`/done);
            // it is a NO-OP that leaves `Starting`. The design §3.4 prose TABLE printed
            // `Starting/NeedsInput→AwaitingInput`, which contradicts the design CODE block
            // (no such arm) and the spec — resolved in favour of the spec + code block.
            ((Starting, Ready), Idle),
            ((Starting, Working), Running),
            ((Starting, NeedsInput), Starting),
            ((Starting, Finished), Completed),
            ((Starting, Failed), Error),
            // Idle row.
            ((Idle, Ready), Idle),
            ((Idle, Working), Running),
            ((Idle, NeedsInput), AwaitingInput),
            ((Idle, Finished), Completed),
            ((Idle, Failed), Error),
            // Running row.
            ((Running, Ready), Running),
            ((Running, Working), Running),
            ((Running, NeedsInput), AwaitingInput),
            ((Running, Finished), Completed),
            ((Running, Failed), Error),
            // AwaitingInput row.
            ((AwaitingInput, Ready), Running),
            ((AwaitingInput, Working), Running),
            ((AwaitingInput, NeedsInput), AwaitingInput),
            ((AwaitingInput, Finished), Completed),
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
