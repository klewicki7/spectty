# Verify Report: M0 — Scaffold + Engram Wiring

Independent adversarial verification (sdd-verify). Verifier did NOT write this code; all gates were re-run from source, including a from-scratch RED/GREEN reproduction of the hexagonal boundary backstop. Strict TDD active.

**Status: PASS** — all 14 spec requirements satisfied, all CI gates green when re-run locally, hexagonal quarantine mechanically proven by me. M0 exit criteria are met.

## Executive summary
0 CRITICAL · 3 WARNING · 4 SUGGESTION. M0 is DONE per its exit criteria. The warnings are operational/onboarding nits (none block archive); the suggestions are forward hygiene. Apply-progress claims were verified against reality and held up — no faked tests, no boundary leak, no scope creep into M1–M5.

## Gate re-run output (actual, this verification)

| Gate | Command | Result |
|---|---|---|
| Toolchain pin | `rustc --version` | `1.89.0` (matches rust-toolchain.toml) |
| Node/pnpm | `node -v` / `pnpm -v` | `v23.0.0` / `9.12.2` (matches engines) |
| Build | `cargo build --workspace` | exit 0, Finished |
| Format | `cargo fmt --all -- --check` | exit 0, zero diff |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, zero warnings |
| Rust tests | `cargo test --workspace` | 3 passed / 0 failed (round-trip, missing-key→None, Arc<dyn>) |
| Core tree | `cargo tree -p spectty-core` | only serde + thiserror; NO tauri/tokio/reqwest/adapters |
| cargo-deny (green) | `cargo deny --manifest-path crates/core/Cargo.toml check bans` | `bans ok` exit 0 |
| Vitest | `pnpm --filter ui test` | Test Files 1 passed, Tests 3 passed |
| UI build | `pnpm --filter ui build` | exit 0, 32 modules, dist emitted |
| CI YAML | `ruby -ryaml YAML.load_file` | valid |

### Independent boundary RED/GREEN reproduction (the spec's most important guard)
I did NOT trust the apply report. I appended `tokio = "1"` to `crates/core/Cargo.toml` myself:
- **RED**: `cargo deny --manifest-path crates/core/Cargo.toml check bans` →
  `error[banned]: crate 'tokio = 1.52.3' is explicitly banned` (non-zero exit). `cargo tree -p spectty-core` showed tokio entering the closure.
- **REVERT + GREEN**: restored Cargo.toml → `bans ok` exit 0; `cargo tree -p spectty-core` clean again; `Cargo.lock` core entry shows only `serde`, `thiserror` (no tokio residue).

Mechanically enforced, not just asserted. Matches spec NEGATIVE/GUARD scenario.

Nuance worth recording: merely *declaring* a banned dep in core still compiles (tokio is a valid crate), so the **dep-list ban (cargo-deny) is the gate that catches declaration**; the **compiler primary gate** only fails once core source actually `use`s a forbidden symbol (unresolved import). Both layers exist and complement each other exactly as the design states.

## Requirement-by-requirement (14 REQs)

| REQ | Capability | Verdict | Evidence |
|---|---|---|---|
| Cargo workspace builds clean | monorepo-scaffold | PASS | 3 members build; toolchain pinned 1.89.0 |
| pnpm workspace + dev app | monorepo-scaffold | PASS | pnpm install resolved; ui builds; tauri.conf devUrl :1420, beforeDevCommand wired |
| Cargo/pnpm coexist | monorepo-scaffold | PASS | both lockfiles present, separate trees, gitignore keeps lockfiles |
| Core placeholder types | hexagonal-core | PASS | Session/Workspace/AgentStatus behaviorless, derive-only |
| Core inward-only | hexagonal-core | PASS | core deps = serde+thiserror; tree clean; **RED/GREEN reproduced by verifier** |
| cargo-deny boundary gate green on clean | hexagonal-core | PASS | `bans ok` exit 0 |
| PersistencePort write+read only | persistence-port | PASS | `upsert(&self,...)` + `get(&self,...)->Result<Option<String>>`; pure, sync, no serde_json/tauri |
| In-memory stub round-trip + missing→None | persistence-port | PASS | 3 tests pass; missing-key returns `Ok(None)` (corrected contract) |
| EngramAdapter skeleton todo!() no network | persistence-port | PASS | impl PersistencePort, bodies `todo!("M3...")`, no reqwest |
| ping→pong via v2 AppHandle::emit | tauri-bridge | PASS | `use tauri::{AppHandle, Emitter}`; `app.emit("pong",...)`; NOT v1 Window::emit; registered in generate_handler! |
| CI gates on macos-latest | ci-pipeline | PASS | ci.yml runs fmt/clippy/build/test/deny/install/vitest/build; all mirror local gates which I ran green |
| ≥1 Vitest mocks @tauri-apps/api + asserts invoke("ping")+listen("pong") | ci-pipeline | PASS | test mocks /core + /event, asserts `invoke("ping")` and `listen("pong", fn)` |
| Documented dev workflow + cargo-watch | onboarding | PASS | getting-started.md: `pnpm tauri dev` canonical, hot-reload split, cargo-watch documented |
| Clone→ping/pong <30min | onboarding | PASS (plausible) | working path documented; prereqs/troubleshooting present |

## Findings

### CRITICAL (0)
None. No spec violation, no broken gate, no boundary leak, no faked/missing test.

