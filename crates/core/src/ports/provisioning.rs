//! The `ProvisioningPort` — a Core port for injecting/retracting the Spectty
//! Agent Protocol's Layer-1 MCP-tool registration in a cooperative agent's config.
//!
//! This port is SEPARATE from [`AgentRunner`](super::agent_runner::AgentRunner)
//! (Lock 1 / D7): provisioning is a session-lifecycle concern (inject-on-create /
//! retract-on-close) with a different cadence than per-output `detect_status`, so
//! coupling them in one trait would force the Generic runner — which needs no
//! injection — to carry the seam. The composition root decides whether to call
//! [`inject`](ProvisioningPort::inject) / [`retract`](ProvisioningPort::retract)
//! by reading [`AgentCapabilities::requires_provisioning`](crate::AgentCapabilities),
//! not via a trait method.
//!
//! The Core stays abstract: NO agent name, NO config path, NO JSON/serde knowledge
//! lives here. The concrete JSON managed-namespace editing + atomic file-IO live in
//! the adapter layer (`spectty-adapters::provision`). This keeps the Core quarantine
//! intact — adding this port introduces ZERO new Core dependencies.

use thiserror::Error;

/// A failure while injecting or retracting the managed MCP registration.
#[derive(Debug, Error)]
pub enum ProvisioningError {
    /// The underlying config file could not be read or written.
    #[error("provisioning io error: {0}")]
    Io(String),
    /// The existing config could not be parsed (or re-serialized) as valid JSON.
    #[error("config parse error: {0}")]
    Parse(String),
}

/// Where to inject the Spectty Agent Protocol registration for an agent.
///
/// A `String` (not `PathBuf`) mirrors the rest of the Core's String-path convention
/// and avoids `OsString` serde edge cases — the Core never touches `std::path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisioningScope {
    /// User/global scope: `~/.claude.json` top-level `mcpServers`.
    Global,
    /// Project scope: `<repo_root>/.mcp.json`. Carries the repo root.
    Project(String),
}

/// Opaque handle returned by [`inject`](ProvisioningPort::inject).
///
/// The Session carries this so [`retract`](ProvisioningPort::retract) can target the
/// EXACT scope that was injected, without having to re-resolve scope at teardown
/// (the git-tracked state could have changed between create and close).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisioningHandle {
    /// The scope the registration was injected into.
    pub scope: ProvisioningScope,
}

/// Core port for injecting / retracting the Spectty Agent Protocol registration.
///
/// SEPARATE from [`AgentRunner`](super::agent_runner::AgentRunner) (Lock 1). M2 ships
/// Layer-1 MCP-tool registration + teardown only (Lock 3); `refresh()` (Layer-2
/// dynamics) is deliberately OMITTED until M3 and is additive when it lands.
///
/// `&self` interior mutability + `Send + Sync`, exactly like
/// [`PersistencePort`](super::persistence::PersistencePort), so a single provisioner
/// shares across command handlers as `tauri::State` behind `Arc`.
pub trait ProvisioningPort: Send + Sync {
    /// Inject the managed `spectty_*` MCP server entry at `scope`, idempotently
    /// (a second inject over an already-injected config is a no-op write of the
    /// same content). Returns the handle to retract with later.
    fn inject(&self, scope: ProvisioningScope) -> Result<ProvisioningHandle, ProvisioningError>;

    /// Remove the managed `spectty_*` keys at the handle's scope on session close.
    /// Idempotent: retracting an already-clean (or absent) config is `Ok(())`.
    fn retract(&self, handle: &ProvisioningHandle) -> Result<(), ProvisioningError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal recording fake proving the port is object-safe and that `inject`
    /// returns a handle carrying the scope it was given, which `retract` consumes —
    /// the M2 spec's "inject + retract present, distinct from AgentRunner" shape.
    #[derive(Default)]
    struct RecordingProvisioner {
        injected: std::sync::Mutex<Vec<ProvisioningScope>>,
        retracted: std::sync::Mutex<Vec<ProvisioningScope>>,
    }

    impl ProvisioningPort for RecordingProvisioner {
        fn inject(
            &self,
            scope: ProvisioningScope,
        ) -> Result<ProvisioningHandle, ProvisioningError> {
            self.injected.lock().expect("lock").push(scope.clone());
            Ok(ProvisioningHandle { scope })
        }

        fn retract(&self, handle: &ProvisioningHandle) -> Result<(), ProvisioningError> {
            self.retracted
                .lock()
                .expect("lock")
                .push(handle.scope.clone());
            Ok(())
        }
    }

    #[test]
    fn inject_returns_handle_carrying_scope_then_retract_consumes_it() {
        let port: &dyn ProvisioningPort = &RecordingProvisioner::default();

        let handle = port
            .inject(ProvisioningScope::Project("/repo".to_string()))
            .expect("inject ok");
        assert_eq!(
            handle.scope,
            ProvisioningScope::Project("/repo".to_string()),
            "handle carries the exact scope injected, so retract targets it"
        );

        port.retract(&handle).expect("retract ok");
    }

    #[test]
    fn global_scope_default_is_distinct_from_project() {
        assert_ne!(
            ProvisioningScope::Global,
            ProvisioningScope::Project("/repo".to_string()),
            "Global and Project are distinct scopes"
        );
    }
}
