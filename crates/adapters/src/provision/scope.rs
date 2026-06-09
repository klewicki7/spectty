//! Scope resolution (D18).
//!
//! Spectty injects its MCP registration GLOBALLY (`~/.claude.json`) by default, but
//! PROJECT-locally (`<repo_root>/.mcp.json`) when the agent's config is git-tracked
//! — so a committed project config is provisioned alongside the repo, not the user.
//!
//! [`resolve_scope`] is a PURE function over an INJECTED `is_git_tracked` predicate
//! so it is table-tested without touching git. The real probe ([`is_git_tracked`])
//! is the impure shell, kept SEPARATE; a full `GitPort` is M4.

use spectty_core::ProvisioningScope;

/// Resolve the provisioning scope for an agent config.
///
/// Defaults to [`ProvisioningScope::Global`]; resolves to
/// [`ProvisioningScope::Project`] (carrying `repo_root`) when BOTH a `repo_root` is
/// known AND `is_git_tracked(config_path)` is true. Pure over the injected predicate.
pub fn resolve_scope(
    repo_root: Option<&str>,
    config_path: &str,
    is_git_tracked: impl Fn(&str) -> bool,
) -> ProvisioningScope {
    match repo_root {
        Some(root) if is_git_tracked(config_path) => ProvisioningScope::Project(root.to_string()),
        _ => ProvisioningScope::Global,
    }
}

/// Resolve the settings.json path for a scope (M3, WU-3).
///
/// - `Global` → `~/.claude/settings.json` (tilde is NOT expanded here — the caller
///   or the OS resolves `~`; the test pins the literal tilde string).
/// - `Project(root)` → `{root}/.claude/settings.json`
///
/// This is a PURE function (no filesystem access). It is DISTINCT from the M2
/// `ClaudeJsonProvisioner` path mapping (`~/.claude.json` / `<root>/.mcp.json`).
pub fn settings_path_for_scope(scope: &ProvisioningScope) -> String {
    match scope {
        ProvisioningScope::Global => "~/.claude/settings.json".to_string(),
        ProvisioningScope::Project(root) => format!("{root}/.claude/settings.json"),
    }
}

/// The real git-tracked probe: `git ls-files --error-unmatch <path>` → exit-0 means
/// tracked. The impure shell, kept out of [`resolve_scope`] so the resolver stays
/// pure. A full `GitPort` (status, worktrees) is M4.
#[must_use]
pub fn is_git_tracked(path: &str) -> bool {
    std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch", path])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_tracked_config_resolves_to_project_scope() {
        let scope = resolve_scope(Some("/repo"), "/repo/.mcp.json", |_| true);
        assert_eq!(scope, ProvisioningScope::Project("/repo".to_string()));
    }

    #[test]
    fn untracked_config_resolves_to_global_scope() {
        let scope = resolve_scope(Some("/repo"), "/repo/.mcp.json", |_| false);
        assert_eq!(scope, ProvisioningScope::Global);
    }

    #[test]
    fn absent_repo_root_resolves_to_global_even_if_tracked() {
        // Without a known repo root there is no project file to target.
        let scope = resolve_scope(None, "/somewhere/config.json", |_| true);
        assert_eq!(scope, ProvisioningScope::Global);
    }

    // ── settings_path_for_scope (WU-3) ────────────────────────────────────────

    #[test]
    fn settings_path_global_is_tilde_claude_settings_json() {
        let path = settings_path_for_scope(&ProvisioningScope::Global);
        assert_eq!(path, "~/.claude/settings.json");
    }

    #[test]
    fn settings_path_project_is_root_dot_claude_settings_json() {
        let path = settings_path_for_scope(&ProvisioningScope::Project("/some/repo".to_string()));
        assert_eq!(path, "/some/repo/.claude/settings.json");
    }

    #[test]
    fn settings_path_is_distinct_from_claude_json_paths() {
        // The M2 provisioner uses ~/.claude.json and <root>/.mcp.json.
        // The M3 settings provisioner MUST use different paths.
        let global = settings_path_for_scope(&ProvisioningScope::Global);
        let project = settings_path_for_scope(&ProvisioningScope::Project("/repo".to_string()));

        assert_ne!(
            global, "~/.claude.json",
            "must be settings.json not .claude.json"
        );
        assert_ne!(
            project, "/repo/.mcp.json",
            "must be settings.json not .mcp.json"
        );
        assert!(
            global.ends_with("settings.json"),
            "global path ends with settings.json"
        );
        assert!(
            project.ends_with("settings.json"),
            "project path ends with settings.json"
        );
    }
}
