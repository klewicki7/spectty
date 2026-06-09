# Capability: hexagonal-core

> M2 delta spec on the archived `hexagonal-core` baseline (M0 + M1). RFC 2119 keywords are
> normative. Full prose + verification-class tags live in the change-level `spec.md`.

M2 INTENTIONALLY adds domain types to Core that the M1 guard deferred — `AgentRunner`,
`ProvisioningPort`, `OutputSignal`, `AgentSpec`, the pure `transition` function, the grown
`Session`, and `SessionRegistry` — while keeping the Core dependency set (`serde` +
`thiserror` only) and the agent-agnostic invariant. This `MODIFIED` requirement supersedes,
for M2, the M1 guard clause that forbade defining an `OutputSignal` type in Core.

## MODIFIED Requirements

### Requirement: Core grows the agent domain WITHOUT new dependencies and WITHOUT agent names
M2 MUST add `AgentRunner`, `ProvisioningPort`, `OutputSignal`, `AgentSpec`/`AgentTier`/
`AgentDescriptor`, `LaunchSpec`, the pure `transition` function, the grown `Session`, and
`SessionRegistry` to `spectty-core` with ZERO new dependencies — the Core MUST remain `serde`
+ `thiserror` only (no `tokio`, `tauri`, `portable-pty`, time crate, or agent/tool crate; the
`ClockPort`-style time seam is a Core TRAIT whose concrete clock lives outside Core). The Core
MUST contain NO agent-name literal and NO config-format or ANSI/regex knowledge. The
core-scoped `cargo-deny` gate MUST stay green. This SUPERSEDES the M1 guard clause forbidding
an `OutputSignal` type in Core (that clause was M1-scoped; M2 deliberately introduces
`OutputSignal` as a Core port type).

#### Scenario: Core manifest still lists only serde + thiserror after M2
- **Given** the `spectty-core` `Cargo.toml` after M2
- **When** its dependency list is inspected
- **Then** it MUST remain limited to `serde` + `thiserror`, with NO `tokio`, `tauri`,
  `portable-pty`, time crate, or agent/tool crate added

#### Scenario: No agent name appears anywhere in the Core
- **Given** the `spectty-core` source after M2
- **When** it is scanned for agent-name literals
- **Then** there MUST be no `"claude"`, no `"bash"`, and no `if agent == …` branch anywhere in
  Core — all agent-specific logic MUST live in `crates/adapters` / `src-tauri`

#### Scenario: core-scoped cargo-deny stays green and clippy is clean
- **Given** the M2 changes applied (runners + provisioner + producer in adapters/src-tauri,
  Core grown but quarantined)
- **When** the core-scoped `cargo-deny` boundary gate and `clippy -D warnings` run in CI
- **Then** `cargo-deny` MUST exit 0 with no forbidden-dependency findings AND clippy MUST
  report no warnings AND `cargo build` MUST succeed
