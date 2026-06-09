//! [`ClaudeSettingsProvisioner`] — the SECOND [`ProvisioningPort`] impl (D21, WU-3).
//!
//! Manages ONLY the `hooks` key in `~/.claude/settings.json` (Global) or
//! `{root}/.claude/settings.json` (Project). It MUST NOT touch `mcpServers`,
//! `permissions`, `env`, `model`, or any other key.
//!
//! Reuses the M2 [`ConfigFile`] atomic-write seam (tmp → fsync → rename +
//! `.spectty.bak` one-time backup). Composition root manages it as a SECOND
//! `Arc<dyn ProvisioningPort>` injected/retracted alongside the MCP one (D21).
//!
//! The `hook_command` field is the resolved absolute path of the `spectty-hook`
//! binary: used as the owned-row identity for R7-generalized retract.

use spectty_core::{ProvisioningError, ProvisioningHandle, ProvisioningPort, ProvisioningScope};

use super::file_io::ConfigFile;
use super::json_namespace::{inject_spectty_hooks, retract_spectty_hooks, HookCommandEntry};
use super::scope::settings_path_for_scope;

/// A [`ProvisioningPort`] that manages the `hooks` section of `settings.json`.
///
/// Generic over the [`ConfigFile`] seam so tests inject an in-memory fake.
pub struct ClaudeSettingsProvisioner<F: ConfigFile> {
    files: F,
    /// The resolved absolute path to the `spectty-hook` binary. Used as the
    /// owned-row identity: retract removes only rows whose `hooks[].command`
    /// equals this path (R7 generalized).
    hook_command: String,
    /// The hook events to inject, with optional matchers.
    /// Each tuple: (event_name, entry, matcher).
    events: Vec<(String, HookCommandEntry, Option<String>)>,
}

impl<F: ConfigFile> ClaudeSettingsProvisioner<F> {
    /// Build a settings provisioner.
    ///
    /// - `files`: the injectable file seam (use [`RealConfigFile`](super::file_io::RealConfigFile) in production).
    /// - `hook_command`: resolved absolute path to the `spectty-hook` binary.
    /// - `events`: hook events to inject (event_name, entry, optional matcher).
    pub fn new(
        files: F,
        hook_command: String,
        events: Vec<(String, HookCommandEntry, Option<String>)>,
    ) -> Self {
        Self {
            files,
            hook_command,
            events,
        }
    }
}

impl<F: ConfigFile> ProvisioningPort for ClaudeSettingsProvisioner<F> {
    fn inject(&self, scope: ProvisioningScope) -> Result<ProvisioningHandle, ProvisioningError> {
        let path = settings_path_for_scope(&scope);
        // An absent config starts as an empty JSON object; inject_spectty_hooks
        // creates the `hooks` key for us.
        let current = self
            .files
            .read(&path)
            .map_err(|e| ProvisioningError::Io(e.to_string()))?
            .unwrap_or_else(|| "{}".to_string());

        let next = inject_spectty_hooks(&current, &self.events)?;

        self.files
            .write_atomic(&path, &next)
            .map_err(|e| ProvisioningError::Io(e.to_string()))?;

        Ok(ProvisioningHandle { scope })
    }

