//! `provision` — the M2 [`ProvisioningPort`](spectty_core::ProvisioningPort) adapter.
//!
//! This module owns ALL the agent-config knowledge the Core deliberately lacks:
//! the JSON managed-namespace editing, scope resolution, and atomic file-IO. The
//! split mirrors the hexagon — the pure, table-tested editor + scope resolver are
//! isolated from the impure filesystem shell behind the [`ConfigFile`] seam.
//!
//! Layout:
//! - [`json_namespace`]: PURE `String -> String` editor that owns ONLY `spectty_*`
//!   keys under `mcpServers`, preserving every foreign key on round-trip (R7).
//! - [`scope`]: pure scope resolution over an injected `is_git_tracked` predicate
//!   (D18), plus the real git probe kept separate from the pure resolver.
//! - [`file_io`]: the [`ConfigFile`] atomic-write seam (tmp + fsync + rename +
//!   `.spectty.bak`) with a real impl and an in-memory test fake.
//! - [`claude_provisioner`]: [`ClaudeJsonProvisioner`] composing the three above
//!   into a [`ProvisioningPort`](spectty_core::ProvisioningPort) impl.

pub mod claude_provisioner;
pub mod file_io;
pub mod json_namespace;
pub mod scope;
pub mod settings_provisioner;

pub use claude_provisioner::ClaudeJsonProvisioner;
pub use file_io::{ConfigFile, RealConfigFile};
pub use json_namespace::{
    inject_spectty_hooks, inject_spectty_mcp, retract_spectty_hooks, retract_spectty_mcp,
    HookCommandEntry, McpServerEntry,
};
pub use scope::{is_git_tracked, resolve_scope, settings_path_for_scope};
pub use settings_provisioner::ClaudeSettingsProvisioner;
