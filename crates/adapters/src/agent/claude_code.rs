use spectty_core::entities::agent_spec::{
    AgentCapabilities, AgentDescriptor, AgentKind, AgentTier,
};
use spectty_core::entities::agent_status::Observed;
use spectty_core::entities::output_signal::OutputSignal;
use spectty_core::ports::agent_runner::{AgentRunner, LaunchContext, LaunchSpec};

/// The `kind` string the [`crate::agent::AgentRunnerRegistry`] resolves to this
/// runner (D12). NOT a Core literal — agent identity lives in the adapter layer.
pub const CLAUDE_CODE_KIND: &str = "claude-code";

/// The program Claude Code is launched as.
const CLAUDE_PROGRAM: &str = "claude";

/// The env var that hands the agent its Spectty session id (for the MCP tools).
const SESSION_ID_ENV: &str = "SPECTTY_SESSION_ID";

/// Empirical scraping PATTERNS as DATA (R5/D11) — co-located with the agent, never
/// in Core. Each pattern is matched as a plain substring against the ANSI-stripped
/// `text_window`; refining a pattern is a one-line data edit plus a unit test, never
/// a Core change. Hand-rolled `contains` keeps `regex` out of the dep graph (D11).
#[derive(Debug, Clone)]
struct ClaudePatterns {
    /// Output that means the agent is blocked on a human (permission / question).
    awaiting_input: &'static [&'static str],
    /// Output that means the agent is at a ready/quiescent prompt.
    ready: &'static [&'static str],
}

/// The M2 placeholder pattern table. These literal substrings are refined against a
/// real Claude Code session during manual acceptance (exit-criterion 3).
const CLAUDE_PATTERNS: ClaudePatterns = ClaudePatterns {
    awaiting_input: &[
        "Do you want to",
        "❯ 1. Yes",
        "(y/n)",
        "Press Enter to continue",
    ],
    ready: &["? for shortcuts"],
};

/// First-class runner for Claude Code (Cooperative tier).
///
/// It requires provisioning (the Spectty MCP-tool registration) and detects status
/// by scraping known prompt/permission patterns held as DATA. `detect_status` is a
/// pure function of the [`OutputSignal`] — no PTY, no clock.
#[derive(Debug, Clone)]
pub struct ClaudeCodeRunner {
    patterns: ClaudePatterns,
}

impl Default for ClaudeCodeRunner {
    fn default() -> Self {
        Self {
            patterns: CLAUDE_PATTERNS,
        }
    }
}

impl ClaudeCodeRunner {
    /// Build a runner with the built-in M2 pattern table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl AgentRunner for ClaudeCodeRunner {
    fn launch_spec(&self, ctx: &LaunchContext) -> LaunchSpec {
        LaunchSpec {
            program: CLAUDE_PROGRAM.to_string(),
            args: Vec::new(),
            // Single pair; already sorted trivially (D20).
            env: vec![(SESSION_ID_ENV.to_string(), ctx.session_id.clone())],
            cwd: ctx.cwd.clone(),
            cols: ctx.cols,
            rows: ctx.rows,
        }
    }

