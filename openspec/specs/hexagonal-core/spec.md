# Capability: hexagonal-core

> Living baseline spec. Established by change `M0-scaffold` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

`spectty-core` is the domain center. It MUST depend only inward — on nothing from
adapters, the Tauri bridge, engram, or any external agent/tool crate. This is the
engram quarantine, enforced mechanically from day one.

## Requirement: Core domain placeholder types exist
`spectty-core` MUST define `Session`, `Workspace`, and `AgentStatus` as behaviorless
placeholder types. They carry no business logic in M0 (state machines and behavior are
deferred to M2).

### Scenario: Core exposes the three placeholder types
- **Given** the `spectty-core` crate compiled
- **When** the public API is inspected
- **Then** `Session`, `Workspace`, and `AgentStatus` MUST each be present as a defined type with no domain behavior attached

## Requirement: Core depends inward only (engram quarantine)
`spectty-core` MUST NOT declare or transitively require any dependency on
`spectty-adapters`, the `src-tauri` bridge crate, the engram client, the tauri crate, or
any external agent/tool crate. The Cargo dependency graph is the PRIMARY enforcement gate:
because Core lists none of these dependencies, the compiler rejects any inward-violating
import. `cargo-deny` in CI is the belt-and-suspenders secondary gate.

### Scenario: Core manifest lists no outward dependencies
- **Given** the `spectty-core` `Cargo.toml`
- **When** its dependency list is inspected
- **Then** it MUST NOT include `spectty-adapters`, `src-tauri`/the bridge crate, `tauri`, the engram client, or any external agent/tool crate

### Scenario: A boundary violation MUST fail the build and CI (negative/guard)
- **Given** a hypothetical change that adds a dependency from `spectty-core` onto `spectty-adapters` (or onto tauri/engram)
- **When** `cargo build` and the CI `cargo-deny` gate run
- **Then** the build MUST fail (compiler rejects the inward violation) AND the `cargo-deny` deny-list check MUST report the forbidden dependency, so the violation cannot merge

### Scenario: cargo-deny boundary gate passes on the clean scaffold
- **Given** the compliant scaffold with no boundary violations
- **When** the `cargo-deny` boundary/deny-list check runs in CI
- **Then** it MUST exit 0 with no forbidden-dependency findings

## Requirement: Core gains no PTY or runtime dependency as the system grows
> Added by change `M1-live-pty-terminal` (archived 2026-06-08) as a guard delta on the
> baseline quarantine: M1 added PTY machinery to adapters and the bridge while leaving the
> Core untouched.
>
> SUPERSEDED IN PART by change `M2-spawn-agent-provisioner` (archived 2026-06-08): the
> M1-scoped clause forbidding an `OutputSignal` type in Core no longer applies — M2
> deliberately introduces `OutputSignal` (and the wider agent domain) as Core port types.
> See "Core grows the agent domain WITHOUT new dependencies and WITHOUT agent names" below.
> The dependency-set invariant (`serde` + `thiserror` only) and the `PtyPort`-absence clause
> remain in force.

As new capabilities (PTY, terminal, etc.) are added, `spectty-core` MUST NOT gain any new
dependency. In particular it MUST NOT depend on `portable-pty`, `tokio`, `tauri`, or any
agent/tool crate, and MUST NOT define a `PtyPort` trait. Such crates MUST land only in
`crates/adapters` and/or `src-tauri`. The Core MUST remain `serde` + `thiserror` only, and
the core-scoped `cargo-deny` gate MUST stay green.

### Scenario: Core manifest still lists no PTY or runtime crate
- **Given** the `spectty-core` `Cargo.toml` after a capability that introduces a PTY/runtime
- **When** its dependency list is inspected
- **Then** it MUST NOT include `portable-pty`, `tokio`, `tauri`, or any agent/tool crate,
  AND it MUST remain limited to `serde` + `thiserror`

### Scenario: core-scoped cargo-deny stays green after the PTY capability lands
- **Given** the PTY machinery added to adapters / src-tauri with the Core untouched
- **When** the core-scoped `cargo-deny` boundary gate runs in CI
- **Then** it MUST exit 0 with no forbidden-dependency findings AND `cargo build` MUST
  succeed

## Requirement: Core grows the agent domain WITHOUT new dependencies and WITHOUT agent names
> Added by change `M2-spawn-agent-provisioner` (archived 2026-06-08). This requirement
> SUPERSEDES, for M2 and beyond, the M1 guard clause forbidding an `OutputSignal` type in
> Core: M2 deliberately introduces `OutputSignal` as a Core port type. The dependency-set
> invariant (`serde` + `thiserror` only) and the agent-agnostic invariant remain in force.

M2 MUST add `AgentRunner`, `ProvisioningPort`, `OutputSignal`, `AgentSpec`/`AgentTier`/
`AgentDescriptor`, `LaunchSpec`, the pure `transition` function, the grown `Session`, and
`SessionRegistry` to `spectty-core` with ZERO new dependencies — the Core MUST remain `serde`
+ `thiserror` only (no `tokio`, `tauri`, `portable-pty`, time crate, or agent/tool crate; the
`ClockPort`-style time seam is a Core TRAIT whose concrete clock lives outside Core). The Core
MUST contain NO agent-name literal and NO config-format or ANSI/regex knowledge. The
core-scoped `cargo-deny` gate MUST stay green.

### Scenario: Core manifest still lists only serde + thiserror after M2
- **Given** the `spectty-core` `Cargo.toml` after M2
- **When** its dependency list is inspected
- **Then** it MUST remain limited to `serde` + `thiserror`, with NO `tokio`, `tauri`,
  `portable-pty`, time crate, or agent/tool crate added

### Scenario: No agent name appears anywhere in the Core
- **Given** the `spectty-core` source after M2
- **When** it is scanned for agent-name literals
- **Then** there MUST be no `"claude"`, no `"bash"`, and no `if agent == …` branch anywhere in
  Core — all agent-specific logic MUST live in `crates/adapters` / `src-tauri`

### Scenario: core-scoped cargo-deny stays green and clippy is clean
- **Given** the M2 changes applied (runners + provisioner + producer in adapters/src-tauri,
  Core grown but quarantined)
- **When** the core-scoped `cargo-deny` boundary gate and `clippy -D warnings` run in CI
- **Then** `cargo-deny` MUST exit 0 with no forbidden-dependency findings AND clippy MUST
  report no warnings AND `cargo build` MUST succeed
