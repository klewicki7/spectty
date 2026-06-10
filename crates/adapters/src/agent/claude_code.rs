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

/// The empirical pattern table, refined against a real Claude Code v2.1.172 session
/// during M3 manual acceptance (criterion 11.3).
///
/// Patterns are stored WITHOUT whitespace and matched against a whitespace-stripped
/// copy of the text window: Claude Code's Ink renderer emits runs of spaces as
/// cursor-forward CSI sequences (not literal 0x20 bytes), so the ANSI-stripped
/// window concatenates words ("Doyouwanttoproceed?", "❯1.Yes"). Stripping
/// whitespace from BOTH sides makes the match rendering-independent — literal
/// spaces, cursor-forward gaps, and line wraps all collapse to the same key.
/// The `claude_patterns_contain_no_whitespace` test pins this invariant.
const CLAUDE_PATTERNS: ClaudePatterns = ClaudePatterns {
    awaiting_input: &["Doyouwantto", "❯1.Yes", "(y/n)", "PressEntertocontinue"],
    ready: &["?forshortcuts"],
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
    /// Build a runner with the built-in empirical pattern table.
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
        // Whitespace-stripped view of the window: Ink renders space runs as
        // cursor-forward CSI sequences, so the ANSI-stripped window concatenates
        // words. Patterns are stored whitespace-free (see CLAUDE_PATTERNS) and
        // matched against this compact view so both renderings match.
        let compact: String = signal
            .text_window
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if self
            .patterns
            .awaiting_input
            .iter()
            .any(|p| compact.contains(p))
        {
            return Some(Observed::NeedsInput);
        }
        if self.patterns.ready.iter().any(|p| compact.contains(p)) {
            return Some(Observed::Ready);
        }
        if signal.is_active {
            return Some(Observed::Working);
        }
        // Quiescent (the spinner stopped emitting) with no pending permission
        // prompt → the agent is idle at its prompt. Quietness — NOT a scraped
        // footer string — is the robust Ready signal (mirrors GenericRunner). In
        // Claude Code v2.1.169 the footer is IDENTICAL between working and idle
        // (e.g. "bypass permissions on (shift+tab to cycle)"), so only `is_active`
        // distinguishes them; the empirical `ready` patterns above remain a
        // best-effort fast-path. Unlike the Generic runner this never times out to
        // Completed — a Cooperative session stays Idle until it exits.
        Some(Observed::Ready)
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
    fn claude_detect_status_quiescent_no_prompt_is_ready() {
        // Quietness — NOT a scraped footer string — is the robust Idle signal
        // (mirrors GenericRunner). A quiescent session with no pending permission
        // prompt is idle at its prompt, even when no `ready` pattern matches.
        let runner = ClaudeCodeRunner::new();
        assert_eq!(
            runner.detect_status(&signal("nothing recognizable here", false, None)),
            Some(Observed::Ready)
        );
    }

    #[test]
    fn claude_detect_status_quiescent_bypass_footer_is_ready() {
        // Real Claude Code v2.1.169 idle (exit-criterion 1, L4): the footer shows
        // the permission-mode hint, there is NO spinner, and the PTY is quiescent.
        // The footer is IDENTICAL between working and idle, so only quietness can
        // distinguish them — this MUST observe Ready, not stick at Working.
        let runner = ClaudeCodeRunner::new();
        let window = "bypass permissions on (shift+tab to cycle) · ← for agents";
        assert_eq!(
            runner.detect_status(&signal(window, false, None)),
            Some(Observed::Ready)
        );
    }

    #[test]
    fn claude_detect_status_active_spinner_with_bypass_footer_is_working() {
        // Real v2.1.169 working: the same mode-hint footer is STILL present, but a
        // spinner line animates (bytes keep arriving → is_active). The shared
        // footer must NOT be read as Ready while the agent is actively working.
        let runner = ClaudeCodeRunner::new();
        let window = "Tomfoolering… (2s · thinking)\nbypass permissions on (shift+tab to cycle)";
        assert_eq!(
            runner.detect_status(&signal(window, true, None)),
            Some(Observed::Working)
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
    fn claude_detect_status_space_stripped_permission_dialog_is_needs_input() {
        // REAL window captured from Claude Code v2.1.172 inside Spectty (M3
        // acceptance 11.3): Ink renders runs of spaces as cursor-forward CSI
        // sequences, NOT literal 0x20 bytes, so the ANSI-stripped text window
        // concatenates words ("Doyouwanttoproceed?", "❯1.Yes"). Pattern matching
        // must therefore be whitespace-insensitive — spaced patterns can NEVER
        // match this rendering.
        let runner = ClaudeCodeRunner::new();
        let window = "Bash command\r\r\r\ntouch/tmp/claude-permission-test&&echo\"permisoconcedido\"\r\r\nCreateaharmlesstestfilein/tmp\r\r\n\r\r\nDoyouwanttoproceed?\r\r\n❯1.Yes\r\r\n2.Yes,andalwaysallowaccesstotmp/fromthisproject\r\r\n3.No\r\r\n\r\r\nEsctocancel·Tabtoamend·ctrl+etoexplain";
        // While the dialog sits quiescent on screen…
        assert_eq!(
            runner.detect_status(&signal(window, false, None)),
            Some(Observed::NeedsInput),
            "quiescent space-stripped permission dialog must observe NeedsInput"
        );
        // …and while the TUI is still actively redrawing it.
        assert_eq!(
            runner.detect_status(&signal(window, true, None)),
            Some(Observed::NeedsInput),
            "active space-stripped permission dialog must observe NeedsInput"
        );
    }

    #[test]
    fn claude_patterns_contain_no_whitespace() {
        // DATA pin: detect_status matches patterns against a whitespace-stripped
        // window, so a pattern containing whitespace can never match. Adding one
        // is a silent dead pattern — this test makes it loud.
        for pattern in CLAUDE_PATTERNS
            .awaiting_input
            .iter()
            .chain(CLAUDE_PATTERNS.ready)
        {
            assert!(
                !pattern.chars().any(char::is_whitespace),
                "pattern {pattern:?} contains whitespace — it can never match the \
                 whitespace-stripped window"
            );
        }
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
