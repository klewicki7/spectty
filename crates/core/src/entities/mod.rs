pub mod agent_spec;
pub mod agent_status;
pub mod diff;
pub mod output_signal;
pub mod session;
pub mod session_registry;
pub mod spec;
pub mod workspace;

pub use agent_spec::{AgentCapabilities, AgentDescriptor, AgentKind, AgentSpec, AgentTier};
pub use agent_status::{transition, AgentStatus, Observed};
pub use diff::{DiffExplanation, FileExplanation};
pub use output_signal::{CostDelta, OutputSignal, QuickAction};
pub use session::{Session, SessionId};
pub use session_registry::{SessionRegistry, SessionSummary};
pub use spec::{ApprovalState, SpecContract, SpecError, SpecTask, TaskProgress, TaskState};
pub use workspace::{Workspace, WorkspaceId};
