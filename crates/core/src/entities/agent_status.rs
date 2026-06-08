use serde::{Deserialize, Serialize};

/// Lifecycle status of an agent session.
///
/// M0: behaviorless placeholder — variants only. Transition rules and the
/// associated state machine are deferred to M2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Starting,
    Idle,
    Running,
    AwaitingInput,
    Completed,
    Error,
}
