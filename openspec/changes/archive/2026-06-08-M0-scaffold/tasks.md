# Tasks: M0 — Scaffold + Engram Wiring

Source: `sdd/M0-scaffold/spec` + `sdd/M0-scaffold/design`. Ordered, dependency-aware
implementation checklist. Each task maps to a spec requirement (REQ tag) and to a
work unit (reviewable commit). M0-scoped only — PTY/xterm, AgentRunner, AgentStatus
transitions, SessionRegistry, real engram HTTP, GitPort, NotifierPort are DEFERRED.

Legend: `[P]` = can run in parallel with siblings in the same group once the group's
prerequisite is met. Groups are sequential unless noted.

---

## WU1 — Workspace skeleton (root tooling)
Prereq: none. Establishes the dual Cargo + pnpm workspace and pinned toolchain.
Maps to: REQ monorepo-scaffold (Cargo workspace builds), REQ monorepo-scaffold
(pnpm workspace coexists).

- [x] 1.1 Root `Cargo.toml`: `[workspace]` with members `crates/core`, `crates/adapters`,
      `src-tauri`; `resolver = "2"`. Done: file parses (members may not exist yet).
      (Batch 2: `src-tauri` now RE-ADDED to members — the crate dir exists.)
- [x] 1.2 `rust-toolchain.toml` pinning `channel = "1.89.0"`, components `rustfmt`, `clippy`.
      Done: `rustc --version` via toolchain reflects 1.89.0, not machine default.
      (REQ: rust-toolchain pins 1.89, build uses pinned.)
- [x] 1.3 Root tooling configs: `rustfmt.toml`, `clippy.toml`, `deny.toml` (skeleton —
      `[bans]` filled in WU8). Done: files present, valid TOML.
      (Batch 1: `rustfmt.toml` + `clippy.toml` created; `deny.toml` deferred to WU8/batch 3.)
- [x] 1.4 `pnpm-workspace.yaml` listing `ui` as the only M0 package; root private
      `package.json` (name, `"private": true`, `@tauri-apps/cli` devDep, `tauri` script,
      `packageManager: pnpm@9.12.2`, `engines.node >=23`).
      Done (batch 2): `pnpm install` resolves the workspace without error.
- [x] 1.5 `.gitignore`: `target/`, `node_modules/`, `dist/`, `ui/dist/`, `*.log`, OS cruft.
      Done: cargo/pnpm artifacts ignored; Cargo.lock + pnpm-lock.yaml NOT ignored.
- [x] 1.6 Verify dual-workspace coexistence: `cargo metadata` resolves AND `pnpm install`
      succeeds; neither corrupts the other's lockfile/target/node_modules.
      Done: both commands exit 0. (REQ: workspaces coexist.)
      (Batch 2: `pnpm install` green alongside cargo; `target/` and `node_modules/` separate.)

## WU2 — spectty-core crate (entities + persistence port)
Prereq: WU1. Pure sync core, inward-only deps (serde + thiserror ONLY). This is the
PRIMARY boundary gate — the dep list itself is the quarantine.
Maps to: REQ hexagonal-core (placeholder types), REQ hexagonal-core (inward-only deps),
REQ persistence-port (trait write/read only).

- [x] 2.1 `crates/core/Cargo.toml`: pkg `spectty-core`; deps `serde` (derive feature) +
      `thiserror` ONLY. No serde_json, no tokio, no anyhow, no tauri, no adapters.
      Done: `cargo build -p spectty-core` compiles. (REQ: core lists no outward deps.)
- [x] 2.2 `src/entities/agent_status.rs`: `AgentStatus` enum with variants
      `Starting | Idle | Running | AwaitingInput | Completed | Error`, derive
      `Serialize, Deserialize` — NO transitions/behavior (deferred M2). [P]
- [x] 2.3 `src/entities/workspace.rs`: `WorkspaceId(String)`, `Workspace { id, root: String }`,
      behaviorless, serde derives. [P]
