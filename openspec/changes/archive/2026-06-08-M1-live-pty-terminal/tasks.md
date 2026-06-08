# M1 — Live PTY Terminal — Task Checklist

> SDD tasks phase. Consumes `sdd/M1-live-pty-terminal/spec` (obs #785) and
> `sdd/M1-live-pty-terminal/design` (obs #786). Artifact store: HYBRID
> (engram `sdd/M1-live-pty-terminal/tasks` + this file).
>
> **Strict TDD is ACTIVE.** Test runners: `cargo test --workspace` (Rust),
> `pnpm -C ui test` (= `vitest run`, confirmed in `ui/package.json`). Every code
> work unit pairs its RED test with its GREEN implementation in the SAME unit:
> write the failing test first, then make it pass, then refactor. Do NOT batch
> tests at the end.
>
> Tasks are grouped into **work units** (WU). Each WU = one reviewable logical
> commit (Conventional Commit) per the work-unit-commits skill: clear start/finish,
> verification in the same unit, rollback that does not remove unrelated work.
>
> **Spec traceability tag** per task: `[REQ:<capability>/<short>]` maps to a spec
> Requirement. Verification class carried from spec: `[unit]` / `[manual]` / `[ci]`.
>
> **Parallelism**: WU-1 is the prerequisite for all Rust WUs. WU-2/WU-3 can run in
> parallel after WU-1. WU-4 depends on WU-3 (transport seam) + WU-2 (Coalescer).
> WU-5/WU-6 (UI) depend only on WU-1's UI-deps half and can run in parallel with the
> Rust WUs. WU-7 (manual acceptance) is last and depends on ALL prior WUs landing.

```
WU-1 (deps) ──┬── WU-2 (Coalescer) ──┐
              ├── WU-3 (transport+adapter) ──┴── WU-4 (commands+registry+wiring) ──┐
              └── WU-5 (ui deps) ── WU-6 (hook+component+App wiring) ───────────────┴── WU-7 (manual acceptance)
```

---

## WU-1 — Dependency & manifest wiring  [ci]
**Commit**: `chore(deps): add portable-pty to adapters + src-tauri and @xterm/* to ui`
**Depends on**: nothing. **Blocks**: WU-2, WU-3, WU-4 (Rust build); WU-5, WU-6 (UI build).
**Rollback**: revert this commit → no PTY deps; M0 still builds.

- [x] 1.1 Add `portable-pty = "0.9.0"` to `crates/adapters/Cargo.toml`. `[REQ:hexagonal-core/core-unchanged]` `[ci]` (PR1)
- [x] 1.2 Add `portable-pty = "0.9.0"` to `src-tauri/Cargo.toml`. `[REQ:hexagonal-core/core-unchanged]` `[ci]` (PR2)
- [x] 1.3 Confirm `crates/core/Cargo.toml` is UNCHANGED (still `serde` + `thiserror` only) — the gate. `[REQ:hexagonal-core/core-unchanged]` `[ci]` (PR1)
- [x] 1.4 Confirm `deny.toml` is UNCHANGED (portable-pty is not in the core-scoped closure). `[REQ:hexagonal-core/cargo-deny-green]` `[ci]` (PR1)
- [x] 1.5 Add `@xterm/xterm ^6.0.0`, `@xterm/addon-fit ^0.11.0`, `@xterm/addon-clipboard ^0.2.0` to `ui/package.json` `dependencies`; install lockfile. `[REQ:terminal-ui/xterm-mounted]` `[ci]` (PR3 — done)
- [ ] **Gate (WU-1)**: `cargo build --workspace` succeeds; `cargo deny --manifest-path crates/core/Cargo.toml check bans` exits 0; `pnpm -C ui install` resolves the 3 scoped deps; `pnpm -C ui build` typechecks.

---

## WU-2 — Pure Coalescer (hybrid size-OR-time batcher)  [unit]
**Commit**: `feat(adapters): add pure size-OR-time PTY output coalescer with tests`
**Depends on**: WU-1. **Blocks**: WU-4 (read-loop wiring uses it).
**Strict TDD**: RED tests first (all 5), then implement `Coalescer` until green, then refactor for hot-loop discipline.
**Rollback**: revert → no coalescer; adapter pty module not yet wired (WU-3 independent).

- [x] 2.1 Create `crates/adapters/src/pty/mod.rs` (module skeleton, doc comment) and add `pub mod pty;` to `crates/adapters/src/lib.rs`. `[REQ:pty-adapter/coalesces-output]` `[unit]` (PR1)
- [x] 2.2 RED: write `coalescer_flushes_when_size_threshold_reached` — `push` of `max_chunk + k` bytes returns a chunk of exactly `max_chunk`, remainder buffered. `[REQ:pty-adapter/coalesces-output → Scenario: size threshold]` `[unit]` (PR1)
- [x] 2.3 RED: write `coalescer_splits_oversized_push_at_max_chunk` — push ≫ `max_chunk` returns an exact `max_chunk` chunk, rest kept buffered. `[REQ:pty-adapter/coalesces-output → Scenario: size threshold (max-chunk split)]` `[unit]` (PR1)
- [x] 2.4 RED: write `coalescer_does_not_flush_below_size_and_time` — small `push` under interval and under size → `None`. `[REQ:pty-adapter/coalesces-output → Scenario: empty/below-threshold no flush]` `[unit]` (PR1)
- [x] 2.5 RED: write `coalescer_drain_due_flushes_after_interval` — inject `now = last_flush + interval` → buffered bytes returned; before interval → `None` (deterministic via injected `Instant`). `[REQ:pty-adapter/coalesces-output → Scenario: time tick]` `[unit]` (PR1)
- [x] 2.6 RED: write `coalescer_drain_all_flushes_remainder_on_eof`; and assert `drain_due`/`drain_all` on an empty buffer return `None` (no empty chunk). `[REQ:pty-adapter/coalesces-output → Scenario: empty buffer flushes nothing]` `[unit]` (PR1)
- [x] 2.7 GREEN: implement `crates/adapters/src/pty/coalescer.rs` — `Coalescer::new(max_chunk, flush_interval, now)`, `#[must_use] push(&mut self, &[u8], now) -> Option<Vec<u8>>`, `#[must_use] drain_due(now)`, `#[must_use] drain_all()`. Inject `Instant` via `now` params (no `sleep`). `[REQ:pty-adapter/coalesces-output]` `[unit]` (PR1)
- [x] 2.8 REFACTOR: reuse `buf` across pushes; only allocation is the returned chunk (`split_off`/`mem::take`); satisfy `clippy::redundant_clone` and `-D warnings`. Re-export `Coalescer` from `pty/mod.rs`. `[REQ:hexagonal-core/clippy-hot-path]` `[ci]` (PR1)
- [ ] **Gate (WU-2)**: `cargo test --workspace` green (all 5 coalescer tests); `cargo fmt --check`; `cargo clippy --all-targets --all-features --locked -- -D warnings`.

---

## WU-3 — Pure spawn config + PtyTransport seam + PtyAdapter  [unit]
**Commit**: `feat(adapters): add PtySpawnConfig, PtyTransport seam, and portable-pty adapter`
**Depends on**: WU-1. **Can run in parallel with WU-2.** **Blocks**: WU-4 (commands test against the seam).
**Strict TDD**: pure builder + default_shell get RED tests first; `PtyAdapter` (real portable-pty) has NO unit test (covered by manual acceptance + the command-layer fake in WU-4).
**Rollback**: revert → no transport/adapter; coalescer (WU-2) stands alone.

- [x] 3.1 RED: write `default_shell_prefers_env_shell` and `default_shell_falls_back_when_unset` using an injected env getter (`impl Fn(&str) -> Option<String>`). `[REQ:pty-adapter/spawns-shell → Scenario: default shell per OS]` `[unit]` (PR1)
- [x] 3.2 RED: write `pty_spawn_config_shell_sets_program_and_size` — `PtySpawnConfig::shell(cols, rows, cwd)` yields program/cwd/size matching inputs, asserted on the pure struct (NOT portable-pty internals). `[REQ:pty-adapter/spawns-shell → Scenario: spawn-input construction]` `[unit]` (PR1)
- [x] 3.3 GREEN: implement `crates/adapters/src/pty/config.rs` — `PtySpawnConfig { program, args, cwd, cols, rows }` (`Debug,Clone,PartialEq,Eq`), `PtySpawnConfig::shell(...)`, `default_shell(get_env)` ($SHELL→/bin/bash unix; %COMSPEC%/cmd.exe windows). MUST NOT be named/shaped as an agent type (no `LaunchSpec`/`AgentSpec`). `[REQ:pty-adapter/spawns-shell]` `[unit]` (PR1)
- [x] 3.4 GREEN: implement `crates/adapters/src/pty/transport.rs` — `pub trait PtyTransport: Send { write(&mut, &[u8]); resize(&mut, u16, u16); kill(&mut); }` all `-> Result<(), PtyError>`; object-safe; NOT a Core port. `[REQ:pty-adapter/accepts-input-resize-kill]` `[unit]` (PR1)
- [x] 3.5 GREEN: implement `crates/adapters/src/pty/adapter.rs` — `PtyError` (thiserror enum: Open/Spawn/Io/Resize/UnknownId/Poisoned), `PtyAdapter { master, writer, child }`, `PtyAdapter::spawn(cfg) -> Result<(Self, Box<dyn Read + Send>), PtyError>` (native_pty_system → openpty → CommandBuilder → take_writer + try_clone_reader), and `impl PtyTransport for PtyAdapter` (write_all / master.resize(PtySize) raising SIGWINCH / child.kill). `[REQ:pty-adapter/accepts-input-resize-kill]` `[REQ:pty-adapter/spawns-shell]` `[unit]/[manual]` (PR1; PtyAdapter has no unit test by design — covered by WU-4 fake + manual acceptance)
- [x] 3.6 Re-export from `crates/adapters/src/lib.rs`: `pub use pty::{PtyAdapter, PtySpawnConfig, Coalescer, PtyTransport, PtyError};`. `[REQ:pty-adapter/spawns-shell]` `[unit]` (PR1)
- [ ] **Gate (WU-3)**: `cargo test --workspace` green (config + default_shell tests); `cargo fmt --check`; `cargo clippy ... -- -D warnings`; `cargo deny --manifest-path crates/core/Cargo.toml check bans` exits 0 (Core still clean).

---

## WU-4 — src-tauri commands + registry state + read-thread/Channel wiring  [unit]
**Commit**: `feat(tauri): add PTY lifecycle commands, registry state, and Channel output wiring`
**Depends on**: WU-2 (Coalescer) + WU-3 (PtyTransport/PtyAdapter/PtyError). **Blocks**: WU-6 wiring is independent but WU-7 acceptance needs this.
**Strict TDD**: command-layer behavior tested against a `FakePtyTransport` (RED first); commands refactored to act on `&mut dyn PtyTransport` via the registry so the fake substitutes. Real PTY NOT opened in tests.
**Rollback**: revert → commands/registry gone; adapters crate (WU-2/3) still compiles standalone.

- [x] 4.1 Create `src-tauri/src/pty_state.rs` — `PtyId = String`, `PtyState { transport: Box<dyn PtyTransport>, stop: Arc<AtomicBool>, reader_thread: Option<JoinHandle<()>> }`, `#[derive(Default)] PtyRegistry(pub Mutex<HashMap<PtyId, PtyState>>)`. Registry-SHAPED; MUST NOT import a Core `SessionRegistry`. `[REQ:pty-bridge/registry-shaped-state → Scenario: keyed by PTY id]` `[unit]` (PR2) — NOTE: `transport` is `Box<dyn PtyTransport>` (not a concrete `adapter: PtyAdapter`) so command tests substitute a fake without a real PTY.
- [x] 4.2 RED: write command-layer tests against a `FakePtyTransport` (records calls): `send_input_writes_bytes_to_transport`, `pty_resize_forwards_cols_rows`, `pty_kill_invokes_transport_kill_and_removes_entry`, `send_input_unknown_id_returns_err`. `[REQ:pty-adapter/accepts-input-resize-kill → Scenario: command-layer via fake]` `[REQ:pty-bridge/lifecycle-commands]` `[unit]` (PR2)
- [x] 4.3 GREEN: implement `src-tauri/src/commands/pty.rs` command bodies (`send_input` / `pty_resize` / `pty_kill`) delegating to free `*_impl` fns that look the transport up by id in the registry (`&mut dyn PtyTransport`); unknown id → `Err`. Errors map via `.map_err(|e| e.to_string())` (ping convention); mutex lock → `.map_err(|_| "pty registry mutex poisoned".to_string())?` (no panic at command boundary). `[REQ:pty-bridge/lifecycle-commands]` `[REQ:pty-bridge/registry-shaped-state]` `[unit]` (PR2)
- [x] 4.4 GREEN: implement `pty_spawn` (`async`, owned types only — `cols: u16, rows: u16, cwd: Option<String>, on_output: Channel<Vec<u8>>, registry: State<'_, PtyRegistry>) -> Result<PtyId, String>`): open `PtyAdapter`, mint id (monotonic counter), insert `PtyState`, return id. `[REQ:pty-bridge/lifecycle-commands → Scenario: pty_spawn returns id]` `[unit]/[manual]` (PR2)
- [x] 4.5 GREEN: spawn the dedicated read thread (`std::thread::Builder::new().name("pty-read-{id}")`, ADR-3 deviation from `spawn_blocking`, rationale comment carried): reused `[0u8; READ_BUF]`, `Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, Instant::now())`, `push` + `drain_due` per read → `on_output.send(chunk)`; `Ok(0)`/error → `drain_all` then break; handle `ErrorKind::Interrupted` continue. Module consts `READ_BUF`/`MAX_CHUNK`/`FLUSH_INTERVAL`. `[REQ:pty-adapter/streams-bytes-off-thread]` `[REQ:pty-adapter/coalesces-output]` `[unit]/[manual]` (PR2) — NOTE: PR5 (R3 fix) later split this into a read thread + an mpsc + a forwarder driven by `recv_timeout` so the time-flush fires while the PTY is quiescent.
- [x] 4.6 GREEN: output transport = raw `Vec<u8>` over `ipc::Channel` (NO `number[]`, NO base64); `PtyExit { id, code: Option<i32> }` (`Clone, serde::Serialize`) emitted via `app.emit("pty_exit", ...)` using the v2 `Emitter` trait on thread exit. `[REQ:pty-bridge/channel-output-event-exit → Scenario: Channel vs event]` `[REQ:pty-adapter/streams-bytes-off-thread → Scenario: raw bytes]` `[unit]/[manual]` (PR2)
- [x] 4.7 GREEN: `pty_kill` lifecycle shutdown — `PtyState::shutdown()` sets `stop` (one-shot latch, idempotent), `transport.kill()` (closes master → read EOF), `join()` the `reader_thread`, and `kill_impl` removes the registry entry; `Drop for PtyState` calls the same `shutdown()` best-effort (no leaked thread, no double-kill). `[REQ:pty-adapter/accepts-input-resize-kill]` `[unit]/[manual]` (PR2)
- [x] 4.8 Register all four commands in `generate_handler!` and `.manage(PtyRegistry::default())` in `src-tauri/src/lib.rs`; added `pub mod pty;` to `src-tauri/src/commands/mod.rs` and `pub mod pty_state;` to `lib.rs`. `[REQ:pty-bridge/lifecycle-commands → Scenario: all four registered]` `[unit]` (PR2)
- [x] 4.9 Verified `capabilities/default.json` is UNCHANGED — `core:default` covers custom invoke + Channel; `core:event:default` covers `pty_exit` (delta: NONE, confirmed). `[REQ:pty-bridge/channel-output-event-exit]` `[ci]/[manual]` (PR2)
- [x] 4.10 W1 CLOSURE (PR1 verify report): added `real_pty_streams_output_and_accepts_resize_write_kill` (`#[cfg(unix)]`) — opens a REAL PTY via `PtyAdapter::spawn` running `/bin/sh -c "printf SPECTTY_PTY_OK"`, drives the actual read/Coalescer loop, asserts the marker bytes arrive, and asserts real `resize(100,30)`/`write`/`kill` succeed against the live master. Closes the CI gap where only the fake seam was exercised. `[REQ:pty-adapter/accepts-input-resize-kill]` `[unit/ci]` (PR2)
- [x] **Gate (WU-4)**: `cargo test --workspace` green (17 tests: 4 command-fake + 1 real-PTY + 12 prior); `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` 0 warnings (forced rebuild); `cargo deny --manifest-path crates/core/Cargo.toml check bans` `bans ok`; `cargo build -p spectty` succeeds.

---

## WU-5 — UI PTY IPC wrappers (typed seam)  [unit]
**Commit**: `feat(ui): add typed PTY IPC wrappers (spawn/send/resize/kill + Channel)`
**Depends on**: WU-1 (UI deps). **Can run in parallel with the Rust WUs.** **Blocks**: WU-6.
**Strict TDD**: thin wrappers are exercised through the hook tests in WU-6; this WU is small and may be merged into WU-6's commit if it stays trivial — keep separate only if it grows.
**Rollback**: revert → no ipc wrappers; hook (WU-6) not yet present.

- [x] 5.1 Create `ui/src/pty/ipc.ts` — typed wrappers `spawnPty(cols, rows, cwd, onOutput): Promise<string>`, `sendInput(id, data: Uint8Array)`, `resizePty(id, cols, rows)`, `killPty(id)`, `createOutputChannel(onBytes)` builder + `decodeChannelBytes(message)` (R1 number[]→Uint8Array decoder). Camel/snake mapping: `onOutput` ↔ `on_output`. `[REQ:terminal-ui/xterm-mounted]` `[unit]` (PR3 — done; +7 ipc unit tests)
- [x] **Gate (WU-5)**: `pnpm -C ui build` typechecks; ipc wrappers + decoder covered by `tests/unit/ipc.test.ts` (7 tests green).

---

## WU-6 — useTerminal hook + Terminal component + App wiring  [unit]
**Commit**: `feat(ui): add xterm Terminal pane with useTerminal hook (spawn/IO/resize/kill)`
**Depends on**: WU-5 (ipc wrappers) + WU-1 (xterm deps). **Blocks**: WU-7.
**Strict TDD**: RED vitest first (mirror `usePingPong.test.ts`), mocking `@tauri-apps/api/core` (invoke + fake Channel), `@tauri-apps/api/event` (listen), and `vi.mock('@xterm/xterm')` (fake Terminal: open/write/onData/dispose) + fake `FitAddon`. Then implement the hook green.
**Rollback**: revert → no Terminal pane; App returns to M0 ping view.

- [x] 6.1 RED: write `ui/tests/unit/useTerminal.test.ts` with mocks above. `[REQ:terminal-ui/xterm-mounted]` `[unit]` (PR3 — done)
- [x] 6.2 RED: spawn-on-mount — mount invokes `pty_spawn` with a Channel (`onOutput`) + initial cols/rows. `[REQ:terminal-ui/xterm-mounted → Scenario: spawns on mount]` `[unit]` (PR3)
- [x] 6.3 RED: channel-bytes→write — fire fake channel message (`number[]`) → `term.write` called with the decoded `Uint8Array`. `[REQ:terminal-ui/xterm-mounted → Scenario: Channel output written]` `[unit]` (PR3 — proves R1 decode)
- [x] 6.4 RED: onData→send_input — `term.onData` yields data → `send_input` invoked with id. `[REQ:terminal-ui/xterm-mounted → Scenario: keystrokes forward]` `[unit]` (PR3)
- [x] 6.5 RED: fit→pty_resize — ResizeObserver fires → `fit()` runs → `pty_resize` invoked with `term.cols/term.rows`. `[REQ:terminal-ui/tracks-resize-via-fit → Scenario: fit drives pty_resize]` `[unit]` (PR3)
- [x] 6.6 RED: unmount→dispose+kill — unmount → `term.dispose()` AND `pty_kill` invoked. `[REQ:terminal-ui/xterm-mounted → Scenario: tears down on unmount]` `[unit]` (PR3)
- [x] 6.7 GREEN: implement `ui/src/hooks/useTerminal.ts` (mirrors `usePingPong`): `Terminal({ scrollback: SCROLLBACK=5000, convertEol: false, cursorBlink: true })`, `FitAddon` + `ClipboardAddon`, `term.open(container)`, `fit()`, `createOutputChannel`→`term.write`, `spawnPty` with initial fit size (async, disposed-race guard kills late-resolved id), `term.onData`→`sendInput`, `ResizeObserver`→`fit()`→`resizePty`, `listen("pty_exit")`. Cleanup: disconnect observer, dispose onData, unlisten, `term.dispose()`, `killPty(id)`. React 19 named imports; NO manual `useMemo`/`useCallback`/`forwardRef`. `[REQ:terminal-ui/xterm-mounted]` `[REQ:terminal-ui/tracks-resize-via-fit]` `[REQ:terminal-ui/retains-scrollback]` `[unit]/[manual]` (PR3)
- [x] 6.8 GREEN: implement `ui/src/components/Terminal.tsx` — `useRef` containerRef, `useTerminal(containerRef)`, render `<div ref={containerRef} className="terminal-pane" />`; `import "@xterm/xterm/css/xterm.css";` once. `[REQ:terminal-ui/xterm-mounted]` `[unit]/[manual]` (PR3)
- [x] 6.9 GREEN: wire `<Terminal />` into `ui/src/App.tsx` as the primary view (M0 ping REMOVED — single full-window terminal for M1; `usePingPong` hook+test deleted, ping wiring gone). Added `ui/src/styles.css` (`.app` flex column + `.terminal-pane` flex:1/min-height:0) imported in `main.tsx` so `ResizeObserver`/`fit` has a real box. `[REQ:terminal-ui/xterm-mounted]` `[manual]` (PR3)
- [x] **Gate (WU-6)**: `pnpm -C ui test` green (12 tests: 7 ipc + 5 hook); `pnpm -C ui build` typechecks (TS strict) and bundles (xterm.css emitted).

---

## WU-7 — Manual acceptance checklist (roadmap exit gate)  [manual]
**Commit**: `docs(m1): record M1 manual acceptance results` (or recorded in apply-progress; NOT unit-testable)
**Depends on**: ALL prior WUs landed (full slice running). This is the `sdd-verify` pass/fail gate.
**Rollback**: n/a (verification artifact only).

> These map verbatim to the roadmap M1 exit criteria. They CANNOT be unit-tested.
> Run the real app (`pnpm tauri dev` / built app) on macOS (gating). Windows/ConPTY
> is best-effort and MUST NOT block.
>
> **ARCHIVE NOTE (2026-06-08)**: this manual acceptance was NOT re-confirmed in the running
> app before archive. The user explicitly chose to archive M1 on the strength of the automated
> gates + 4 adversarial sdd-verify passes + green CI, and will run the manual re-test separately.
> Recorded as a user-deferred KNOWN-OPEN ITEM in archive-report.md — NOT silently omitted.

- [ ] 7.1 `vim` opens, edits, and exits cleanly (alt-screen, cursor keys, colors). `[REQ:roadmap-exit/criterion-1]` `[REQ:terminal-ui/renders-ansi]` `[manual]`
- [ ] 7.2 `htop` renders and refreshes without tearing/lag — validates Coalescer batching cadence. `[REQ:roadmap-exit/criterion-1]` `[manual]`
- [ ] 7.3 `git log --oneline --graph` renders colors and graph correctly. `[REQ:roadmap-exit/criterion-1]` `[manual]`
- [ ] 7.4 Resize the window → xterm re-fits AND the PTY receives the new size (SIGWINCH) so the running program reflows. `[REQ:roadmap-exit/criterion-2]` `[REQ:pty-adapter/accepts-input-resize-kill → Scenario: resize SIGWINCH]` `[manual]`
- [ ] 7.5 Scrollback retained after output exceeds the viewport (up to `SCROLLBACK`=5000). `[REQ:roadmap-exit/criterion-3]` `[REQ:terminal-ui/retains-scrollback]` `[manual]`
- [ ] 7.6 Copy selected text → reaches the system clipboard; paste → delivered to the PTY (OSC 52, no extra Tauri capability). `[REQ:terminal-ui/copy-paste]` `[manual]`
- [ ] 7.7 Shell exits → `pty_exit` event observed by the UI; pane marks closed. `[REQ:pty-bridge/channel-output-event-exit → Scenario: PTY exit emits event]` `[manual]`
- [ ] 7.8 (best-effort, ungated) Windows ConPTY smoke if a Windows host is available — failure does NOT block M1. `[REQ:cross-platform/macos-gating-windows-best-effort]` `[manual]`
- [ ] **Gate (WU-7)**: all macOS criteria (7.1–7.7) pass → M1 acceptance PASS; record results for `sdd-verify`. Windows (7.8) informational only.

---

## Cross-cutting gates (apply to every code WU)
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace`
- `cargo deny --manifest-path crates/core/Cargo.toml check bans` (core scope stays green)
- `pnpm -C ui test` (= `vitest run`)
- `pnpm -C ui build`
- VibeLens: after edits in a WU, call `show_diff_explanation` with that WU's `git diff HEAD` (per project CLAUDE.md) — apply-phase obligation, not a commit.

---

## Review Workload Forecast

**Estimated total changed lines** (additions + deletions, approximate):
- Rust — adapters PTY module (coalescer/config/transport/adapter/mod) + tests: ~330
- Rust — src-tauri (pty_state, commands/pty, wiring, command-fake tests): ~300
- Config — Cargo.toml x2, package.json + lockfile, capabilities (no-op verify): ~40 (+ lockfile churn excluded from review budget)
- TS — ipc.ts + useTerminal.ts + Terminal.tsx + App wiring + CSS: ~230
- TS — useTerminal.test.ts: ~150
- **Total reviewable: ~1050 changed lines** (excluding generated lockfile churn).

**Chained PRs recommended: Yes**
**400-line budget risk: High**
**Decision needed before apply: Yes** (delivery_strategy in effect = `ask-on-risk` → orchestrator MUST stop and ask).

**Proposed PR-split boundary** (respects work-unit commit boundaries; each slice lands independently and is reviewable in ≤60 min):
- **PR1 — Rust PTY core + bridge** (`WU-1` Rust half + `WU-2` + `WU-3` + `WU-4`): portable-pty deps, pure Coalescer, spawn config + transport seam + adapter, src-tauri commands + registry + read-thread/Channel wiring + command-fake tests. ~630 lines. Self-contained: ships and tests behind the fake/cargo gates with NO UI. Over 400 → if not split further, candidate stacked sub-slice: **PR1a** = adapters (`WU-2`+`WU-3`, ~330) / **PR1b** = src-tauri commands+wiring (`WU-4`, ~300, depends on PR1a).
- **PR2 — UI terminal pane** (`WU-1` UI half + `WU-5` + `WU-6`): @xterm deps, ipc wrappers, useTerminal hook + tests, Terminal component, App wiring, CSS. ~420 lines. Depends on PR1's commands existing (compiles/tests standalone via mocks; runtime acceptance needs PR1 merged).
- **PR3 — M1 acceptance** (`WU-7`): manual acceptance run + recorded results; effectively a verify/doc PR, ~0 code lines. Depends on PR1 + PR2 merged.

Recommended chain (if user approves chaining): `stacked-to-main` if PR1a/PR1b/PR2 can each land independently; `feature-branch-chain` with a tracker if M1 must integrate as one unit before main. Orchestrator to confirm `chain_strategy` if chaining is chosen. If the user prefers a single PR, this run requires a recorded `size:exception` (~1050 lines ≫ 400).

> **ACTUAL DELIVERY (archive record)**: shipped as PR1 (adapters, merged #2), PR2 (src-tauri bridge,
> merged #3), PR3 (terminal UI, merged #4), and PR5 (R3 quiescent-flush fix, branch
> `fix/m1-pty-quiescent-flush`, committed `b5114f9`). "PR4" was the manual-acceptance gate (no code)
> that caught R3. WU-1..WU-6 complete; WU-7 manual acceptance user-deferred at archive.
