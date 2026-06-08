//! `agent` — the M2 [`AgentRunner`](spectty_core::ports::agent_runner::AgentRunner)
//! adapter layer.
//!
//! This module owns the two concrete runners and the string→runner registry. It is
//! the ADAPTER side of the hexagon: agent names (`claude`, the default shell),
//! scraping patterns, and tier/capability data ALL live here — `spectty-core` knows
//! none of them. `detect_status` returns `Option<Observed>` (D8); the pure Core
//! [`transition`](spectty_core::transition) owns the state machine.
//!
//! Layout:
//! - [`GenericRunner`]: heuristic idle-timeout runner (Generic tier, no provisioning).
//! - [`ClaudeCodeRunner`]: pattern-scraping runner (Cooperative tier, provisioned).
//! - [`AgentRunnerRegistry`]: maps an [`AgentKind`](spectty_core::AgentKind) STRING
//!   to a runner (D12), so adding an agent never edits Core.

pub mod claude_code;
pub mod generic;
pub mod output_signal;

pub use claude_code::ClaudeCodeRunner;
pub use generic::GenericRunner;
pub use output_signal::OutputSignalProducer;

use std::collections::HashMap;

use spectty_core::ports::agent_runner::AgentRunner;
use spectty_core::AgentKind;

/// Default Generic idle-timeout, in millis (roadmap exit-criterion 5). The
/// composition root may build a registry with a different value.
pub const DEFAULT_GENERIC_IDLE_TIMEOUT_MS: u64 = 3_000;

/// Resolves an [`AgentKind`] string to a concrete [`AgentRunner`] (D12).
///
/// The registry is the ONLY place that maps the opaque `kind` string onto a runner;
/// because the mapping lives in the adapter layer, registering a new agent is an
/// adapter change, never a Core edit — exactly the agent-agnostic boundary ADR-0004
/// requires.
pub struct AgentRunnerRegistry {
    runners: HashMap<AgentKind, Box<dyn AgentRunner>>,
}

impl AgentRunnerRegistry {
    /// Build the registry with the two M2 built-in runners (`"claude-code"` and
    /// `"generic"`). Production callers use this; tests may resolve either kind
    /// without spawning a process.
    #[must_use]
    pub fn with_builtin() -> Self {
        Self::with_generic_idle_timeout(DEFAULT_GENERIC_IDLE_TIMEOUT_MS)
    }

    /// Like [`with_builtin`](Self::with_builtin) but with a custom Generic
    /// idle-timeout (the configurable inactivity window of exit-criterion 5).
    #[must_use]
    pub fn with_generic_idle_timeout(idle_timeout_ms: u64) -> Self {
        let mut runners: HashMap<AgentKind, Box<dyn AgentRunner>> = HashMap::new();
        runners.insert(
            AgentKind(claude_code::CLAUDE_CODE_KIND.to_string()),
            Box::new(ClaudeCodeRunner::new()),
        );
        runners.insert(
            AgentKind(generic::GENERIC_KIND.to_string()),
            Box::new(GenericRunner::new(idle_timeout_ms, |k| {
                std::env::var(k).ok()
            })),
        );
        Self { runners }
    }

    /// Resolve a runner by kind, or `None` when no runner is registered for it.
    #[must_use]
    pub fn resolve(&self, kind: &AgentKind) -> Option<&dyn AgentRunner> {
        self.runners.get(kind).map(AsRef::as_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectty_core::AgentTier;

    #[test]
    fn registry_resolves_claude_code_to_cooperative_runner() {
        let registry = AgentRunnerRegistry::with_builtin();
        let runner = registry
            .resolve(&AgentKind("claude-code".to_string()))
            .expect("claude-code is a built-in");
        assert_eq!(runner.tier(), AgentTier::Cooperative);
    }

    #[test]
    fn registry_resolves_generic_to_generic_runner() {
        let registry = AgentRunnerRegistry::with_builtin();
        let runner = registry
            .resolve(&AgentKind("generic".to_string()))
            .expect("generic is a built-in");
        assert_eq!(runner.tier(), AgentTier::Generic);
    }

    #[test]
    fn registry_returns_none_for_unknown_kind() {
        let registry = AgentRunnerRegistry::with_builtin();
        assert!(registry
            .resolve(&AgentKind("unregistered".to_string()))
            .is_none());
    }
}
