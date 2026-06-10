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
/// - `Global` → `{home}/.claude/settings.json` where `home` is the caller-supplied
///   resolved home directory (e.g. `std::env::var("HOME")`). The tilde (`~`) is NOT
///   used here — `~` is shell syntax, never expanded by the filesystem. Passing a
///   literal tilde as `home` is the caller's responsibility to avoid (see `lib.rs`
///   `home_settings_json()` which resolves `$HOME` at the composition root, exactly
///   mirroring the `home_claude_json` pattern in [`ClaudeJsonProvisioner`]).
/// - `Project(root)` → `{root}/.claude/settings.json`
///
/// This is a PURE function (no filesystem access). The `home` parameter is ignored
/// for the `Project` variant. It is DISTINCT from the M2 `ClaudeJsonProvisioner`
/// path mapping (`~/.claude.json` / `<root>/.mcp.json`).
pub fn settings_path_for_scope(scope: &ProvisioningScope, home: &str) -> String {
    match scope {
        ProvisioningScope::Global => format!("{home}/.claude/settings.json"),
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

    // ── settings_path_for_scope (WU-3, fixed) ────────────────────────────────
    //
    // RED: the old test pinned the literal `~` as CORRECT. The real bug was that
    // `~` is shell syntax never expanded by the filesystem; when the app launches
    // via Finder/open, cwd is `/` and `create_dir_all("/~/.claude")` hits EROFS on
    // macOS's sealed read-only root.
    //
    // The seam: `settings_path_for_scope` now accepts an explicit `home: &str`
    // parameter. Global scope joins `{home}/.claude/settings.json`; Project scope
    // is unchanged. The impure `std::env::var("HOME")` lookup lives in the caller
    // (ClaudeSettingsProvisioner::new or the composition root), mirroring the
    // ClaudeJsonProvisioner design where `home_claude_json` is injected at
    // construction.

    #[test]
    fn settings_path_global_uses_injected_home_not_tilde() {
        // RED: current impl returns "~/.claude/settings.json" — must FAIL until
        // settings_path_for_scope accepts a home parameter and uses it.
        let path = settings_path_for_scope(&ProvisioningScope::Global, "/Users/alice");
        // Must NOT contain a literal tilde.
        assert!(
            !path.contains('~'),
            "Global settings path must not contain a literal tilde; got: {path}"
        );
        // Must start with the provided home directory (absolute).
        assert!(
            path.starts_with("/Users/alice"),
            "Global settings path must start with the injected home; got: {path}"
        );
        assert_eq!(path, "/Users/alice/.claude/settings.json");
    }

    #[test]
    fn settings_path_global_with_root_home() {
        // Edge: HOME=/root (common on Linux servers/containers).
        let path = settings_path_for_scope(&ProvisioningScope::Global, "/root");
        assert_eq!(path, "/root/.claude/settings.json");
        assert!(!path.contains('~'), "no tilde");
    }

    #[test]
    fn settings_path_project_is_root_dot_claude_settings_json() {
        // Project scope is unchanged (already absolute from workspace_path).
        let path =
            settings_path_for_scope(&ProvisioningScope::Project("/some/repo".to_string()), "");
        assert_eq!(path, "/some/repo/.claude/settings.json");
    }

    #[test]
    fn settings_path_is_distinct_from_claude_json_paths() {
        // The M2 provisioner uses ~/.claude.json and <root>/.mcp.json.
        // The M3 settings provisioner MUST use different paths.
        let global = settings_path_for_scope(&ProvisioningScope::Global, "/Users/alice");
        let project = settings_path_for_scope(&ProvisioningScope::Project("/repo".to_string()), "");

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
