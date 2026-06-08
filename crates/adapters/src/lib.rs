//! `spectty-adapters` — concrete implementations of the `spectty-core` ports.
//!
//! M0 ships two [`spectty_core::ports::PersistencePort`] implementations:
//! - [`InMemoryPersistenceAdapter`]: a real, dependency-free adapter used to
//!   prove the persistence round-trip.
//! - [`EngramAdapter`]: a `todo!()` skeleton proving the adapter shape; real
//!   network transport is deferred to M3.

pub mod agent;
pub mod persistence;
pub mod pty;

pub use agent::{AgentRunnerRegistry, ClaudeCodeRunner, GenericRunner, OutputSignalProducer};
pub use persistence::{EngramAdapter, InMemoryPersistenceAdapter};
pub use pty::{Coalescer, PtyAdapter, PtyError, PtySpawnConfig, PtyTransport};
