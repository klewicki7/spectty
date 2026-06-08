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
}
