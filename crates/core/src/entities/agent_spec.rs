use serde::{Deserialize, Serialize};

/// Which agent a [`Session`](crate::entities::Session) runs and at what cooperation
/// tier.
///
/// PURE data — no agent behavior and no command strings live here; the runner
/// adapter maps [`AgentSpec::kind`] to a concrete launch. [`AgentKind`] is a serde
/// STRING newtype (NOT a closed enum) so adding an agent never edits Core (D12).
/// The Core NEVER branches on the `kind` value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    /// Opaque agent identifier the runner registry resolves to a concrete runner,
    /// e.g. `"claude-code"` | `"generic"`.
    pub kind: AgentKind,
    /// For the Generic agent: the user-supplied program + args. `None` for
    /// first-class agents that derive their launch from `kind`.
    pub command: Option<Vec<String>>,
    pub tier: AgentTier,
}

/// Opaque, serde-string agent identifier (D12).
///
/// A `String` newtype rather than a closed Core enum so that registering a new
/// agent is an adapter-layer change (the `AgentRunnerRegistry` maps the string to
/// a runner), never a Core edit — exactly the agent-agnostic boundary ADR-0004
/// requires.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentKind(pub String);

/// How cooperatively an agent participates in the Spectty Agent Protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentTier {
    /// First-class agents that speak the protocol (e.g. Claude Code).
    Cooperative,
    /// Plain shell/command agents driven by output heuristics only.
    Generic,
}

/// UI-facing identity + capabilities so the UI degrades gracefully (ADR-0004).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDescriptor {
    pub kind: AgentKind,
    pub display_name: String,
    pub tier: AgentTier,
    pub capabilities: AgentCapabilities,
}

/// Static capability flags that let the composition root and UI adapt per agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub reports_cost: bool,
    pub structured_permissions: bool,
    pub emits_diff_signals: bool,
    /// `false` for Generic → the composition root skips wiring the
    /// `ProvisioningPort` for this Session WITHOUT the runner carrying a
    /// `provisioner()` trait method (the R9/D7 separation).
    pub requires_provisioning: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> AgentSpec {
        AgentSpec {
            kind: AgentKind("claude-code".to_string()),
            command: None,
            tier: AgentTier::Cooperative,
        }
    }

    #[test]
    fn agent_spec_round_trips_through_serde() {
        let spec = sample_spec();
        let json = serde_json::to_string(&spec).expect("serialize");
        let back: AgentSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(spec, back);
    }

    #[test]
    fn agent_spec_round_trips_with_generic_command() {
        let spec = AgentSpec {
            kind: AgentKind("generic".to_string()),
            command: Some(vec!["bash".to_string(), "-l".to_string()]),
            tier: AgentTier::Generic,
        };
        let json = serde_json::to_string(&spec).expect("serialize");
        let back: AgentSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(spec, back);
    }

    #[test]
    fn agent_kind_serializes_as_a_bare_string() {
        let json = serde_json::to_string(&AgentKind("generic".to_string())).expect("serialize");
        assert_eq!(json, "\"generic\"");
    }

    #[test]
    fn agent_descriptor_round_trips_through_serde() {
        let descriptor = AgentDescriptor {
            kind: AgentKind("claude-code".to_string()),
            display_name: "Claude Code".to_string(),
            tier: AgentTier::Cooperative,
            capabilities: AgentCapabilities {
                reports_cost: false,
                structured_permissions: false,
                emits_diff_signals: false,
                requires_provisioning: true,
            },
        };
        let json = serde_json::to_string(&descriptor).expect("serialize");
        let back: AgentDescriptor = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(descriptor, back);
    }
}
