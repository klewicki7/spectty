//! `spectty-core` — the technology-agnostic hexagonal core for Spectty.
//!
//! M0 scope: behaviorless placeholder entities ([`Session`], [`Workspace`],
//! [`AgentStatus`]) plus the single behavior-bearing [`PersistencePort`] contract.
//!
//! This crate depends INWARD ONLY (serde + thiserror). It must never reference
//! adapters, the tauri bridge, an engram client, tokio, or any external
//! agent/tool crate — the dependency graph enforces this at compile time.

pub mod entities;
pub mod ports;

pub use entities::{AgentStatus, Session, SessionId, Workspace, WorkspaceId};
pub use ports::{PersistenceError, PersistencePort};
