use crate::entities::agent_spec::{AgentDescriptor, AgentTier};
use crate::entities::agent_status::{AgentStatus, Observed};
use crate::entities::output_signal::{CostDelta, OutputSignal, QuickAction};

/// Per-agent context for launching an agent in a PTY.
///
/// PURE data: the resolved workspace `cwd` + window size + the session id (so the
/// agent can be told its `session_id` via env for the MCP tools) plus an optional
/// user command for the Generic agent. No agent name and no process handle live
/// here — the runner adapter maps this onto a [`LaunchSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContext {
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub session_id: String,
    /// Optional user command for the Generic agent (ignored by first-class
    /// runners that derive their launch from `kind`).
    pub user_command: Option<Vec<String>>,
}

/// What to spawn in a PTY. Core-pure mirror of the adapter-layer `PtySpawnConfig`.
///
/// The composition root maps this 1:1 onto the adapter's spawn config. `env` is a
/// SORTED `Vec<(String, String)>` (NOT a `HashMap`, D20) so `launch_spec` equality
/// is deterministic in tests — no `HashMap` iteration-order noise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    /// Sorted (by key) `(key, value)` environment pairs (D20).
    pub env: Vec<(String, String)>,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
}

/// The single agent-agnostic launch + status port (ADR-0004).
///
/// The Core carries ZERO agent names; each implementation in the adapter layer
/// encapsulates one agent. The M2 method subset:
/// - [`launch_spec`](AgentRunner::launch_spec), [`detect_status`](AgentRunner::detect_status)
///   and [`descriptor`](AgentRunner::descriptor) are fully specified;
/// - [`parse_cost`](AgentRunner::parse_cost) and
///   [`quick_actions`](AgentRunner::quick_actions) are honest, tested SKELETONS
///   (return `None` / empty).
///
/// `detect_status` returns `Option<Observed>` — what the detector SAW — NOT an
/// `AgentStatus` (D8). The pure Core [`transition`](crate::transition) decides the
/// resulting status, so the legal-transition policy lives in ONE place and a runner
/// can never illegally jump.
///
/// NOTE (R9/D7): ADR-0004 sketched a `provisioner(&self) -> Option<Box<dyn _>>`
/// method on this trait. M2 OVERRIDES that: provisioning is the separate
/// [`ProvisioningPort`](crate::ports::provisioning::ProvisioningPort). This trait
/// carries NO `provisioner()` method; the composition root reads
/// [`AgentCapabilities::requires_provisioning`](crate::AgentCapabilities) instead.
pub trait AgentRunner: Send + Sync {
    /// Map a launch context onto the concrete process to spawn in the PTY.
    fn launch_spec(&self, ctx: &LaunchContext) -> LaunchSpec;

    /// Observe recent output. `None` = "no confident observation this tick" →
    /// no transition, no event. The Core [`transition`](crate::transition) maps the
    /// returned [`Observed`] onto the next [`AgentStatus`](crate::AgentStatus).
    fn detect_status(&self, signal: &OutputSignal) -> Option<Observed>;

    /// SKELETON (M2): always returns `None`. M3 parses real token/cost deltas.
    fn parse_cost(&self, _signal: &OutputSignal) -> Option<CostDelta> {
        None
    }

    /// SKELETON (M2): always returns an empty set. M3 offers real prompt answers.
    fn quick_actions(&self, _status: &AgentStatus) -> Vec<QuickAction> {
        Vec::new()
    }

    /// UI-facing identity + capabilities so the UI degrades gracefully (ADR-0004).
    fn descriptor(&self) -> AgentDescriptor;

    /// The cooperation tier of this agent. Defaults to the descriptor's tier so an
    /// implementation only overrides it when it needs to (the spec asserts
    /// `ClaudeCodeRunner` reports `Cooperative` and `GenericRunner` reports
    /// `Generic` without spawning either agent).
    fn tier(&self) -> AgentTier {
        self.descriptor().tier
    }
}