### WARNING (3)
- **W1 — CI `--frozen-lockfile` will fail until lockfiles are committed.** `pnpm-lock.yaml` and `Cargo.lock` exist on disk but are NOT git-tracked (zero commits yet). CI step `pnpm install --frozen-lockfile` (`.github/workflows/ci.yml:95`) errors if `pnpm-lock.yaml` is absent from the checkout. `.gitignore` correctly keeps lockfiles, so the fix is simply: commit both lockfiles in the first commit. Not a code defect — a "no commits exist yet" artifact — but it WILL red the first CI run if lockfiles are omitted. Fix: ensure `git add Cargo.lock pnpm-lock.yaml` in the initial commit.
- **W2 — sccache in CI is unvalidated and may hard-fail the job.** `ci.yml:23` sets `RUSTC_WRAPPER: sccache` globally and uses `mozilla-actions/sccache-action@v0.0.6`. If the action/binary or GHA cache backend misbehaves, EVERY cargo step fails (wrapper missing). It is flagged PROVISIONAL per design open-Q2, but as written it is a single point of failure for all Rust gates. Fix: gate sccache behind a conditional or verify the action on the first CI run; be ready to drop `RUSTC_WRAPPER` if it flakes.
- **W3 — Doc references `portable-pty` (M1 concept) in M0 onboarding.** `docs/engineering/getting-started.md:174-177` has a "`portable-pty` build failures" troubleshooting entry. PTY is explicitly DEFERRED to M1. M0 has no portable-pty dependency, so this troubleshooting section is dead/aspirational for the current scaffold and could confuse a new contributor ("why would I hit a portable-pty error?"). Minor doc scope-bleed — not a code violation. Fix: remove or move the portable-pty note to an M1 doc.

### SUGGESTION (4)
- **S1 — `.expect()` on mutex lock in `in_memory.rs:30,38`.** Idiomatic and acceptable for an in-memory test adapter (poisoning only on a panic-while-locked, which these trivial ops won't trigger). If you later want zero-panic guarantees you could map `PoisonError` into `PersistenceError::Backend`, but for M0 this is fine and arguably clearer. No change required.
- **S2 — `tokio` dependency in `src-tauri/Cargo.toml:27` is currently unused.** The M0 ping command is sync; tokio (`rt-multi-thread`, `macros`) isn't exercised yet. It's harmless (and forward-looking for M2/M3 async), but clippy/unused-dep tooling won't flag it and it slightly inflates the bridge build. Keep if intentional pre-wiring; otherwise defer to when first awaited.
- **S3 — `core:event:default` capability is broad enough; confirm it stays minimal.** `capabilities/default.json` grants `core:default` + `core:event:default` for the `main` window — correct and minimal for listen/emit of `pong`. As commands grow, prefer per-command permissions over widening defaults. Note for M2+, no action now.
- **S4 — No commit/branch/PR exists yet (Git policy honored).** The entire tree is untracked. Archive/PR readiness depends on an initial commit that includes lockfiles (see W1). VibeLens `show_diff_explanation` was correctly skipped (no HEAD). Flag for the apply/PR step, not a defect.

## Quality judgment
- **Rust idiomaticity**: clean. Interior mutability via `Mutex<HashMap>` correctly honors the `&self` port and keeps the adapter `Send + Sync` for `Arc<dyn PersistencePort>` (proven by `test_usable_behind_arc_dyn_port`). thiserror in core, anyhow reserved for adapter edge, commands return `Result<_,String>`. No needless clones (`get` clones the stored String, which is required to return owned data). Doc comments are accurate and explain the boundary rationale.
- **Tauri v2 correctness**: correct. `Emitter` trait imported so `AppHandle::emit` resolves (the v1→v2 guard), command registered in `generate_handler!`, lib/bin split with `spectty_lib`, capabilities allow the frontend `listen`. UI uses v2 import paths `@tauri-apps/api/core` + `/event`.
- **PersistencePort corrected contract**: verified FINAL shape `&self` + `get -> Result<Option<String>, PersistenceError>` with `PersistenceError::Backend(String)` only (NotFound removed). Matches design obs 771 correction exactly.
- **Scope discipline**: NO scope creep. No PTY, no AgentRunner, no AgentStatus transitions, no real engram HTTP, no GitPort/NotifierPort, no xterm. EngramAdapter is a pure `todo!()` skeleton. Operational additions (deny.toml license/advisory blocks, CI caching/concurrency) are gate hardening, not architecture.
- **Test integrity**: tests genuinely assert behavior — round-trip asserts `Some(payload)`, missing-key asserts `Ok(None)` (not just no-panic), Arc test exercises the dyn port. Vitest mocks the real v2 API surface and asserts `invoke("ping")` + `listen("pong", fn)` + payload propagation. No vacuous/always-true assertions found.

## Verdict
**M0 is DONE.** All 14 spec requirements pass against the actual tree, every CI gate is green when re-run locally, and the hexagonal quarantine — M0's thesis — was independently proven via a RED/GREEN cargo-deny reproduction by the verifier. The 3 warnings are pre-first-commit operational items (commit lockfiles, validate sccache, trim an M1 doc note); none block archive. Recommend proceeding to **sdd-archive**, with W1 (commit lockfiles) addressed in the first commit so CI goes green on push.
