//! [`ClaudeJsonProvisioner`] — the [`ProvisioningPort`] impl (D14).
//!
//! Composes the three seams of this module: the pure JSON namespace
//! [editor](super::json_namespace) writes the managed `spectty` entry; the
//! [`ConfigFile`] seam performs the atomic write + one-time backup; the scope
//! decides which file is targeted (GLOBAL `~/.claude.json` top-level `mcpServers`
//! vs PROJECT `<repo_root>/.mcp.json`).
//!
//! Scope resolution is NOT done here — the composition root resolves scope once
//! (via [`resolve_scope`](super::scope::resolve_scope)) and passes it to
//! [`inject`](ProvisioningPort::inject); the returned [`ProvisioningHandle`] is
//! stored on the Session so [`retract`](ProvisioningPort::retract) targets the EXACT
//! scope that was injected.

use spectty_core::{ProvisioningError, ProvisioningHandle, ProvisioningPort, ProvisioningScope};

use super::file_io::ConfigFile;
use super::json_namespace::{inject_spectty_mcp, retract_spectty_mcp, McpServerEntry};

/// The single managed MCP server key Spectty owns under `mcpServers`.
const MANAGED_SERVER_NAME: &str = "spectty";

/// A [`ProvisioningPort`] that edits Claude Code's JSON config files.
///
/// Generic over the [`ConfigFile`] seam so tests inject an in-memory fake. The
/// `mcp_entry.command` points at the installed `spectty-mcp` stub binary (WU-8).
pub struct ClaudeJsonProvisioner<F: ConfigFile> {
    files: F,
    /// The resolved absolute path to `~/.claude.json` (GLOBAL scope target).
    home_claude_json: String,
    /// The entry Spectty registers (points at the `spectty-mcp` binary).
    mcp_entry: McpServerEntry,
}

impl<F: ConfigFile> ClaudeJsonProvisioner<F> {
    /// Build a provisioner over a [`ConfigFile`] seam, the resolved global config
    /// path, and the managed MCP entry.
    pub fn new(files: F, home_claude_json: String, mcp_entry: McpServerEntry) -> Self {
        Self {
            files,
            home_claude_json,
            mcp_entry,
        }
    }

    /// The config file path for a scope. GLOBAL → `~/.claude.json` top-level;
    /// PROJECT → `<repo_root>/.mcp.json`.
    fn path_for(&self, scope: &ProvisioningScope) -> String {
        match scope {
            ProvisioningScope::Global => self.home_claude_json.clone(),
            ProvisioningScope::Project(root) => format!("{root}/.mcp.json"),
        }
    }
}

impl<F: ConfigFile> ProvisioningPort for ClaudeJsonProvisioner<F> {
    fn inject(&self, scope: ProvisioningScope) -> Result<ProvisioningHandle, ProvisioningError> {
        let path = self.path_for(&scope);
        // An absent config starts as an empty JSON object; the editor creates
        // `mcpServers` for us.
        let current = self
            .files
            .read(&path)
            .map_err(|e| ProvisioningError::Io(e.to_string()))?
            .unwrap_or_else(|| "{}".to_string());

        let next = inject_spectty_mcp(&current, MANAGED_SERVER_NAME, &self.mcp_entry)?;

        self.files
            .write_atomic(&path, &next)
            .map_err(|e| ProvisioningError::Io(e.to_string()))?;

        Ok(ProvisioningHandle { scope })
    }

    fn retract(&self, handle: &ProvisioningHandle) -> Result<(), ProvisioningError> {
        let path = self.path_for(&handle.scope);
        // Retracting an absent config is a no-op (idempotent close).
        let Some(current) = self
            .files
            .read(&path)
            .map_err(|e| ProvisioningError::Io(e.to_string()))?
        else {
            return Ok(());
        };

        let next = retract_spectty_mcp(&current, MANAGED_SERVER_NAME)?;

        self.files
            .write_atomic(&path, &next)
            .map_err(|e| ProvisioningError::Io(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::file_io::fake::FakeConfigFile;
    use super::*;

    fn entry() -> McpServerEntry {
        McpServerEntry {
            command: "/usr/local/bin/spectty-mcp".to_string(),
            args: vec!["--stdio".to_string()],
            env: vec![],
        }
    }

    #[test]
    fn inject_global_writes_managed_entry_and_returns_handle() {
        let home = "/home/.claude.json";
        let provisioner = ClaudeJsonProvisioner::new(
            FakeConfigFile::with_file(home, "{}"),
            home.to_string(),
            entry(),
        );

        let handle = provisioner
            .inject(ProvisioningScope::Global)
            .expect("inject ok");

        assert_eq!(
            handle.scope,
            ProvisioningScope::Global,
            "handle carries scope"
        );

        // The test module is a child of this module, so the private `files` field
        // is visible — read the seam back to assert what was persisted.
        let current = provisioner
            .files
            .read(home)
            .expect("read")
            .expect("file present");
        assert!(
            current.contains("spectty"),
            "managed entry written to global config"
        );
    }

    #[test]
    fn inject_backs_up_original_before_first_write() {
        let home = "/home/.claude.json";
        let provisioner = ClaudeJsonProvisioner::new(
            FakeConfigFile::with_file(home, "{}"),
            home.to_string(),
            entry(),
        );

        provisioner
            .inject(ProvisioningScope::Global)
            .expect("inject");

        let backup = provisioner
            .files
            .read(&format!("{home}.spectty.bak"))
            .expect("read bak")
            .expect("backup present");
        assert_eq!(backup, "{}", "original backed up before first write");
    }

    #[test]
    fn inject_project_targets_repo_root_mcp_json() {
        let provisioner = ClaudeJsonProvisioner::new(
            FakeConfigFile::default(),
            "/home/.claude.json".to_string(),
            entry(),
        );

        provisioner
            .inject(ProvisioningScope::Project("/repo".to_string()))
            .expect("inject project");

        let written = provisioner
            .files
            .read("/repo/.mcp.json")
            .expect("read")
            .expect("project file written");
        assert!(
            written.contains("spectty"),
            "project scope targets .mcp.json at repo root"
        );
    }

    #[test]
    fn retract_absent_file_is_ok() {
        let provisioner = ClaudeJsonProvisioner::new(
            FakeConfigFile::default(),
            "/home/.claude.json".to_string(),
            entry(),
        );

        let handle = ProvisioningHandle {
            scope: ProvisioningScope::Global,
        };
        provisioner
            .retract(&handle)
            .expect("retract on absent file is Ok");
    }

    #[test]
    fn inject_then_retract_removes_managed_entry() {
        let home = "/home/.claude.json";
        let provisioner = ClaudeJsonProvisioner::new(
            FakeConfigFile::with_file(home, "{}"),
            home.to_string(),
            entry(),
        );

        let handle = provisioner
            .inject(ProvisioningScope::Global)
            .expect("inject");
        provisioner.retract(&handle).expect("retract");

        let current = provisioner
            .files
            .read(home)
            .expect("read")
            .expect("file present");
        assert!(!current.contains("spectty-mcp"), "managed entry retracted");
    }
}
