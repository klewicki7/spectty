//! `spectty-adapters` — concrete implementations of the `spectty-core` ports.
//!
//! M0 ships two [`spectty_core::ports::PersistencePort`] implementations:
//! - [`InMemoryPersistenceAdapter`]: a real, dependency-free adapter used to
//!   prove the persistence round-trip.
//! - [`EngramAdapter`]: a `todo!()` skeleton proving the adapter shape; real
//!   network transport is deferred to M3.

pub mod agent;
pub mod clock;
pub mod diff;
pub mod file_watch;
pub mod git;
pub mod hook;
pub mod persistence;
pub mod provision;
pub mod pty;

pub use agent::{AgentRunnerRegistry, ClaudeCodeRunner, GenericRunner, OutputSignalProducer};
pub use clock::SystemClock;
pub use diff::vibelens::{
    build_explanation, changed_files, McpStdio, RealMcpStdio, VibeLensMcpAdapter,
};
pub use file_watch::{Debouncer, NotifyFileWatcher, NotifyWatchGuard};
pub use git::GitCliAdapter;
pub use hook::{
    event_to_observed, parse_state_file, HookEvent, HookState, StateFileReader,
    PERMISSION_PROMPT_MATCHER,
};
pub use persistence::{EngramAdapter, InMemoryPersistenceAdapter};
pub use provision::{
    inject_spectty_hooks, inject_spectty_mcp, is_git_tracked, resolve_scope, retract_spectty_hooks,
    retract_spectty_mcp, settings_path_for_scope, ClaudeJsonProvisioner, ClaudeSettingsProvisioner,
    ConfigFile, HookCommandEntry, McpServerEntry, RealConfigFile,
};
pub use pty::{Coalescer, PtyAdapter, PtyError, PtySpawnConfig, PtyTransport};
