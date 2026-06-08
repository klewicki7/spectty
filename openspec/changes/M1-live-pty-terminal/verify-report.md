# M1 — Live PTY Terminal — Verify Report (PR1 ONLY)

> SDD verify phase. Scope: **PR1 only** = the `crates/adapters` PTY slice
> (WU-1 rust deps, WU-2 Coalescer, WU-3 config+transport+adapter) on branch
> `feat/m1-pty-adapters`. src-tauri/ui intentionally untouched (PR2/PR3 — NOT in
> this verification's pass/fail). Verified adversarially from source against
> spec (obs#785) and design (obs#786); apply report (obs#788) was re-checked, not
> trusted. M0 base: `622270a`.

## Overall verdict: PASS (PR1 is pushable)

0 CRITICAL, 1 WARNING, 3 SUGGESTION. No blocking issues for PR1.

## Gate report (each re-run from source)

| Gate | Command | Result |
|------|---------|--------|
| fmt | `cargo fmt --all -- --check` | PASS (exit 0, no diff) |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | PASS (0 warnings; re-confirmed after `cargo clean -p spectty-adapters` forced full rebuild) |
| test | `cargo test --workspace` | PASS — 12 tests (3 persistence M0 + 5 coalescer + 3 config + 1 transport), 0 failed |
| deny | `cargo deny --manifest-path crates/core/Cargo.toml check bans` | PASS ("bans ok") |

## Spec requirement coverage (PR1-applicable only)

| Requirement (spec §pty-adapter / §hexagonal-core) | Status | Evidence |
|---|---|---|
| Spawn shell in real PTY; spawn-input is minimal (NOT agent-shaped) [unit] | MET | `PtySpawnConfig{program,args,cwd,cols,rows}` — no LaunchSpec/AgentSpec; `PtyAdapter::spawn` via `native_pty_system().openpty`→`spawn_command`. Config-build asserted on the struct, no portable-pty internals (config.rs test). |
| Default shell per-OS fallback [unit] | MET | `default_shell(get_env)`: unix `$SHELL`→`/bin/bash`, win `%COMSPEC%`→`cmd.exe`. 2 injected-env tests (prefers env / falls back). |
| Output as raw bytes, dedicated std::thread (NOT spawn_blocking), no number[]/base64 [unit] | PARTIAL-by-design | Reader-handoff seam present: `spawn` returns `(Self, Box<dyn Read + Send>)` separately-cloned reader; doc states dedicated `std::thread` ownership. The thread + Channel<Vec<u8>> land in PR2 (correct scoping). |
| Coalescer hybrid size-OR-time flush, 3 pure scenarios + no-empty + split [unit] | MET | size-flush, oversized-split, below-threshold no-flush, time-tick (injected Instant), drain_all-EOF + empty=None. 5 genuine byte-value assertions, no tautologies. |
| write/resize(SIGWINCH)/kill via internal PtyTransport seam, fake-tested, NOT a Core port [unit] | MET | `PtyTransport` trait (Send, object-safe), recording-fake test proves substitutability + forwarding. Real adapter impl present; cols/rows mapping correct (no transposition). |
| hexagonal-core delta: Core unchanged, no portable-pty/tokio/tauri, deny green, clippy clean [ci] | MET | core/Cargo.toml + deny.toml byte-identical vs 622270a; `cargo tree -p spectty-core` shows NO portable-pty; adapters closure has it; no pty/PtyPort in core/src; clippy 0 warnings. |

Bridge/UI requirements (pty-bridge, terminal-ui, manual acceptance) = PR2/PR3/PR4 — correctly NOT implemented here, out of PR1 pass/fail.

## Findings

### WARNING
- **W1 — Real adapter resize/write/kill cols/rows mapping is untested in CI.** The
  `PtyTransport` *fake* test proves the seam shape, but the REAL `PtyAdapter::resize`
  (cols→PtySize.cols, rows→PtySize.rows) has no unit test; a transposition bug would
  only surface at manual acceptance (PR4), not in CI. Mapping is currently CORRECT
  (verified by inspection: adapter.rs:127-137 and spawn 79-84). Deliberate PR1 scoping
  (apply report acknowledges "covered by WU-4 fake + manual acceptance"), so non-blocking
  for PR1 — but PR2/PR4 MUST exercise the real resize path. Not a defect today.

### SUGGESTION
- **S1 — `#[must_use]` on the spawn handle is satisfied transitively, not explicitly.**
  Spec §clippy-guard says "the spawn handle is `must_use`". `PtyAdapter::spawn` returns
  `Result<(Self, Box<dyn Read+Send>), PtyError>`; `Result` is already `#[must_use]` in std,
  so dropping it triggers `unused_must_use`. The intent is MET; an explicit attribute would
  be redundant on a `Result` return. No action required.
- **S2 — Coalescer `push` drains at most ONE max_chunk per call.** A single push of
  >2×max_chunk buffers the remainder (>max_chunk) until the next push/drain. This is
  design-conformant (design.md:179-180,446 specify exactly this) and safe in the real read
  loop (READ_BUF ≈ max_chunk so a read can't strand >~max_chunk). Worth a one-line doc note
  on `push` that oversized single pushes leave >max_chunk buffered, for future callers.
- **S3 — VibeLens `show_diff_explanation` was not invoked during apply** (tool absent in that
  context, per apply report). CLAUDE.md per-edit explanation contract unmet for PR1; run it
  before opening the PR or note the exception.

### Verified non-issues (adversarial checks that passed)
- No `unwrap`/`expect`/`panic`/`todo!` in production pty code (3 `.expect` are all inside
  `#[cfg(test)]`).
- No needless clone/`to_vec` in coalescer/adapter hot path; buffer reused via `split_off`/`mem::take`/`mem::replace`.
- Manual `Debug` for `PtyAdapter` is sound (`dyn MasterPty`/`dyn Write` aren't Debug; prints `child` + `finish_non_exhaustive`).
- ADR-3 deviation (dedicated std::thread vs spawn_blocking) is DELIBERATE and DOCUMENTED
  (design.md:62-66, "called out for verify") — not an accident.
- src-tauri and ui are byte-identical to M0 (empty diff) — correct PR1 scoping, no PR2/PR3 scope leaked.
- Tests are genuine: concrete byte/value assertions, none would pass with a wrong impl.

## Next recommended
PR1 is clean → proceed. Address S3 (VibeLens) before opening the PR per CLAUDE.md.
PR2 (src-tauri commands + dedicated read thread + Channel) is the next apply slice; it MUST
include the real resize-path coverage flagged in W1.

---

# M1 — Live PTY Terminal — Verify Report (PR2 — src-tauri PTY bridge)

> SDD verify phase, **second pass**. Scope: **PR2 only** = the `src-tauri` PTY
> bridge (WU-4): `PtyRegistry` state, the 4 commands
> (`pty_spawn`/`send_input`/`pty_resize`/`pty_kill`), the dedicated `std::thread`
> read loop streaming raw bytes over `ipc::Channel<Vec<u8>>` via the Coalescer,
> and the `pty_exit` event. Branch `feat/m1-pty-bridge` (PR1 already merged to
> main as #2). UI intentionally untouched (PR3 — NOT in this pass/fail).
> Verified adversarially from source against spec (obs#785), design (obs#786),
> tasks (obs#787); apply report (obs#788) and bugfix (obs#791) re-checked, not
> trusted. Base: main `f190233`.

## Overall verdict: PASS (PR2 is pushable)

**0 CRITICAL, 1 WARNING, 2 SUGGESTION.** No blocking issues. PR2 closes PR1's W1.

## Gate report (each re-run from source)

| Gate | Command | Result |
|------|---------|--------|
| fmt | `cargo fmt --all -- --check` | PASS (exit 0, no diff) |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | PASS — 0 warnings. Cache-defeated: `cargo clean -p spectty` alone served a stale green (1.98s "Finished", no recompile), so source files were `touch`ed to force a genuine re-check (`Checking spectty`, exit 0). |
| test | `cargo test --workspace` | PASS — **17 tests** (5 src-tauri + 12 adapters/persistence), 0 failed. Real-PTY test ran in-band, finished <0.01s, no hang. |
| deny | `cargo deny --manifest-path crates/core/Cargo.toml check bans` | PASS ("bans ok") |
| tree | `cargo tree -p spectty-core` | PASS — NO portable-pty/tauri/tokio in core closure (quarantine HOLDS) |
| build | `cargo build -p spectty` | PASS — Tauri app compiles |

The 5 src-tauri tests, all named and run (none `#[ignore]`d):
`send_input_writes_bytes_to_transport`, `pty_resize_forwards_cols_rows`,
`pty_kill_invokes_transport_kill_and_removes_entry`, `send_input_unknown_id_returns_err`
(the 4 fake-transport tests) + `real_pty_streams_output_and_accepts_resize_write_kill`
(the `#[cfg(unix)]` W1-closure real-PTY test).

## W1 closure — CONFIRMED

PR1's W1 ("real adapter resize/write/kill cols/rows mapping untested in CI") is
**CLOSED**. `real_pty_streams_output_and_accepts_resize_write_kill` (pty.rs:366-433):
opens a REAL pty via `PtyAdapter::spawn` running `/bin/sh -c "printf SPECTTY_PTY_OK"`,
then asserts ALL of:
- real `resize(100, 30)` succeeds against the live master (the exact W1 gap — cols/rows
  forwarded untransposed: command `(cols,rows)` → `transport.resize(cols,rows)` →
  adapter `PtySize{rows,cols}` named fields);
- real `write(b"")` round-trips through the live writer;
- the actual read/Coalescer loop collects output and the marker bytes `SPECTTY_PTY_OK` arrive;
- real `kill()` succeeds against the live child.

**CI-safety: SOUND.** Non-interactive `printf` that exits on its own → deterministic EOF;
no TTY interaction, no long-running/interactive program (vim/htop are PR4 manual only).
The child self-exits, so even if `kill` were a no-op the loop terminates on natural EOF.
Confirmed empirically: whole suite finished in <0.01s, no hang. `#[cfg(unix)]`-gated, so the
gating macOS/Linux CI runs it; Windows is best-effort/ungated by design.

## Deadlock audit (CRITICAL focus) — CONCLUSION: NO DEADLOCK

**Structurally impossible, not merely avoided.** The read-thread closure
(pty.rs:166-223) captures only `app`, `id`, `reader` (owned `Box<dyn Read>`),
`on_output` (the Channel), and `stop` (`Arc<AtomicBool>`). It **never** captures,
references, or locks the registry `Mutex` — verified by grepping the whole closure
body. The classic deadlock (killer holds the registry lock across `join()` while the
read thread blocks acquiring the same lock) cannot occur because the read thread has
no path to that lock.

Defense-in-depth is also correct: `kill_impl` (pty.rs:85-96) scopes the guard in an
inner block — `remove(id)` returns the owned `PtyState` and the guard drops at the
block's `}` (line 91) BEFORE `state.shutdown()`/`join()` runs (line 94). So the
registry lock is provably not held across the join even though that ordering is
belt-and-suspenders given the closure can't lock anyway.

**Kill→EOF unblock chain verified:** a thread parked in `reader.read()` does NOT observe
`stop` (checked only at loop top). It unblocks via `shutdown()` → `transport.kill()` →
`child.kill()` → child dies → slave closes → master reports EOF → `read()` returns `Ok(0)`
→ `drain_all` + `break` → thread exits → `join()` returns. The real-PTY test proves EOF
propagation works on this platform (read loop drains to `Ok(0)`).

## Double-kill / Drop idempotency — CONFIRMED SOUND

`shutdown()` (pty_state.rs:49-59) uses a one-shot latch: `if self.stop.swap(true, SeqCst) { return; }`
before `transport.kill()` + `join()`. An explicit `pty_kill` (which removes the entry then
calls `shutdown()`) followed by the removed `PtyState`'s `Drop` (which also calls `shutdown()`)
runs the teardown EXACTLY ONCE. This is the bugfix in obs#791 and is covered by the RED-caught
`pty_kill_invokes_transport_kill_and_removes_entry` test (asserts kills == 1). The latch matters
beyond the test: a second real `child.kill()` on an already-reaped child can return an OS error.

**No thread leak on any path:** kill (explicit `shutdown`+join), EOF (loop breaks naturally,
`Drop`'s `shutdown` swaps already-set-by-loop?  — see note), error (loop breaks, `Drop` joins),
Drop (joins via `shutdown`), app teardown (registry dropped → each `PtyState` dropped → joined).
NOTE: on natural EOF the LOOP exits on its own but the `stop` flag was NOT set by the loop, so a
later `Drop`/`shutdown` will `swap(true)` (first call) and additionally call `transport.kill()` on
the already-exited child (a benign best-effort `let _ =`), then `join()` the already-finished thread
(returns immediately). No leak, no panic — `kill` error is swallowed. Correct.

## Design deviation — JUSTIFIED

`PtyState.transport: Box<dyn PtyTransport>` instead of design's concrete `adapter: PtyAdapter`.
SOUND and design-faithful: the `PtyTransport` seam already exists in the adapters crate
(transport.rs) precisely to be substitutable; boxing it lets the command-layer free functions
operate on the trait object so the 4 fake-transport unit tests run with NO real pty. No capability
is lost — `PtyAdapter` is the production impl boxed in at spawn (pty.rs:121), and the real path
(resize reaching the live master) is proven by the W1 test. Acceptable deviation; flag noted.

## Channel / transport correctness — CONFIRMED

- Raw `Vec<u8>` over `ipc::Channel<Vec<u8>>` — NOT number[]/base64. `on_output.send(chunk)` sends
  byte vectors directly (pty.rs:187,194,199,210). ADR-2 honored.
- Coalescer wired correctly: `push` (size flush) + `drain_due` (time flush) per non-EOF read;
  `drain_all` on EOF and on read-error. No bytes dropped silently except by design (a Channel
  `send` error → `break`, i.e. the FE Channel is gone, nothing to deliver to).
- `pty_exit` emitted EXACTLY ONCE — single `app.emit("pty_exit", ...)` after the loop breaks
  (pty.rs:220), fires once per thread lifetime. `code: None` documented (exit code not retrievable
  from the read side without owning the child handle in M1).
- cols/rows NOT transposed anywhere: spawn `shell(cols,rows,..)`, command `resize(cols,rows)`,
  adapter `PtySize{rows,cols}` (named fields). Real-PTY test asserts `(120,40)` and `resize(100,30)`.

## Capabilities — CONFIRMED sufficient

`src-tauri/capabilities/default.json` byte-identical to main (empty diff). `core:default` covers
the 4 custom-command `invoke`s + the `ipc::Channel` IPC; `core:event:default` covers the `pty_exit`
event emit. No new capability needed — design's "delta NONE" prediction holds. No runtime-deny risk
for the new surface.

## Spec / scope coverage (PR2-applicable)

| Requirement (spec §pty-bridge / §hexagonal-core) | Status | Evidence |
|---|---|---|
| 4 commands registered in `generate_handler!`, owned types, Result error [unit] | MET | lib.rs:23-29 registers all 4 + `.manage(PtyRegistry::default())` (first managed state). `pty_spawn` async+owned; 3 sync. Errors via `.map_err(\|e\| e.to_string())`. |
| Output over per-spawn `ipc::Channel<raw bytes>`; `pty_exit{code}` via v2 Emitter [unit/manual] | MET | `Channel<Vec<u8>>`, dedicated thread, `app.emit("pty_exit", PtyExit{id,code})`. |
| Registry-shaped `State<Mutex<HashMap<PtyId,PtyState>>>` NOT Core SessionRegistry [unit] | MET | `PtyRegistry(pub Mutex<HashMap<PtyId,PtyState>>)`, imports nothing from core. |
| Poisoned lock → Err at boundary, no panic [design] | MET | `lock_registry` maps poison to `"pty registry mutex poisoned"` String. |
| ADR-3 dedicated std::thread (NOT spawn_blocking) [design] | MET | `std::thread::Builder::new().name("pty-read-{id}")`; rationale doc-commented verbatim for verify. |
| hexagonal-core delta: core/deny untouched, quarantine holds [ci] | MET | core/Cargo.toml + deny.toml + crates/core/src untouched vs main; `cargo tree -p spectty-core` clean; deny "bans ok". |

UI requirements (terminal-ui, manual acceptance) = PR3/PR4 — `ui/` byte-identical to main
(empty diff). **Correct PR2 scoping, NOT a gap.**

## Findings

### WARNING
- **W2 (PR2) — Explicit kill-mid-stream EOF path is not directly tested.** The W1 real-PTY test
  exercises NATURAL EOF (child self-exits via `printf`). The harder concurrency path — a read
  thread parked in `reader.read()` on a still-producing child, then unblocked by an explicit
  `child.kill()` closing the master — is exercised only transitively (the test calls `kill()` AFTER
  natural exit, so the read loop already saw EOF). The kill→EOF chain is sound by construction and
  the design is the proven WezTerm pattern, but no automated test proves a blocking read is
  interrupted by `child.kill()` on a live, output-producing child. Non-blocking (manual acceptance
  PR4 will exercise it when killing a running vim/htop), but a future regression in EOF-on-kill
  behavior would escape CI. Consider a Unix test: spawn `/bin/sh -c "while true; do echo x; sleep 0.05; done"`,
  read a chunk, then `kill()` and assert the read loop terminates within a timeout.

### SUGGESTION
- **S4 (PR2) — Sub-threshold tail latency during child idle.** If the child emits a < MAX_CHUNK
  trickle and then goes idle, the read thread parks in `reader.read()` and `drain_due` is never
  called, so the buffered tail waits for the NEXT byte or EOF. No byte loss (bytes are delayed,
  not dropped), and at 8ms FLUSH_INTERVAL with interactive output this is invisible. A timed
  `recv`/`read` with a wakeup, or a separate flush timer, would bound worst-case tail latency.
  Not needed for M1; note for M2 if interactive latency is ever perceived.
- **S5 (PR2) — VibeLens `show_diff_explanation` not invoked during apply** (tool absent in that
  context, per apply report obs#788). CLAUDE.md per-edit explanation contract unmet for PR2; run
  it on `git diff HEAD` before opening the PR, or record the exception. (Carry-over of PR1 S3.)

### Verified non-issues (adversarial checks that passed)
- ZERO `unwrap`/`expect`/`panic!`/`todo!` in production bridge code. The only production `.expect`
  is `lib.rs:31` (Tauri `run()` bootstrap — M0 ping convention, app can't proceed without a window).
  All `.expect`/`.unwrap` in pty.rs (lines 257-432) are inside the `#[cfg(test)]` mod (starts line 232).
- Poisoned registry lock → `Err` String, never panic — a crashed PTY thread can't brick the UI.
- No needless clone in the hot read loop: reused `[0u8; READ_BUF]`, `id.clone()` only at spawn-time
  (cold path), Coalescer reuses its buffer via `split_off`/`mem::take`/`mem::replace`.
- Thread is named (`pty-read-{id}`) per best-practices.
- `#[must_use]` on Coalescer flush methods (carried from PR1).
- Tests are genuine: concrete byte/value/`is_err()`+message assertions, none would pass with a
  wrong impl (e.g. `resize` test would fail on a cols/rows swap; `kill` test asserts kills==1 and
  entry removed; unknown-id asserts the specific error substring).

## Next recommended
**`sdd-archive`** is NOT yet appropriate (PR3/PR4 remain). PR2 is **clean and pushable** →
proceed to open PR2, then `sdd-apply` PR3 (UI: ipc.ts, useTerminal, Terminal.tsx, App wiring,
@xterm deps). Address S5 (VibeLens) before opening PR2. Optionally close W2 with a kill-mid-stream
test in PR2 or PR4.

---

# M1 — Live PTY Terminal — Verify Report (PR3 — terminal UI / xterm.js)

> SDD verify phase, **third pass**. Scope: **PR3 only** = the terminal UI
> (WU-5 ipc wrappers + WU-6 hook/component/App): `ui/src/pty/ipc.ts`,
> `ui/src/hooks/useTerminal.ts`, `ui/src/components/Terminal.tsx`, `App.tsx`,
> `main.tsx`, `styles.css`, `ui/tests/unit/{ipc,useTerminal}.test.ts`,
> `ui/package.json`. Branch `feat/m1-terminal-ui` (PR1+PR2 merged to main).
> Rust intentionally untouched. Verified adversarially from source against spec
> (obs#785), design (obs#786), tasks (obs#787), the R1 discovery (obs#792), and
> the PR2 backend contract on main (`src-tauri/src/commands/pty.rs`,
> `lib.rs`). Apply report (obs#788) was re-checked, NOT trusted. Base: main `329abfb`.

## Overall verdict: PASS (PR3 is pushable)

**0 CRITICAL, 0 WARNING, 2 SUGGESTION.** No blocking issues. Output WILL render
(R1 decode path verified correct end-to-end). The remaining gate is PR4 manual
acceptance (vim/htop/git-graph render, resize SIGWINCH, scrollback) — out of PR3 pass/fail.

## Gate report (each re-run from source)

| Gate | Command | Result |
|------|---------|--------|
| test | `pnpm -C ui test` | PASS — **12 tests** (7 ipc + 5 useTerminal), 0 failed, 1.06s. usePingPong test intentionally removed. |
| build | `pnpm -C ui build` (`tsc --noEmit && vite build`) | PASS — TS strict clean, vite built; `dist/assets/index-*.css 5.46kB` (xterm.css bundled). Only the informational `>500kB JS chunk` warning (xterm is large), not an error. |
| Rust | (untouched) | Correctly SKIPPED — PR3 changes no Rust; `cargo build` not needed. |

TS strictness confirmed in `ui/tsconfig`: `strict: true`, `noUnusedLocals: true`,
`noUnusedParameters: true` — all ON, build is clean under all three.

## R1 correctness (CRITICAL focus) — CONCLUSION: CORRECT, output WILL render

The crux of whether bytes reach the screen. Verified independently:
- **Backend sends a bare `Vec<u8>`**: `src-tauri/src/commands/pty.rs:187,194,210`
  call `on_output.send(chunk)` where `chunk: Vec<u8>` (and the channel is
  `Channel<Vec<u8>>`, pty.rs:107,170). NOT `tauri::ipc::Response::new(..)`.
- **Therefore JS receives a `number[]`**: per obs#792 (verified against
  tauri-2.11.2 IPC source — blanket `impl<T:Serialize> IpcResponse` → serde_json
  → `InvokeResponseBody::Json`), a bare `Vec<u8>` over `Channel::send` arrives at
  JS `onmessage` as a JSON `number[]`, NOT a `Uint8Array`/`ArrayBuffer`.
- **`decodeChannelBytes` (ipc.ts:30-49) handles all shapes correctly**:
  `Uint8Array` passthrough → `ArrayBuffer` → `new Uint8Array(buf)` → `Array.isArray`
  → `Uint8Array.from` (THE actual M1 path) → defensive `ArrayBuffer.isView` branch
  → else `throw new TypeError` (does NOT silently drop unexpected shapes).
- **The number[] branch is genuinely tested**: `useTerminal.test.ts:135-148` fires
  the real wire shape `[104,105]` on the fake channel and asserts `term.writes[0]`
  is `instanceof Uint8Array` equal to `[104,105]` — an end-to-end proof, not a
  tautology. `ipc.test.ts:24-50` separately asserts the number[]/ArrayBuffer/Uint8Array
  branches of `decodeChannelBytes`.
- **xterm 6 `write()` accepts `Uint8Array`** (verified in installed typings per obs#792).

**Conclusion: the decode path is CORRECT. Bytes from the PTY will render.** A future
switch to the raw-`Response` binary fast-path needs zero FE change (ArrayBuffer branch ready).

## Arg-name contract (CRITICAL focus) — CONCLUSION: CORRECT, no silent runtime failure

Every `invoke` arg name in `ipc.ts` matches the Rust command signature in
`pty.rs` after Tauri's snake_case→camelCase mapping:

| JS call (`ipc.ts`) | Rust command (`pty.rs`) | Match |
|---|---|---|
| `invoke("pty_spawn", {cols, rows, cwd, onOutput})` | `pty_spawn(cols:u16, rows:u16, cwd:Option<String>, on_output: Channel<Vec<u8>>)` | ✓ `onOutput`↔`on_output` |
| `invoke("send_input", {id, data: Array.from(data)})` | `send_input(id:PtyId, data:Vec<u8>)` | ✓ number[]→Vec<u8> |
| `invoke("pty_resize", {id, cols, rows})` | `pty_resize(id, cols:u16, rows:u16)` | ✓ |
| `invoke("pty_kill", {id})` | `pty_kill(id:PtyId)` | ✓ |

Command-name string constants (`pty_spawn`/`send_input`/`pty_resize`/`pty_kill`)
match the `generate_handler!` registrations in `lib.rs:24-28` exactly.

**`send_input` shape is CORRECT and the `Array.from` is NECESSARY:** `sendInput`
sends `data: Array.from(data)` (a plain `number[]`). Verified empirically that
`Array.from(Uint8Array)` yields a true `number[]` (`[97,98]`). A raw `Uint8Array`
would serialize to JSON as an OBJECT (`{"0":97,"1":98}`) which would NOT deserialize
into a Rust `Vec<u8>` — so `Array.from` is required, not cosmetic. This is done
correctly and is unit-asserted (`ipc.test.ts:69-79` expects `data: [97,98]`).

## Lifecycle correctness — CONCLUSION: SOUND, no leak on either path

`useTerminal.ts` single `useEffect` (mirrors usePingPong):
- **Cleanup tears down everything** (return fn, lines 105-114): `disposed=true`,
  `observer.disconnect()`, `dataDisposable.dispose()`, `unlistenPromise.then(u=>u())`
  (pty_exit), `term.dispose()`, and `killPty(ptyId)` if id known.
- **Async-spawn-vs-unmount race GUARDED** (lines 96-103): `spawnPty(...).then(id => { if (disposed) { killPty(id); return; } ptyId = id; })`. If unmount wins the race,
  the late-arriving id is immediately killed — no leaked backend PTY. (Note: this
  late-id-kill branch is only transitively covered; the unmount test resolves the
  spawn BEFORE unmount. Non-blocking — see S6.)
- **onData drops input until id known** (lines 75-80): `if (ptyId === null) return;`
  — no crash/no orphan send before spawn resolves.
- **scrollback: 5000** set (line 17/58) per spec.
- **StrictMode double-mount (dev)**: each mount runs the effect fresh (own term,
  observer, channel, listener in closure scope); the first cleanup fully disposes
  them before the second mount. No leak across the dev double-invoke.

## React 19 / TS discipline — CONCLUSION: CLEAN

- Named imports only (`useEffect`, `useRef`, `type RefObject`, `Terminal`,
  `FitAddon`, `ClipboardAddon`, `listen`) — no `import React`, no `* as React`.
- NO `useMemo`/`useCallback`/`forwardRef` anywhere (React Compiler discipline).
- `RefObject<HTMLDivElement | null>` typing correct (hook signature line 43;
  `Terminal.tsx` `useRef<HTMLDivElement>(null)`).
- No `any`, no `@ts-ignore`/`@ts-expect-error`, no `eslint-disable` in PR3 source
  (grep clean across ipc.ts/useTerminal.ts/Terminal.tsx/App.tsx).
- `decodeChannelBytes` throws `TypeError` on truly-unexpected shapes (does not
  silently drop) — fail-loud per skill discipline.

## Removal soundness — CONCLUSION: CLEAN

- `ui/src/hooks/usePingPong.ts` + `ui/tests/unit/usePingPong.test.ts` DELETED.
  The only remaining `usePingPong` reference is a doc-comment in `useTerminal.ts:31`
  ("Mirrors usePingPong's shape") — no import, no usage. Build passes (no dangling import).
- Rust `ping` command STILL registered: `commands::ping::ping` in
  `lib.rs:24` `generate_handler!`, `commands/mod.rs:3 pub mod ping`. The deletion
  was UI-only — `ping` stays as harmless backend liveness proof. `cargo build`
  correctly not required (no Rust change).
- `App.tsx`/`main.tsx` have no dangling imports — `App` imports `Terminal`,
  `main.tsx` imports `App` + `./styles.css`; tsc strict build green.

## Scope — CONCLUSION: CORRECT

PR3 did NOT perform PR4 manual acceptance (vim/htop/git-graph render, window-resize
SIGWINCH reflow visual check, scrollback-retained visual check, copy/paste OSC52).
Correct scoping — those require a running app (`pnpm tauri dev` on macOS) and are
the PR4 / manual sdd-verify gate. xterm dep versions resolved exactly
6.0.0 / 0.11.0 / 0.2.0 (pnpm-lock).

## Findings

### SUGGESTION
- **S6 (PR3) — Async-spawn-vs-unmount race kill branch is only transitively tested.**
  The `disposed`-guarded late-id `killPty` (useTerminal.ts:97-100) is correct by
  construction, but the unmount test resolves `pty_spawn` BEFORE unmounting, so it
  exercises the happy unmount-after-resolve path, not the unmount-before-resolve
  path where the late id must be killed. Non-blocking (logic is sound and the leak
  it prevents is backend-side). Worth a hardening test: delay the `invoke` resolution,
  unmount first, then resolve, and assert `pty_kill` is called with the late id.
- **S7 (PR3) — VibeLens `show_diff_explanation` not invoked during apply** (tool
  absent in that context, per apply report obs#788). CLAUDE.md per-edit explanation
  contract unmet for PR3; run it on `git diff HEAD` before opening the PR, or record
  the exception. (Carry-over of PR1 S3 / PR2 S5.)

### Verified non-issues (adversarial checks that passed)
- R1 decode path is CORRECT against the REAL PR2 backend (bare Vec<u8> → number[]
  → Uint8Array → term.write) — output renders.
- All 4 invoke arg names + the channel arg map to the Rust signatures with no mismatch.
- `send_input` sends number[] (not a raw Uint8Array that would fail Vec<u8> deserialize).
- Effect cleanup fully tears down (observer, onData, unlisten, dispose, killPty) — no leak.
- Tests are genuine: concrete value/instanceof/objectContaining assertions; the
  channel-bytes test proves a Uint8Array of the exact bytes reaches term.write; the
  resize test asserts `{id, cols, rows}`; the unmount test asserts `pty_kill {id:"pty-1"}`.
  None would pass against a wrong impl.
- No `any`/ts-ignore; React19 named imports, no manual memoization.

## Next recommended
PR3 is **clean and pushable**. Open PR3 (after running VibeLens `show_diff_explanation`
per S7, or recording the exception), then proceed to **WU-7 / PR4 manual acceptance**
on macOS (`pnpm tauri dev`): vim/htop/`git log --oneline --graph` render+behave,
window resize tracks PTY size (SIGWINCH), scrollback retained beyond one screen,
copy/paste OSC52, `pty_exit` observed. That manual gate — NOT this pass — is the final
M1 exit criterion before `sdd-archive`.
