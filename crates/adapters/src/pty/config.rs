//! Pure PTY spawn configuration.
//!
//! [`PtySpawnConfig`] is a plain data description of *what process to launch in a
//! PTY and at what size*. It is intentionally NOT shaped as an agent/launch spec
//! (no `LaunchSpec`/`AgentSpec`): M1 spawns a raw login shell, and agent typing
//! belongs to M2+. Keeping it a pure struct lets the builder and the per-OS
//! default-shell resolution be unit-tested without touching `portable-pty`.

/// Description of the process to spawn in a PTY plus its initial window size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySpawnConfig {
    /// Executable to launch (e.g. the user's login shell).
    pub program: String,
    /// Arguments passed to `program`.
    pub args: Vec<String>,
    /// Working directory, if a specific one is requested.
    pub cwd: Option<String>,
    /// Initial terminal width in columns.
    pub cols: u16,
    /// Initial terminal height in rows.
    pub rows: u16,
    /// Extra environment variables to set on the child process (key, value pairs).
    /// Used to pass `SPECTTY_SESSION_ID` and any other `LaunchSpec.env` entries
    /// down to the PTY child so hooks and MCP tools can correlate the session.
    pub env: Vec<(String, String)>,
}

impl PtySpawnConfig {
    /// Build a config that launches the per-OS default login shell at the given
    /// size and (optional) working directory.
    ///
    /// `get_env` is injected so the shell resolution is deterministic under test;
    /// production callers pass `|k| std::env::var(k).ok()`.
    #[must_use]
    pub fn shell(
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        get_env: impl Fn(&str) -> Option<String>,
    ) -> Self {
        Self {
            program: default_shell(get_env),
            args: Vec::new(),
            cwd,
            cols,
            rows,
            env: Vec::new(),
        }
    }
}

/// Resolve the default login shell for the host OS.
///
/// Unix: prefer `$SHELL`, falling back to `/bin/bash`.
/// Windows: prefer `%COMSPEC%`, falling back to `cmd.exe`.
///
/// `get_env` is injected to keep this pure and testable.
#[must_use]
pub fn default_shell(get_env: impl Fn(&str) -> Option<String>) -> String {
    #[cfg(windows)]
    {
        get_env("COMSPEC")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string())
    }
    #[cfg(not(windows))]
    {
        get_env("SHELL")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/bash".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Env var the resolver reads on this platform, and its fallback default.
    #[cfg(windows)]
    const SHELL_ENV: &str = "COMSPEC";
    #[cfg(windows)]
    const FALLBACK: &str = "cmd.exe";
    #[cfg(not(windows))]
    const SHELL_ENV: &str = "SHELL";
    #[cfg(not(windows))]
    const FALLBACK: &str = "/bin/bash";

    #[test]
    fn default_shell_prefers_env_shell() {
        let custom = "/usr/bin/fish";
        let resolved = default_shell(|k| (k == SHELL_ENV).then(|| custom.to_string()));

        assert_eq!(
            resolved, custom,
            "the platform shell env var must win when set"
        );
    }

    #[test]
    fn default_shell_falls_back_when_unset() {
        let resolved = default_shell(|_| None);

        assert_eq!(
            resolved, FALLBACK,
            "an unset shell env must fall back to the per-OS default"
        );
    }

    #[test]
    fn pty_spawn_config_shell_sets_program_and_size() {
        let cfg = PtySpawnConfig::shell(120, 40, Some("/tmp/work".to_string()), |k| {
            (k == SHELL_ENV).then(|| "/bin/zsh".to_string())
        });

        assert_eq!(
            cfg,
            PtySpawnConfig {
                program: "/bin/zsh".to_string(),
                args: Vec::new(),
                cwd: Some("/tmp/work".to_string()),
                cols: 120,
                rows: 40,
                env: Vec::new(),
            },
            "shell() must wire program/cwd/size from its inputs"
        );
    }

    // C2 RED: PtySpawnConfig carries env pairs so the caller (session.rs) can populate
    // SPECTTY_SESSION_ID and any other LaunchSpec.env entries. Without this field the
    // env is silently dropped and the sidecar never sees its session id.
    #[test]
    fn pty_spawn_config_carries_env_pairs() {
        let cfg = PtySpawnConfig {
            program: "claude".to_string(),
            args: Vec::new(),
            cwd: None,
            cols: 80,
            rows: 24,
            env: vec![
                ("SPECTTY_SESSION_ID".to_string(), "abc-123".to_string()),
                ("EXTRA".to_string(), "val".to_string()),
            ],
        };
        assert_eq!(cfg.env.len(), 2);
        assert_eq!(
            cfg.env[0],
            ("SPECTTY_SESSION_ID".to_string(), "abc-123".to_string())
        );
        assert_eq!(cfg.env[1], ("EXTRA".to_string(), "val".to_string()));
    }
}
