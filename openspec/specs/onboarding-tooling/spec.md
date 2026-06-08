# Capability: onboarding-tooling

> Living baseline spec. Established by change `M0-scaffold` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

A new contributor MUST be able to go from clean clone to a running `ping → pong` quickly,
guided by documented conventions.

## Requirement: Documented dev workflow with hot reload
The repository MUST document the canonical dev workflow, including the `pnpm tauri dev`
hot-reload entry point and hot-reload tooling (cargo-watch). The docs MUST identify
`pnpm tauri dev` as the single canonical dev entry.

### Scenario: Docs describe the canonical dev entry and hot reload
- **Given** the project conventions/onboarding documentation
- **When** a new contributor reads it
- **Then** it MUST name `pnpm tauri dev` as the canonical dev entry and MUST describe hot-reload tooling (cargo-watch) for Rust changes

## Requirement: New contributor onboards under 30 minutes
A new contributor following the documented workflow MUST be able to clone, build, and
observe `ping → pong` in under 30 minutes on a supported macOS machine. The docs SHOULD
note that the first clean build may take several minutes (sccache is used in CI to mitigate).

### Scenario: Clean-clone to ping→pong within the onboarding budget
- **Given** a new contributor on a supported macOS machine with prerequisites (Rust 1.89, pnpm, Node) installed
- **When** they follow the documented steps from clean clone through `pnpm tauri dev`
- **Then** they MUST reach a running app showing `pong` in the web console in under 30 minutes total
