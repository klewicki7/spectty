# Capability: ci-pipeline

> Living baseline spec. Established by change `M0-scaffold` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

CI MUST enforce build, formatting, linting, tests, and the boundary gate on every change,
running on a macOS runner.

## Requirement: CI runs and passes all Rust and JS gates
CI MUST run on `macos-latest` and MUST pass: `cargo build`, `cargo fmt --check`,
`cargo clippy -D warnings`, `cargo test --workspace`, and `pnpm test` (Vitest). The
`cargo-deny` boundary gate (per hexagonal-core) MUST also pass.

### Scenario: Clean scaffold turns CI green
- **Given** the compliant scaffold pushed to a branch
- **When** CI runs on the macOS runner
- **Then** `cargo build`, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, `pnpm test`, and the `cargo-deny` boundary gate MUST all exit 0

### Scenario: A clippy warning fails CI (negative/guard)
- **Given** a change that introduces a clippy lint warning
- **When** CI runs `cargo clippy -D warnings`
- **Then** the step MUST fail (warnings promoted to errors) and block the merge

### Scenario: Unformatted code fails CI (negative/guard)
- **Given** a change that leaves code not matching `cargo fmt`
- **When** CI runs `cargo fmt --check`
- **Then** the step MUST fail and block the merge

## Requirement: Vitest harness proves the UI test path with mocked Tauri
At least one Vitest test MUST mock `@tauri-apps/api` and assert the UI's
`invoke("ping")` call and `listen("pong")` wiring. This proves the front-end test
harness works without a running backend.

### Scenario: Vitest test passes against a mocked Tauri API
- **Given** a Vitest test that mocks `@tauri-apps/api`
- **When** `pnpm test` runs
- **Then** the test MUST assert that the UI invokes `ping` and registers a `pong` listener, and MUST pass without a running Tauri backend