- [x] 2.4 `src/entities/session.rs`: `SessionId(String)`,
      `Session { id, workspace: WorkspaceId, status: AgentStatus, title: String }`,
      behaviorless, serde derives (Worktree/Spec/Cost/Diff/Checkpoint deferred). [P]
- [x] 2.5 `src/entities/mod.rs` re-exporting the three modules.
      Done: all three placeholder types present, no domain behavior.
      (REQ: Session, Workspace, AgentStatus exist, behaviorless.)
- [x] 2.6 `src/ports/persistence.rs`: `PersistenceError` (thiserror) with `Backend(String)`;
      `trait PersistencePort: Send + Sync` with
      `fn upsert(&self, topic_key: &str, payload: String) -> Result<(), PersistenceError>`
      and `fn get(&self, topic_key: &str) -> Result<Option<String>, PersistenceError>`. SYNC,
      no engram/HTTP/tauri/adapter/serde_json refs. Done: port exposes write+read only.
      (REQ: PersistencePort write+read only, pure contract.)
      (Apply-time correction: `&self` not `&mut self` so the port is shareable as
      `Arc<dyn PersistencePort>` across concurrent Sessions; `get` returns `Ok(None)` for a
      missing key — matches the spec's missing-key→None guard. `NotFound` variant dropped.)
- [x] 2.7 `src/ports/mod.rs` + `src/lib.rs` wiring modules + public re-exports.
      Done: `cargo build -p spectty-core` compiles with zero outward deps.

## WU3 — spectty-adapters crate (in-memory + engram skeleton)
Prereq: WU2. Adapters depend on core only (+ serde_json, anyhow, thiserror).
Maps to: REQ persistence-port (in-memory stub adapter), REQ persistence-port
(EngramAdapter skeleton with todo!()).

- [x] 3.1 `crates/adapters/Cargo.toml`: pkg `spectty-adapters`; deps `spectty-core`,
      `serde_json`, `anyhow`, `thiserror`. NO tauri, NO reqwest (M3). Done: parses.
- [x] 3.2 `src/persistence/in_memory.rs`:
      `InMemoryPersistenceAdapter { store: Mutex<HashMap<String,String>> }`,
      `impl PersistencePort` — `upsert` locks + inserts; `get` locks + clones → `Ok(Option)`.
      Interior mutability (`Mutex`) honors the `&self` contract and keeps the adapter
      `Send + Sync` so it is usable behind `Arc<dyn PersistencePort>`. REAL adapter (not a
      mock). (REQ: in-memory stub implements PersistencePort.)
- [x] 3.3 `src/persistence/engram.rs`: `EngramAdapter::default()`, `impl PersistencePort`
      with `todo!("M3: POST/GET engram :7437 /api/observations")` bodies. NO network,
      NO daemon needed to compile. (REQ: EngramAdapter skeleton, todo!() bodies.)
- [x] 3.4 `src/persistence/mod.rs` + `src/lib.rs` re-exports.
      Done: `cargo build -p spectty-adapters` compiles; both adapters satisfy
      `PersistencePort`.

## WU4 — PersistencePort round-trip test (port proof)
Prereq: WU3. Tests live WITH the code they verify (work-unit-commits rule).
Maps to: REQ persistence-port (round-trip asserts value unchanged),
REQ persistence-port NEGATIVE/GUARD (missing key returns None/NotFound, no error/stale).

- [x] 4.1 Inline `#[cfg(test)]` in `crates/adapters/src/persistence/in_memory.rs`:
      `test_in_memory_persistence_round_trips` — upsert payload under key, get returns
      same payload unchanged. NO `#[tokio::test]` (sync port is the payoff).
- [x] 4.2 `test_get_missing_key_returns_none` — get on absent key returns `Ok(None)`,
      no error, no panic, no stale value. Plus `test_usable_behind_arc_dyn_port` proving
      the adapter works through `Arc<dyn PersistencePort>` (the `&self`/shareability win).
      Done: `cargo test --workspace` green. (3 passed, 0 failed.)

## WU5 — src-tauri bridge (ping → pong, Tauri v2)
Prereq: WU2 + WU3 (links core + adapters). ONLY crate allowed a tauri dep.
Maps to: REQ tauri-bridge (ping command emits pong via v2 AppHandle::emit),
REQ tauri-bridge GUARD (v2 Emitter, not v1 Window::emit).

- [x] 5.1 `src-tauri/Cargo.toml`: pkg `spectty`; deps `spectty-core`, `spectty-adapters`,
      `tauri` v2, `tokio`, `serde`; `tauri-build` build-dep; `build.rs`. `[lib] name = "spectty_lib"`
      (v2 lib/bin split). Done: parses, member of workspace, `cargo build -p spectty` compiles.
- [x] 5.2 `src/commands/ping.rs` (Tauri v2): `use tauri::{AppHandle, Emitter};`
      `#[tauri::command] pub fn ping(app: AppHandle) -> Result<(), String>` emitting
      `app.emit("pong", "pong from spectty backend")` mapped to `String` error.
      GUARD VERIFIED: v2 `Emitter` trait import is what makes `app.emit` resolve; v1
      `Window::emit` would not compile. Confirmed against tauri 2.11.2.
- [x] 5.3 `src/commands/mod.rs`; `src/lib.rs` (`pub fn run()`) + `src/main.rs`:
      `Builder::default().invoke_handler(generate_handler![commands::ping::ping]).run(generate_context!())`.
- [x] 5.4 `tauri.conf.json` (v2 schema): `build.frontendDist ../ui/dist`,
      `build.devUrl http://localhost:1420`, window label `main`; `capabilities/default.json`
      grants `core:default` + `core:event:default` to the `main` window (needed for emit/listen).
      Done: `cargo build -p spectty` + `cargo build --workspace` compile green.

## WU6 — ui/ React 19 + Vite (frontend + invoke/listen wiring)
Prereq: WU5 (needs the `ping` command to invoke). Tauri calls live in hooks
(project-structure convention).
Maps to: REQ monorepo-scaffold (pnpm install + pnpm tauri dev launches app),
REQ tauri-bridge (ping→pong visible in web console).

- [x] 6.1 `ui/package.json`: `react`@19, `react-dom`@19, `vite`@6, `@tauri-apps/api`@2,
      `vitest`@2, `@testing-library/react`, `@vitejs/plugin-react`, `jsdom`, `typescript`@5.7.
      `dev`/`build`/`test`/`typecheck` scripts. `vite.config.ts` (port 1420, strictPort),
      `tsconfig.json` (strict, react-jsx), `index.html`. Done: `pnpm install` resolves.
- [x] 6.2 `ui/src/hooks/usePingPong.ts`: `invoke` from `@tauri-apps/api/core`, `listen`
      from `@tauri-apps/api/event` (v2 paths). `useEffect` registers `listen("pong")` →
      `setPong` + `console.log`, returns unlisten cleanup; `sendPing = async () => invoke("ping")`.
- [x] 6.3 `ui/src/App.tsx` (button calls `sendPing`, displays pong) + `ui/src/main.tsx`
      (React 19 `createRoot`, named imports, no `forwardRef`/manual memo). Done: `pnpm --filter ui build`
      (tsc --noEmit + vite build) green, `ui/dist` produced. Live `pnpm tauri dev` ping→pong is a
      runtime check left for manual verify (compile + Vitest contract is the M0 automated bar).

## WU7 — Vitest frontend test (FE harness without backend)
Prereq: WU6.
Maps to: REQ ci-pipeline (>=1 Vitest test mocks @tauri-apps/api, asserts invoke+listen).

- [x] 7.1 `ui/tests/unit/usePingPong.test.ts`: `vi.mock` `@tauri-apps/api/core` (invoke spy)
      and `@tauri-apps/api/event` (capture listen callback). `renderHook`; assert a `"pong"`
      listener is registered on mount, `sendPing` calls `invoke("ping")`, and firing the captured
      pong handler updates `pong` state. `vitest.config.ts` jsdom env. TDD: written FIRST (RED —
      module-not-found), then hook implemented (GREEN). Done: `pnpm --filter ui test` → 3 passed,
      no running backend.

## WU8 — cargo-deny boundary backstop
Prereq: WU2 (needs core dep closure to exist). Secondary belt-and-suspenders gate;
Cargo dep graph remains the PRIMARY gate.
Maps to: REQ hexagonal-core (cargo-deny secondary gate),
REQ hexagonal-core NEGATIVE/GUARD (forbidden core dep fails cargo-deny),
REQ hexagonal-core (cargo-deny exits 0 on clean scaffold).

- [x] 8.1 Fill `deny.toml` `[bans]`: deny `tauri`, `tokio`, `reqwest` (engram client added M3).
      Done: root `deny.toml` with `[bans] deny = [tauri, tokio, reqwest]` + permissive
      licenses/advisories so the scoped `check bans` invocation stays focused.
- [x] 8.2 Verify scope: `cargo deny --manifest-path crates/core/Cargo.toml check bans`
      exits 0 on clean scaffold (per-crate so src-tauri keeps tauri). Done: `bans ok`, exit 0.
      Confirmed cargo-deny 0.19.8 installed via `cargo install cargo-deny --locked`.
- [x] 8.3 GUARD proof (NEGATIVE CONTRACT — demonstrated, not just asserted): temporarily
      added `tokio = "1"` to `crates/core/Cargo.toml`, ran the scoped check →
      `error[banned]: crate 'tokio = 1.52.3' is explicitly banned` (exit 2), `cargo tree
      -p spectty-core` showed tokio in the core closure. REVERTED → `bans ok` (exit 0),
      tree clean again. Proves the quarantine is mechanically enforced. No forbidden dep left.
      (REQ NEGATIVE/GUARD: forbidden core dep cannot merge.)

## WU9 — CI workflow (macos-latest, all gates)
Prereq: WU1–WU8 (gates reference all prior outputs).
Maps to: REQ ci-pipeline (build, fmt --check, clippy -D warnings, test --workspace,
pnpm test, cargo-deny), REQ ci-pipeline GUARDs (clippy warning + unformatted code block merge).

- [x] 9.1 `.github/workflows/ci.yml` single job on `macos-latest` (Linux breaks Tauri deps).
      Steps: checkout; rust-toolchain 1.89.0 (rustfmt, clippy); sccache-action
      (`RUSTC_WRAPPER=sccache`, PROVISIONAL per design Q2 — commented as such, measure at
      verify); pnpm/action-setup; setup-node 23 + pnpm cache; Cargo cache keyed on Cargo.lock;
      cargo-deny-action (install only). Concurrency-cancel + push/PR-to-main triggers.
- [x] 9.2 Gate steps in order: `cargo fmt --all -- --check`;
      `cargo clippy --workspace --all-targets -- -D warnings`; `cargo build --workspace`
      (primary boundary gate); `cargo test --workspace`;
      `cargo deny --manifest-path crates/core/Cargo.toml check bans` (backstop);
      `pnpm install --frozen-lockfile`; `pnpm --filter ui test`; `pnpm --filter ui build`.
      Done: YAML validated (15 steps parse via ruby YAML loader). Steps mirror the exact
      gates run green locally. A clippy warning (`-D warnings`) or unformatted file would
      fail and block merge. GitHub Actions cannot run locally — bar is valid+complete workflow.

## WU10 — Dev tooling + onboarding docs
Prereq: WU6 (canonical command must actually run the app).
Maps to: REQ onboarding-tooling (documented dev workflow, pnpm tauri dev canonical,
cargo-watch hot-reload), REQ onboarding-tooling (clone→build→ping/pong < 30 min).

- [x] 10.1 Document canonical dev entry `pnpm tauri dev` (Vite :1420 HMR + src-tauri dev)
      and the hot-reload split: UI via Vite HMR; Rust-only loop via optional
      `cargo watch -x 'build --workspace'`. Done: getting-started.md "Run in development"
      section names `pnpm tauri dev` as canonical + documents the hot-reload split + the
      ping/pong console verification. CLI is the pnpm-scoped `@tauri-apps/cli` (no global install).
- [x] 10.2 Onboarding path in `docs/engineering/getting-started.md`: clean clone →
      `pnpm install` → `pnpm tauri dev` → see ping/pong in console. Prereqs Rust 1.89/Node 23/
      pnpm. Test commands corrected to real ones (`cargo test --workspace`,
      `pnpm --filter ui test`, `cargo deny ... check bans`); the non-existent `pnpm test:e2e`
      (Playwright, deferred M3+) removed. Replaced the "aspirational" banner with the working
      < 30 min path. Done: documented commands match reality.

---

## Spec Requirement Coverage Map (all 14 REQs covered)

| Capability | REQ | Task(s) |
|---|---|---|
| monorepo-scaffold | Cargo workspace builds from clean clone | 1.1, 1.2, 1.6 |
| monorepo-scaffold | pnpm workspace installs + runs dev app | 1.4, 6.1, 6.3 |
| monorepo-scaffold | workspaces coexist (no lockfile corruption) | 1.6 |
| hexagonal-core | placeholder types Session/Workspace/AgentStatus | 2.2, 2.3, 2.4, 2.5 |
| hexagonal-core | core depends inward only (+ neg guard) | 2.1, 2.7, 8.x |
| persistence-port | PersistencePort write/read only | 2.6 |
| persistence-port | in-memory stub + round-trip (+ neg guard) | 3.2, 4.1, 4.2 |
| persistence-port | EngramAdapter skeleton todo!() | 3.3 |
| tauri-bridge | ping → pong via v2 AppHandle::emit (+ guard) | 5.2, 6.3 |
| ci-pipeline | CI gates green (+ clippy/fmt neg guards) | 9.1, 9.2 |
| ci-pipeline | >=1 Vitest test mocks @tauri-apps/api | 7.1 |
| onboarding-tooling | documented dev workflow + cargo-watch | 10.1, 10.2 |
| onboarding-tooling | clone→ping/pong < 30 min | 10.2 |
| hexagonal-core | cargo-deny boundary gate exits 0 clean | 8.1, 8.2, 8.3 |

---

## Review Workload Forecast

- Estimated total changed lines: ~520–600 (Rust: ~230 incl. 2 tests; UI: ~150 incl.
  Vitest; configs/CI/docs: ~180). Net-new files, near-zero modifications.
- Exceeds 400-line budget: YES (~+150–200 over).
- Chained PRs recommended: No. This is a greenfield scaffold where the hexagonal
  boundary, the dep-graph quarantine, and its enforcement (cargo-deny + CI) only make
  sense reviewed together — splitting would land half a boundary and a CI that fails on
  the absent half. Strong cohesion outweighs line count.
- Delivery strategy: single-PR (cached). Expected path is recording a `size:exception`,
  NOT splitting. This scaffold is one cohesive unit best reviewed whole.
- Decision needed before apply: No (single-PR + `size:exception` is the expected,
  pre-agreed path). Proceed to sdd-apply with `size:exception` recorded.
- Work-unit commits inside the single PR: WU1..WU10 map to clean Conventional Commits,
  each with a coherent start/finish + co-located verification, so the PR still tells a
  reviewable story commit-by-commit.

## Open items carried from design (resolve at apply)
1. Exact semver pins — defer to first green compile, record in lockfiles.
2. sccache in CI — enabled for M0 to protect <30min; measure at verify, drop if fast.
3. cargo-deny invocation scope — confirm `--manifest-path crates/core/Cargo.toml check bans`.
4. Root `pnpm test` proxy vs `--filter ui` — cosmetic, resolve at apply.
