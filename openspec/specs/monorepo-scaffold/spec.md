# Capability: monorepo-scaffold

> Living baseline spec. Established by change `M0-scaffold` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

A single repository MUST host a Cargo workspace (Rust) and a pnpm workspace (JS/TS)
that coexist and both build from a clean clone.

## Requirement: Cargo workspace builds from a clean clone
The repository MUST define a root Cargo workspace including `spectty-core`,
`spectty-adapters`, and the `src-tauri` bridge crate. `cargo build` MUST succeed on a
clean clone with no manual setup beyond installing the pinned toolchain.

### Scenario: Clean clone compiles the Rust workspace
- **Given** a fresh clone of the repository on macOS with the pinned Rust toolchain installed
- **When** a contributor runs `cargo build` at the repository root
- **Then** the command MUST exit 0 and produce build artifacts for `spectty-core`, `spectty-adapters`, and the `src-tauri` crate

### Scenario: Toolchain is pinned, not floating
- **Given** a `rust-toolchain.toml` pinning Rust 1.89 at the repository root
- **When** a contributor builds without an explicitly selected toolchain
- **Then** the build MUST use the pinned 1.89 toolchain rather than the machine default

## Requirement: pnpm workspace installs and runs the dev app
The repository MUST define a pnpm workspace (`pnpm-workspace.yaml`, root `package.json`)
including the `ui/` package. `pnpm install` followed by the canonical dev entry MUST
launch the running Tauri + React app.

### Scenario: Clean clone installs and starts the dev app
- **Given** a fresh clone with pnpm and Node available
- **When** a contributor runs `pnpm install` and then `pnpm tauri dev`
- **Then** the app window MUST launch with the React 19 + Vite frontend served and the Tauri shell running

### Scenario: Cargo and pnpm workspaces coexist without collision
- **Given** both the Cargo workspace and the pnpm workspace defined at the repository root
- **When** a contributor runs `cargo build` and `pnpm install` in either order
- **Then** neither command MUST corrupt or invalidate the other's lockfile, target directory, or node_modules
