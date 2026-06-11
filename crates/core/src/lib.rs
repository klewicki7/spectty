//! `spectty-core` — the technology-agnostic hexagonal core for Spectty.
//!
//! M0 scope: behaviorless placeholder entities ([`Session`], [`Workspace`],
//! [`AgentStatus`]) plus the single behavior-bearing [`PersistencePort`] contract.
//!
//! M2 grows the supervision domain: value types ([`AgentSpec`], [`AgentDescriptor`],
//! [`OutputSignal`]) and the [`ClockPort`] time seam yielding a serde-safe
//! [`Timestamp`]. These remain Core-pure (serde + thiserror only) — no agent names,
//! no ANSI/config knowledge, no `Instant` crossing the boundary.
//!
//! This crate depends INWARD ONLY (serde + thiserror). It must never reference
//! adapters, the tauri bridge, an engram client, tokio, or any external
//! agent/tool crate — the dependency graph enforces this at compile time.

pub mod entities;
pub mod ports;

pub use entities::{
    transition, AgentCapabilities, AgentDescriptor, AgentKind, AgentSpec, AgentStatus, AgentTier,
    ApprovalState, CostDelta, Observed, OutputSignal, QuickAction, Session, SessionId,
    SessionRegistry, SessionSummary, SpecContract, SpecError, SpecTask, TaskProgress, TaskState,
    Workspace, WorkspaceId,
};
pub use ports::{
    AgentRunner, ClockPort, LaunchContext, LaunchSpec, PersistenceError, PersistencePort,
    ProvisioningError, ProvisioningHandle, ProvisioningPort, ProvisioningScope, Timestamp,
};
