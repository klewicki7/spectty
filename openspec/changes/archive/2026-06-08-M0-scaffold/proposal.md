# Proposal: M0 — Scaffold + Engram Wiring

## Intent

Prove the Spectty stack wires end-to-end and that hexagonal boundaries — including the engram quarantine — are enforced from **day one**, not bolted on later. M0 establishes the skeleton every later milestone builds on: a runnable Tauri + Rust + React app, a domain core that depends on nothing outward, and a `PersistencePort` defined in Core with engram as its first adapter. Success means a new contributor can clone, build, and see `ping → pong` in < 30 minutes (roadmap "What it proves").

## Scope

### In Scope
- **Monorepo**: Cargo workspace + pnpm workspace at root (`Cargo.toml`, `package.json`, `pnpm-workspace.yaml`, `rust-toolchain.toml` pinning Rust 1.89).
- **Hexagonal skeleton** (`crates/core`, pkg name `spectty-core`): `entities/` with behaviorless placeholders `Session`, `Workspace`, `AgentStatus`; `ports/` with `PersistencePort` trait (`write`/`read` only).
- **Adapters** (`crates/adapters`, pkg name `spectty-adapters`): in-memory stub adapter (round-trip test support) + `EngramAdapter` skeleton (`todo!()`, no real HTTP).
- **Bridge** (`src-tauri`): one `#[tauri::command] ping` + one `pong` event (Tauri v2 `AppHandle::emit`).
- **UI** (`ui/`): React 19 + Vite calling `invoke("ping")`, listening for `"pong"`, logging to console.
- **CI** (macOS runner): `cargo build`, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, `pnpm test` (Vitest). `cargo-deny` boundary gate. sccache for build times.
- **Dev tooling**: `pnpm tauri dev` as the canonical hot-reload entry point; conventions documented.

### Out of Scope (deferred)
- PTY / xterm.js → M1
- `AgentStatus` state machine, `AgentRunner`, `SessionRegistry` → M2
- Real engram HTTP + polling/subscribe → M3
- `GitPort` → M4 · `NotifierPort` → M5
- Playwright E2E, headless-webview integration tests → M3+

## Capabilities

### New Capabilities
- `monorepo-scaffold`: Cargo + pnpm workspace layout, toolchain pin, dev entry point.
- `hexagonal-core`: `spectty-core` entities + `PersistencePort`, with inward-only dependency rule.
- `persistence-port`: port contract + in-memory stub + `EngramAdapter` skeleton + round-trip test.
- `tauri-bridge`: `ping` command and `pong` event proving bidirectional comms.
- `ci-pipeline`: build/fmt/clippy/test/vitest + `cargo-deny` boundary enforcement.

### Modified Capabilities
- None (greenfield).

## Approach

Locked decisions (carrying exploration recommendations):
- **Crate names**: `spectty-core` / `spectty-adapters` (searchable in `Cargo.lock`, no third-party collision).
- **Boundary enforcement**: Cargo dependency graph is the PRIMARY gate — `spectty-core`'s `Cargo.toml` lists no tauri/engram/adapters deps, so the compiler rejects violations (physics, not policy). `cargo-deny` in CI is belt-and-suspenders against accidental future deps.
- **"Engram wired" = port + stub**: `PersistencePort` defined in Core; in-memory stub passes the round-trip unit test. NO engram daemon runs in M0; `EngramAdapter` is a `todo!()` skeleton.
- **Vitest**: mock `@tauri-apps/api` and assert `invoke("ping")` + `listen("pong")` wiring (proves test infra).

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `Cargo.toml`, `package.json`, `pnpm-workspace.yaml`, `rust-toolchain.toml` | New | Workspace roots + toolchain pin |
| `crates/core/` | New | `spectty-core`: entities + `PersistencePort` |
| `crates/adapters/` | New | `spectty-adapters`: in-memory stub + `EngramAdapter` skeleton |
| `src-tauri/` | New | Bridge: `ping` command + `pong` event |
| `ui/` | New | React 19 + Vite + Vitest |
| `.github/workflows/`, `deny.toml` | New | CI matrix + boundary gate |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Tauri v2 `emit` moved to `AppHandle` (v1 used `Window`) | Med | Use v2 APIs throughout; check Tauri v2 docs at apply time |
| "Engram wired" misread as "daemon must run" | Med | Proposal explicit: M0 = port + stub + skeleton only; no daemon |
| First clean build (5–15 min) threatens < 30 min goal | Med | Add sccache in CI; document slow first build |
| macOS-only; Linux runner breaks on Tauri native deps | Low | Pin CI to `macos-latest` explicitly |

## Rollback Plan

Greenfield — no existing system to break. Rollback = revert the scaffold PR / delete the created directories. No data migration, no production impact.

## Dependencies

- Rust 1.89 stable, pnpm, Node for React 19/Vite. No running engram daemon required for M0 exit criteria.

## Delivery Note

Cached delivery strategy is **single-PR** with a `size:exception` to be recorded BEFORE apply (scaffold is one cohesive unit that exceeds the 400-line budget). Tasks/apply phases MUST honor this.

## Success Criteria

- [ ] Clean clone: `cargo build` and `pnpm tauri dev` both succeed.
- [ ] `ping → pong` round-trip visible in the web console of the running app.
- [ ] `spectty-core` imports nothing from adapters / tauri / engram (Cargo graph + `cargo-deny` confirm).
- [ ] `PersistencePort::write` / `read` round-trip passes a unit test (in-memory stub).
- [ ] CI green: build, fmt, clippy (`-D warnings`), `cargo test`, Vitest.
- [ ] New contributor onboarding < 30 minutes.
