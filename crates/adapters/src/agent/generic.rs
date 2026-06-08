use spectty_core::entities::agent_spec::{
    AgentCapabilities, AgentDescriptor, AgentKind, AgentTier,
};
use spectty_core::entities::agent_status::Observed;
use spectty_core::entities::output_signal::OutputSignal;
use spectty_core::ports::agent_runner::{AgentRunner, LaunchContext, LaunchSpec};

use crate::pty::config::default_shell;

/// The `kind` string the [`crate::agent::AgentRunnerRegistry`] resolves to this
/// runner (D12). NOT a Core literal — agent identity lives in the adapter layer.
pub const GENERIC_KIND: &str = "generic";

/// A heuristic runner for plain shell/command agents (Generic tier).
///
/// It speaks none of the Spectty Agent Protocol and requires no provisioning. Its
/// `detect_status` is driven ENTIRELY by the precomputed fields of an
/// [`OutputSignal`] — an exit code or the INJECTED `idle_ms` versus a configurable
/// `idle_timeout_ms` — so it is a pure, table-testable function with no wall clock
/// (D10).
#[derive(Debug, Clone)]
pub struct GenericRunner {
    /// Inactivity window, in millis, after which a quiescent session is treated as
    /// `Finished` (roadmap exit-criterion 5). Injected so the boundary is testable.
    idle_timeout_ms: u64,
    /// Resolved env reader for the default shell fallback (deterministic in tests).
    default_program: String,
}

impl GenericRunner {
    /// Build a runner with the given inactivity window, resolving the per-OS
    /// default shell up-front via the injected `get_env` (keeps `launch_spec` pure
    /// and deterministic under test, mirroring `PtySpawnConfig::shell`).
    #[must_use]
    pub fn new(idle_timeout_ms: u64, get_env: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            idle_timeout_ms,
            default_program: default_shell(get_env),
        }
    }
}

impl AgentRunner for GenericRunner {
    fn launch_spec(&self, ctx: &LaunchContext) -> LaunchSpec {
        let (program, args) = match ctx.user_command.as_deref() {
            Some([program, rest @ ..]) => (program.clone(), rest.to_vec()),
            _ => (self.default_program.clone(), Vec::new()),
        };
        LaunchSpec {
            program,
            args,
            env: Vec::new(),
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
        if signal.is_active {
            return Some(Observed::Working);
        }
        if signal.idle_ms >= self.idle_timeout_ms {
            // Roadmap exit-criterion 5: a quiescent Generic session times out and
            // the Core transition maps `Finished` from `Idle` to `Completed`.
            return Some(Observed::Finished);
        }
        // Quiescent but not yet timed out → reached a ready/idle prompt.
        Some(Observed::Ready)
    }

    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            kind: AgentKind(GENERIC_KIND.to_string()),
            display_name: "Generic".to_string(),
            tier: AgentTier::Generic,
            capabilities: AgentCapabilities {
                reports_cost: false,
                structured_permissions: false,
                emits_diff_signals: false,
                requires_provisioning: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectty_core::Timestamp;

    fn signal(is_active: bool, idle_ms: u64, exit_code: Option<i32>) -> OutputSignal {
        OutputSignal {
            text_window: String::new(),
            is_active,
            exit_code,
            last_byte_at: Timestamp(0),
            idle_ms,
        }
    }

    fn runner(idle_timeout_ms: u64) -> GenericRunner {
        GenericRunner::new(idle_timeout_ms, |_| None)
    }

    #[test]
    fn generic_detect_status_clean_exit_is_finished() {
        assert_eq!(
            runner(3_000).detect_status(&signal(false, 0, Some(0))),
            Some(Observed::Finished)
        );
    }

    #[test]
    fn generic_detect_status_nonzero_exit_is_failed() {
        assert_eq!(
            runner(3_000).detect_status(&signal(false, 0, Some(1))),
            Some(Observed::Failed)
        );
    }

    #[test]
    fn generic_detect_status_active_output_is_working() {
        assert_eq!(
            runner(3_000).detect_status(&signal(true, 0, None)),
            Some(Observed::Working)
        );
    }

    #[test]
    fn generic_detect_status_just_under_threshold_is_ready_not_finished() {
        // Boundary: idle_ms strictly below the timeout is NOT a timeout → Ready
        // (which the Core transition maps Starting→Idle; it never completes).
        assert_eq!(
            runner(3_000).detect_status(&signal(false, 2_999, None)),
            Some(Observed::Ready)
        );
    }

    #[test]
    fn generic_detect_status_at_threshold_is_finished() {
        // Boundary: idle_ms == timeout is inclusive → idle-timeout fires.
        assert_eq!(
            runner(3_000).detect_status(&signal(false, 3_000, None)),
            Some(Observed::Finished)
        );
    }

    #[test]
    fn generic_detect_status_over_threshold_is_finished() {
        assert_eq!(
            runner(3_000).detect_status(&signal(false, 9_999, None)),
            Some(Observed::Finished)
        );
    }

    #[test]
    fn generic_launch_spec_uses_user_command() {
        let runner = runner(3_000);
        let ctx = LaunchContext {
            cwd: "/tmp/work".to_string(),
            cols: 120,
            rows: 40,
            session_id: "7".to_string(),
            user_command: Some(vec!["bash".to_string(), "-l".to_string()]),
        };
        assert_eq!(
            runner.launch_spec(&ctx),
            LaunchSpec {
                program: "bash".to_string(),
                args: vec!["-l".to_string()],
                env: Vec::new(),
                cwd: "/tmp/work".to_string(),
                cols: 120,
                rows: 40,
            }
        );
    }

    #[test]
    fn generic_launch_spec_falls_back_to_default_shell() {
        let runner = GenericRunner::new(3_000, |k| {
            (k == "SHELL" || k == "COMSPEC").then(|| "/bin/zsh".to_string())
        });
        let ctx = LaunchContext {
            cwd: "/repo".to_string(),
            cols: 80,
            rows: 24,
            session_id: "1".to_string(),
            user_command: None,
        };
        let spec = runner.launch_spec(&ctx);
        assert_eq!(spec.args, Vec::<String>::new());
        assert_eq!(spec.cwd, "/repo");
        // The program is the per-OS default shell resolved at construction.
        assert!(!spec.program.is_empty());
    }

    #[test]
    fn generic_tier_is_generic_without_spawning() {
        assert_eq!(runner(3_000).tier(), AgentTier::Generic);
    }

    #[test]
    fn generic_descriptor_requires_no_provisioning() {
        assert!(
            !runner(3_000)
                .descriptor()
                .capabilities
                .requires_provisioning
        );
    }

    #[test]
    fn generic_parse_cost_is_zero_skeleton() {
        assert_eq!(runner(3_000).parse_cost(&signal(false, 0, None)), None);
    }

    #[test]
    fn generic_quick_actions_is_empty_skeleton() {
        use spectty_core::AgentStatus;
        assert!(runner(3_000).quick_actions(&AgentStatus::Idle).is_empty());
    }
}