    fn retract(&self, handle: &ProvisioningHandle) -> Result<(), ProvisioningError> {
        let path = settings_path_for_scope(&handle.scope);
        // Retracting an absent file is a no-op (idempotent close).
        let Some(current) = self
            .files
            .read(&path)
            .map_err(|e| ProvisioningError::Io(e.to_string()))?
        else {
            return Ok(());
        };

        let next = retract_spectty_hooks(&current, &self.hook_command)?;

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

    /// Build a minimal provisioner for test: injects `Stop` and `UserPromptSubmit`
    /// without matcher; uses the fictitious binary path.
    fn provisioner_with_fake(fake: FakeConfigFile) -> ClaudeSettingsProvisioner<FakeConfigFile> {
        let hook_cmd = "/usr/local/bin/spectty-hook".to_string();
        let events = vec![
            (
                "Stop".to_string(),
                HookCommandEntry {
                    command: hook_cmd.clone(),
                    args: vec!["--event".to_string(), "Stop".to_string()],
                },
                None,
            ),
            (
                "UserPromptSubmit".to_string(),
                HookCommandEntry {
                    command: hook_cmd.clone(),
                    args: vec!["--event".to_string(), "Submit".to_string()],
                },
                None,
            ),
        ];
        ClaudeSettingsProvisioner::new(fake, hook_cmd, events)
    }

    #[test]
    fn inject_global_writes_hooks_to_settings_json() {
        // Global scope → ~/.claude/settings.json
        let path = "~/.claude/settings.json";
        let p = provisioner_with_fake(FakeConfigFile::with_file(path, "{}"));

        let handle = p.inject(ProvisioningScope::Global).expect("inject ok");
        assert_eq!(
            handle.scope,
            ProvisioningScope::Global,
            "handle carries scope"
        );

        let current = p.files.read(path).expect("read").expect("present");
        let parsed: serde_json::Value =
            serde_json::from_str(&current).expect("valid JSON after inject");

        assert!(
            parsed["hooks"]["Stop"].as_array().is_some(),
            "Stop hooks injected"
        );
        assert!(
            parsed["hooks"]["UserPromptSubmit"].as_array().is_some(),
            "UserPromptSubmit hooks injected"
        );
    }

    #[test]
    fn inject_project_targets_dot_claude_settings_json_at_repo_root() {
        let p = provisioner_with_fake(FakeConfigFile::default());

        p.inject(ProvisioningScope::Project("/repo".to_string()))
            .expect("inject project ok");

        let written = p
            .files
            .read("/repo/.claude/settings.json")
            .expect("read")
            .expect("project file written");
        let parsed: serde_json::Value = serde_json::from_str(&written).expect("valid JSON");
        assert!(
            parsed["hooks"]["Stop"].as_array().is_some(),
            "project settings.json has injected hooks"
        );
    }

    #[test]
    fn inject_backs_up_original_before_first_write() {
        let path = "~/.claude/settings.json";
        let p = provisioner_with_fake(FakeConfigFile::with_file(path, r#"{"model":"claude"}"#));

        p.inject(ProvisioningScope::Global).expect("inject");

        let backup = p
            .files
            .read(&format!("{path}.spectty.bak"))
            .expect("read bak")
            .expect("backup present");
        assert_eq!(
            backup, r#"{"model":"claude"}"#,
            "original settings.json backed up before first write"
        );
    }

    #[test]
    fn inject_does_not_touch_foreign_top_level_keys() {
        // model, permissions, env must survive inject.
        let path = "~/.claude/settings.json";
        let original = serde_json::to_string_pretty(&serde_json::json!({
            "model": "claude-opus-4-5",
            "permissions": { "allow": ["Bash"] }
        }))
        .expect("fixture");
        let p = provisioner_with_fake(FakeConfigFile::with_file(path, &original));

        p.inject(ProvisioningScope::Global).expect("inject");

        let current = p.files.read(path).expect("read").expect("present");
        let parsed: serde_json::Value = serde_json::from_str(&current).expect("valid JSON");
        assert_eq!(parsed["model"], "claude-opus-4-5", "model preserved");
        assert!(
            parsed["permissions"]["allow"].as_array().is_some(),
            "permissions preserved"
        );
    }

    #[test]
    fn retract_removes_spectty_hooks_and_leaves_foreign_hooks() {
        let path = "~/.claude/settings.json";
        let hook_cmd = "/usr/local/bin/spectty-hook";
        // Pre-existing settings.json with a foreign user hook on Stop.
        let original = serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "Stop": [
                    {
                        "hooks": [
                            { "type": "command", "command": "/usr/local/bin/user-notify", "args": [] }
                        ]
                    }
                ]
            }
        }))
        .expect("fixture");
        let p = provisioner_with_fake(FakeConfigFile::with_file(path, &original));

        let handle = p.inject(ProvisioningScope::Global).expect("inject");
        p.retract(&handle).expect("retract");

        let current = p.files.read(path).expect("read").expect("present");
        let parsed: serde_json::Value = serde_json::from_str(&current).expect("valid JSON");

        // Spectty row is gone.
        let spectty_present = parsed["hooks"]["Stop"]
            .as_array()
            .map(|arr| {
                arr.iter().any(|el| {
                    el.get("hooks")
                        .and_then(serde_json::Value::as_array)
                        .map(|h| h.iter().any(|inner| inner["command"] == hook_cmd))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        assert!(!spectty_present, "Spectty row must be gone after retract");

        // Foreign user hook survives.
        let foreign_present = parsed["hooks"]["Stop"]
            .as_array()
            .map(|arr| {
                arr.iter().any(|el| {
                    el.get("hooks")
                        .and_then(serde_json::Value::as_array)
                        .map(|h| {
                            h.iter()
                                .any(|inner| inner["command"] == "/usr/local/bin/user-notify")
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        assert!(
            foreign_present,
            "foreign user hook on Stop must survive retract"
        );
    }

    #[test]
    fn retract_absent_file_is_ok() {
        // No settings.json at all: retract must be Ok (idempotent close).
        let p = provisioner_with_fake(FakeConfigFile::default());
        let handle = ProvisioningHandle {
            scope: ProvisioningScope::Global,
        };
        p.retract(&handle)
            .expect("retract on absent settings.json is Ok");
    }

    #[test]
    fn inject_then_retract_leaves_empty_hooks_array_for_managed_events() {
        // After retract, the Stop array still exists but has zero Spectty rows.
        // (Foreign-only events keep their arrays; Spectty-only events are empty arrays.)
        let path = "~/.claude/settings.json";
        let p = provisioner_with_fake(FakeConfigFile::with_file(path, "{}"));

        let handle = p.inject(ProvisioningScope::Global).expect("inject");
        p.retract(&handle).expect("retract");

        let current = p.files.read(path).expect("read").expect("present");
        let parsed: serde_json::Value = serde_json::from_str(&current).expect("valid JSON");

        // The Stop array may be present but must have zero Spectty rows.
        if let Some(stop_arr) = parsed["hooks"]["Stop"].as_array() {
            let has_spectty = stop_arr.iter().any(|el| {
                el.get("hooks")
                    .and_then(serde_json::Value::as_array)
                    .map(|h| {
                        h.iter()
                            .any(|inner| inner["command"] == "/usr/local/bin/spectty-hook")
                    })
                    .unwrap_or(false)
            });
            assert!(!has_spectty, "no Spectty row in Stop array after retract");
        }
        // If Stop key is absent entirely that's also fine (empty array removed).
    }
}
