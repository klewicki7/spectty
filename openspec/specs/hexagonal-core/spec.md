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
