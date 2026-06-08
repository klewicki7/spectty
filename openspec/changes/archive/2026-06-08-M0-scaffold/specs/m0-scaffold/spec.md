# Spec Delta: M0 — Scaffold + Engram Wiring

> Source: `sdd/M0-scaffold/proposal`. This delta specifies WHAT MUST be true after
> M0 is applied. It does NOT prescribe implementation. RFC 2119 keywords
> (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.
>
> Scope guard: M0 covers scaffold + the engram quarantine + ping/pong + CI gates only.
> PTY, xterm.js, AgentRunner, AgentStatus state machine, SessionRegistry, real engram
> HTTP/polling, GitPort, and NotifierPort are DEFERRED (M1–M5 per proposal "Scope — Out").
> Any scenario referencing those concerns is out of scope for this delta.
>
> ARCHIVED 2026-06-08: these ADDED capabilities were promoted into the living baseline
> specs at `openspec/specs/{capability}/spec.md`. This delta is kept as the historical
> record of what M0 introduced.

---

## ADDED Capability: monorepo-scaffold

A single repository MUST host a Cargo workspace (Rust) and a pnpm workspace (JS/TS)
that coexist and both build from a clean clone.

### Requirement: Cargo workspace builds from a clean clone
The repository MUST define a root Cargo workspace including `spectty-core`,
`spectty-adapters`, and the `src-tauri` bridge crate. `cargo build` MUST succeed on a
clean clone with no manual setup beyond installing the pinned toolchain.

#### Scenario: Clean clone compiles the Rust workspace
- **Given** a fresh clone of the repository on macOS with the pinned Rust toolchain installed
- **When** a contributor runs `cargo build` at the repository root
- **Then** the command MUST exit 0 and produce build artifacts for `spectty-core`, `spectty-adapters`, and the `src-tauri` crate

#### Scenario: Toolchain is pinned, not floating
- **Given** a `rust-toolchain.toml` pinning Rust 1.89 at the repository root
- **When** a contributor builds without an explicitly selected toolchain
- **Then** the build MUST use the pinned 1.89 toolchain rather than the machine default

### Requirement: pnpm workspace installs and runs the dev app
The repository MUST define a pnpm workspace (`pnpm-workspace.yaml`, root `package.json`)
including the `ui/` package. `pnpm install` followed by the canonical dev entry MUST
launch the running Tauri + React app.

#### Scenario: Clean clone installs and starts the dev app
- **Given** a fresh clone with pnpm and Node available
- **When** a contributor runs `pnpm install` and then `pnpm tauri dev`
- **Then** the app window MUST launch with the React 19 + Vite frontend served and the Tauri shell running

#### Scenario: Cargo and pnpm workspaces coexist without collision
- **Given** both the Cargo workspace and the pnpm workspace defined at the repository root
- **When** a contributor runs `cargo build` and `pnpm install` in either order
- **Then** neither command MUST corrupt or invalidate the other's lockfile, target directory, or node_modules

---

## ADDED Capability: hexagonal-core

`spectty-core` is the domain center. It MUST depend only inward — on nothing from
adapters, the Tauri bridge, engram, or any external agent/tool crate. This is the
engram quarantine, enforced mechanically from day one.

### Requirement: Core domain placeholder types exist
`spectty-core` MUST define `Session`, `Workspace`, and `AgentStatus` as behaviorless
placeholder types. They carry no business logic in M0 (state machines and behavior are
deferred to M2 per the proposal).

#### Scenario: Core exposes the three placeholder types
- **Given** the `spectty-core` crate compiled
- **When** the public API is inspected
- **Then** `Session`, `Workspace`, and `AgentStatus` MUST each be present as a defined type with no domain behavior attached

### Requirement: Core depends inward only (engram quarantine)
`spectty-core` MUST NOT declare or transitively require any dependency on
`spectty-adapters`, the `src-tauri` bridge crate, the engram client, the tauri crate, or
any external agent/tool crate. The Cargo dependency graph is the PRIMARY enforcement gate:
because Core lists none of these dependencies, the compiler rejects any inward-violating
import. `cargo-deny` in CI is the belt-and-suspenders secondary gate.

#### Scenario: Core manifest lists no outward dependencies
- **Given** the `spectty-core` `Cargo.toml`
- **When** its dependency list is inspected
- **Then** it MUST NOT include `spectty-adapters`, `src-tauri`/the bridge crate, `tauri`, the engram client, or any external agent/tool crate

#### Scenario: A boundary violation MUST fail the build and CI (negative/guard)
- **Given** a hypothetical change that adds a dependency from `spectty-core` onto `spectty-adapters` (or onto tauri/engram)
- **When** `cargo build` and the CI `cargo-deny` gate run
- **Then** the build MUST fail (compiler rejects the inward violation) AND the `cargo-deny` deny-list check MUST report the forbidden dependency, so the violation cannot merge

#### Scenario: cargo-deny boundary gate passes on the clean scaffold
- **Given** the compliant M0 scaffold with no boundary violations
- **When** the `cargo-deny` boundary/deny-list check runs in CI
- **Then** it MUST exit 0 with no forbidden-dependency findings

---

## ADDED Capability: persistence-port

The persistence contract MUST live in Core as a port. Adapters implement it. Engram is the
first adapter, present as a skeleton only in M0 (no daemon, no network).

### Requirement: PersistencePort contract defined in Core
`spectty-core` MUST define a `PersistencePort` trait exposing `write(key, value)` and
`read(key)` operations only. The trait MUST NOT reference engram, HTTP, tauri, or any
adapter type — it is a pure contract.

#### Scenario: Port exposes write and read only
- **Given** the `PersistencePort` trait in `spectty-core`
- **When** its method set is inspected
- **Then** it MUST expose a `write(key, value)` operation and a `read(key)` operation, and MUST NOT expose engram/HTTP/adapter-specific methods

### Requirement: In-memory stub adapter satisfies the contract
An in-memory stub adapter MUST implement `PersistencePort` and MUST be usable in unit
tests with no external dependencies. A unit test MUST perform a `write` → `read`
round-trip and assert the stored value round-trips unchanged.

#### Scenario: write → read round-trip returns the stored value
- **Given** an in-memory stub adapter implementing `PersistencePort`
- **When** a test writes value `V` under key `K`, then reads key `K`
- **Then** the read MUST return value `V` unchanged

#### Scenario: read of a missing key returns empty (negative/guard)
- **Given** an in-memory stub adapter with no value stored under key `K`
- **When** a test reads key `K`
- **Then** the read MUST return the empty/absent result (e.g. `None`) and MUST NOT error or return a stale value

### Requirement: EngramAdapter skeleton proves the adapter shape
`spectty-adapters` MUST contain an `EngramAdapter` that implements `PersistencePort` with
method bodies as `todo!()`. It proves the adapter shape without any network call. No engram
daemon is required for M0 exit. Real engram HTTP and polling/subscribe are DEFERRED to M3.

#### Scenario: EngramAdapter implements the port without running a daemon
- **Given** the `EngramAdapter` in `spectty-adapters`
- **When** the crate compiles and the adapter is type-checked against `PersistencePort`
- **Then** it MUST satisfy the `PersistencePort` trait, its method bodies MUST be `todo!()`, and compiling/type-checking it MUST NOT require a running engram daemon or any network access

---

## ADDED Capability: tauri-bridge

The Bridge proves bidirectional communication between the Rust shell and the React UI via
one command and one event.

### Requirement: ping command emits an observable pong event
The `src-tauri` bridge MUST expose a `ping` Tauri command (Tauri v2). Invoking it MUST
result in a `pong` event emitted via the v2 `AppHandle::emit` API, observable in the
running app.

#### Scenario: ping → pong is visible in the running app
- **Given** the app running via `pnpm tauri dev`
- **When** the UI invokes the `ping` command
- **Then** a `pong` event MUST be emitted by the bridge AND the UI listener MUST observe it and log it to the web console

#### Scenario: Bridge uses Tauri v2 emit API (guard against v1 drift)
- **Given** the bridge implementation of `pong`
- **When** the emit call is inspected
- **Then** it MUST use the Tauri v2 `AppHandle::emit` API and MUST NOT use removed Tauri v1 emit signatures

---

## ADDED Capability: ci-pipeline

CI MUST enforce build, formatting, linting, tests, and the boundary gate on every change,
running on a macOS runner.

### Requirement: CI runs and passes all Rust and JS gates
CI MUST run on `macos-latest` and MUST pass: `cargo build`, `cargo fmt --check`,
`cargo clippy -D warnings`, `cargo test --workspace`, and `pnpm test` (Vitest). The
`cargo-deny` boundary gate (per hexagonal-core) MUST also pass.

#### Scenario: Clean scaffold turns CI green
- **Given** the compliant M0 scaffold pushed to a branch
- **When** CI runs on the macOS runner
- **Then** `cargo build`, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, `pnpm test`, and the `cargo-deny` boundary gate MUST all exit 0

#### Scenario: A clippy warning fails CI (negative/guard)
- **Given** a change that introduces a clippy lint warning
- **When** CI runs `cargo clippy -D warnings`
- **Then** the step MUST fail (warnings promoted to errors) and block the merge

#### Scenario: Unformatted code fails CI (negative/guard)
- **Given** a change that leaves code not matching `cargo fmt`
- **When** CI runs `cargo fmt --check`
- **Then** the step MUST fail and block the merge

### Requirement: Vitest harness proves the UI test path with mocked Tauri
At least one Vitest test MUST mock `@tauri-apps/api` and assert the UI's
`invoke("ping")` call and `listen("pong")` wiring. This proves the front-end test
harness works without a running backend.

#### Scenario: Vitest test passes against a mocked Tauri API
- **Given** a Vitest test that mocks `@tauri-apps/api`
- **When** `pnpm test` runs
- **Then** the test MUST assert that the UI invokes `ping` and registers a `pong` listener, and MUST pass without a running Tauri backend

---

## ADDED Capability: onboarding-tooling

A new contributor MUST be able to go from clean clone to a running `ping → pong` quickly,
guided by documented conventions.

### Requirement: Documented dev workflow with hot reload
The repository MUST document the canonical dev workflow, including the `pnpm tauri dev`
hot-reload entry point and hot-reload tooling (cargo-watch). The docs MUST identify
`pnpm tauri dev` as the single canonical dev entry.

#### Scenario: Docs describe the canonical dev entry and hot reload
- **Given** the project conventions/onboarding documentation
- **When** a new contributor reads it
- **Then** it MUST name `pnpm tauri dev` as the canonical dev entry and MUST describe hot-reload tooling (cargo-watch) for Rust changes

### Requirement: New contributor onboards under 30 minutes
A new contributor following the documented workflow MUST be able to clone, build, and
observe `ping → pong` in under 30 minutes on a supported macOS machine. The docs SHOULD
note that the first clean build may take several minutes (sccache is used in CI to mitigate).

#### Scenario: Clean-clone to ping→pong within the onboarding budget
- **Given** a new contributor on a supported macOS machine with prerequisites (Rust 1.89, pnpm, Node) installed
- **When** they follow the documented steps from clean clone through `pnpm tauri dev`
- **Then** they MUST reach a running app showing `pong` in the web console in under 30 minutes total

---

## Deferred (NOT in this delta)

Per the proposal "Scope — Out", the following are explicitly out of scope for M0 and MUST
NOT be specified or implemented here:

- PTY adapter / xterm.js rendering → M1
- `AgentStatus` state machine, `AgentRunner`, `SessionRegistry` → M2
- Real engram HTTP client + polling/subscribe event layer → M3
- `GitPort` (worktrees) → M4
- `NotifierPort` (OS notifications) → M5
- Playwright E2E / headless-webview integration tests → M3+