    fn detect_status(&self, signal: &OutputSignal) -> Option<Observed> {
        match signal.exit_code {
            Some(0) => return Some(Observed::Finished),
            Some(_) => return Some(Observed::Failed),
            None => {}
        }
        if self
            .patterns
            .awaiting_input
            .iter()
            .any(|p| signal.text_window.contains(p))
        {
            return Some(Observed::NeedsInput);
        }
        if self
            .patterns
            .ready
            .iter()
            .any(|p| signal.text_window.contains(p))
        {
            return Some(Observed::Ready);
        }
        if signal.is_active {
            return Some(Observed::Working);
        }
        // No confident observation → no transition, no event.
        None
    }

    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            kind: AgentKind(CLAUDE_CODE_KIND.to_string()),
            display_name: "Claude Code".to_string(),
            tier: AgentTier::Cooperative,
            capabilities: AgentCapabilities {
                reports_cost: false,
                // M2 ships heuristic permission detection, not a structured channel.
                structured_permissions: false,
                emits_diff_signals: false,
                requires_provisioning: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectty_core::Timestamp;

    fn signal(text: &str, is_active: bool, exit_code: Option<i32>) -> OutputSignal {
        OutputSignal {
            text_window: text.to_string(),
            is_active,
            exit_code,
            last_byte_at: Timestamp(0),
            idle_ms: 0,
        }
    }

    #[test]
    fn claude_detect_status_each_awaiting_input_pattern_is_needs_input() {
        let runner = ClaudeCodeRunner::new();
        for pattern in CLAUDE_PATTERNS.awaiting_input {
            let window = format!("...output...\n{pattern}\n");
            assert_eq!(
                runner.detect_status(&signal(&window, true, None)),
                Some(Observed::NeedsInput),
                "permission pattern {pattern:?} must observe NeedsInput",
            );
        }
    }

    #[test]
    fn claude_detect_status_each_ready_pattern_is_ready() {
        let runner = ClaudeCodeRunner::new();
        for pattern in CLAUDE_PATTERNS.ready {
            let window = format!("idle box\n{pattern}\n");
            assert_eq!(
                runner.detect_status(&signal(&window, false, None)),
                Some(Observed::Ready),
                "ready pattern {pattern:?} must observe Ready",
            );
        }
    }

    #[test]
    fn claude_detect_status_active_without_match_is_working() {
        let runner = ClaudeCodeRunner::new();
        assert_eq!(
            runner.detect_status(&signal("writing code to main.rs", true, None)),
            Some(Observed::Working)
        );
    }

    #[test]
    fn claude_detect_status_no_match_and_inactive_is_none() {
        let runner = ClaudeCodeRunner::new();
        assert_eq!(
            runner.detect_status(&signal("nothing recognizable here", false, None)),
            None
        );
    }

    #[test]
    fn claude_detect_status_clean_exit_is_finished() {
        let runner = ClaudeCodeRunner::new();
        assert_eq!(
            runner.detect_status(&signal("", false, Some(0))),
            Some(Observed::Finished)
        );
    }

    #[test]
    fn claude_detect_status_nonzero_exit_is_failed() {
        let runner = ClaudeCodeRunner::new();
        assert_eq!(
            runner.detect_status(&signal("", false, Some(127))),
            Some(Observed::Failed)
        );
    }

    #[test]
    fn claude_detect_status_awaiting_input_wins_over_active() {
        // A permission prompt is significant even while bytes are still arriving.
        let runner = ClaudeCodeRunner::new();
        assert_eq!(
            runner.detect_status(&signal("Do you want to proceed?", true, None)),
            Some(Observed::NeedsInput)
        );
    }

    #[test]
    fn claude_launch_spec_program_cwd_and_session_env() {
        let runner = ClaudeCodeRunner::new();
        let ctx = LaunchContext {
            cwd: "/repo".to_string(),
            cols: 100,
            rows: 30,
            session_id: "42".to_string(),
            user_command: None,
        };
        assert_eq!(
            runner.launch_spec(&ctx),
            LaunchSpec {
                program: "claude".to_string(),
                args: Vec::new(),
                env: vec![("SPECTTY_SESSION_ID".to_string(), "42".to_string())],
                cwd: "/repo".to_string(),
                cols: 100,
                rows: 30,
            }
        );
    }

    #[test]
    fn claude_launch_spec_ignores_user_command() {
        // First-class runners derive their launch from `kind`, never a user command.
        let runner = ClaudeCodeRunner::new();
        let ctx = LaunchContext {
            cwd: "/repo".to_string(),
            cols: 80,
            rows: 24,
            session_id: "1".to_string(),
            user_command: Some(vec!["bash".to_string()]),
        };
        assert_eq!(runner.launch_spec(&ctx).program, "claude");
    }

    #[test]
    fn claude_tier_is_cooperative_without_spawning() {
        assert_eq!(ClaudeCodeRunner::new().tier(), AgentTier::Cooperative);
    }

    #[test]
    fn claude_descriptor_requires_provisioning() {
        assert!(
            ClaudeCodeRunner::new()
                .descriptor()
                .capabilities
                .requires_provisioning
        );
    }

    #[test]
    fn claude_parse_cost_is_zero_skeleton() {
        assert_eq!(
            ClaudeCodeRunner::new().parse_cost(&signal("", false, None)),
            None
        );
    }

    #[test]
    fn claude_quick_actions_is_empty_skeleton() {
        use spectty_core::AgentStatus;
        assert!(ClaudeCodeRunner::new()
            .quick_actions(&AgentStatus::AwaitingInput)
            .is_empty());
    }
}
