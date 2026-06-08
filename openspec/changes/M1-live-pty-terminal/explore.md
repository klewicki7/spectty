# Exploration: M1 — Live PTY Terminal

> SDD explore phase for change **M1-live-pty-terminal** (Spectty / ai-terminal).
> Maps roadmap + `docs/architecture/pty-layer.md` intent onto the merged M0 hexagonal
> scaffold. Engram mirror: `sdd/M1-live-pty-terminal/explore` (obs 783).

## Context
M0 (scaffold + engram wiring) is merged. M1 must ship a real PTY-backed terminal in the
UI **without** pre-building M2's `AgentRunner` and **without** breaking the engram-style
core quarantine (core depends only on `serde` + `thiserror`). New code lands in
`crates/adapters` (PtyAdapter) + `src-tauri` (commands + state) + `ui` (Terminal pane).
**Core gets NOTHING in M1.**

## 1. Crate boundary (recommended: keep Core untouched in M1)
- `pty-layer.md` frames `PtyPort` as "internal, used by AgentRunner/supervisor" — but
  `AgentRunner` is M2. M1 only needs raw-byte streaming + write + resize + kill (the RAW
  path). The `OutputSignal` decode path exists to feed `AgentRunner::detect_status` — also M2.
- Therefore in M1 the PTY is a pure **adapter + Tauri-state** concern. Do NOT add a
  `PtyPort` trait to `crates/core` in M1 — it would be a trait with no Core consumer
  (YAGNI; the Core's value is being testable without a PTY). Adding it now risks designing
  the trait around M1's byte-pump needs and rewriting it for M2's `AgentRunner` contract.
- `PtyAdapter` lives in `crates/adapters` (new module `pty/`). It does NOT implement any
  Core trait yet. `src-tauri` owns a `PtyAdapter` instance via `tauri::State`.
- Preserves the `deny.toml` gate: adapters may gain `portable-pty`; core stays
  `serde` + `thiserror`. `portable-pty` lands only in adapters/src-tauri, never in the
  core-scoped `cargo deny --manifest-path crates/core/Cargo.toml` closure.
- **Alternative considered**: a thin internal `PtyPort` trait in `crates/adapters` (not
  core) to ease the M2 fake. Lower value than shipping a concrete adapter now; extract the
  trait at M2 when `AgentRunner` needs the seam.

## 2. portable-pty integration
- Crate: `portable-pty` 0.9.0 (WezTerm's; `openpty` on POSIX, `ConPTY` on Windows).
- API: `native_pty_system().openpty(PtySize{cols,rows,pixel_width,pixel_height}) -> PtyPair`;
  `CommandBuilder::new(prog).args/.cwd/.env`; `slave.spawn_command(cmd) -> Box<dyn Child>`;
  `master.try_clone_reader() -> Box<dyn Read+Send>`; `master.take_writer() -> Box<dyn Write+Send>`;
  `master.resize(PtySize)` sends SIGWINCH; `child.kill()` / `child.wait()`.
- **Blocking→async bridge**: portable-pty read is BLOCKING. M0 already has tokio in
  src-tauri. Recommended: a **dedicated `std::thread`** running a blocking read loop into a
  `[u8; 4096..8192]` buffer, NOT `tokio::spawn_blocking` (which is for short tasks, not a
  never-returning loop). This is the WezTerm-style pattern — a **deviation-with-rationale**
  from pty-layer.md's `spawn_blocking`; propose must ratify.
- Carry bytes to UI: the read thread holds a cloned `AppHandle` / a `Channel`.
- Default shell M1: spawn `$SHELL` (fallback `/bin/bash` unix, `cmd.exe`/powershell win).
  Keep a minimal internal spawn input (program+cwd+cols+rows) — do NOT name it `LaunchSpec`
  or model `AgentRunner` (that anti-pattern guard is M2).

## 3. Tauri ↔ xterm.js transport (high-frequency path)
- Commands (all `#[tauri::command]`, in `generate_handler!`): `pty_spawn` (returns a pty id),
  `send_input` (`data: Vec<u8>/String -> master.write_all`), `pty_resize` (cols,rows),
  `pty_kill`. Owned types only (no `&str` in async commands).
- **Output: Event vs Channel** — `pty-layer.md` flags IPC flooding as OPEN.
  - (a) Tauri **event** `pty_output` (reuses M0 Emitter pattern). Simple, but global event
    bus, every emit JSON-serialized → webview IPC pressure under heavy output.
  - (b) Tauri **`ipc::Channel<T>`** (per-invoke typed stream). Lower overhead, scales to
    per-session in M2 (one channel per `pty_spawn`). **RECOMMENDED** for output. Keep
    events for low-freq lifecycle (`pty_exit{code}`).
- **Bytes encoding**: `number[]` is wasteful; base64 is compact but needs FE decode.
  **RECOMMENDED: raw `Vec<u8>` via Channel → `Uint8Array` into `term.write()`** (@xterm/xterm 6
  accepts `Uint8Array`). base64 is the fallback (NEVER `number[]`).
- **IPC batching** (OPEN): do NOT emit per-read-syscall. Coalesce in the Rust read loop:
  flush on size threshold (~8–16 KB) OR time tick (~8–16 ms / 60–120 Hz), whichever first.
  Tunables are M1 constants, validated against vim/htop. **This batching logic is PURE and
  UNIT-TESTABLE.**

## 4. xterm.js in React (React 19 + compiler, no manual memo)
- Packages: `@xterm/xterm` 6.0.0, `@xterm/addon-fit` 0.11.0, `@xterm/addon-clipboard` 0.2.0
  (scoped `@xterm/*` names, NOT legacy `xterm`). Import `@xterm/xterm/css/xterm.css`.
- Mount in a Terminal/Pane component: `useRef` for container + Terminal instance; `useEffect`
  to `new Terminal({scrollback, convertEol:false})`, `loadAddon(FitAddon)`, `term.open()`,
  `fitAddon.fit()`, then `pty_spawn`. Cleanup: `term.dispose()` + `pty_kill` on unmount.
  No `useMemo`/`useCallback` (React compiler). Named React imports.
- Resize: `ResizeObserver` → `fitAddon.fit()` → `invoke('pty_resize', {cols, rows})`,
  debounced; send initial size at spawn.
- Input: `term.onData(data => invoke('send_input', { data }))`.
- Output: `channel.onmessage -> term.write(bytes)`.
- Scrollback: `Terminal({ scrollback: 5000 })` (configurable constant).
- Clipboard: `ClipboardAddon` (OSC 52) for programmatic; xterm default selection copy works.

## 5. State ownership in src-tauri
- M1 single terminal acceptable. Own the handle behind
  `tauri::State<Mutex<HashMap<PtyId, PtyState>>>` keyed by id from `pty_spawn` — registry
  shape now (even with one entry) so it becomes M2's `SessionRegistry` without a rewrite.
- `PtyState` holds: writer (`Box<dyn Write+Send>`), child (for kill), read-thread stop handle.
- Mutex poisoning: follow M0 `in_memory` (`.lock().expect(...)`) or map to a serializable
  command error. Commands return `Result<_, String>` like `ping`.

## 6. Testability under Strict TDD (`cargo test --workspace`; `pnpm -C ui test` → vitest)
- **Rust unit-testable WITHOUT a real PTY**:
  - Byte **batching/coalescing**: extract a pure `Coalescer`/`RingBatcher` (push + drain on
    threshold/tick) — test thresholds, time-tick flush, max-chunk split.
  - Spawn-input construction (program/cwd/env/size assembly) → assert on a small intermediate
    struct, not portable-pty internals.
  - A FAKE pty seam: minimal internal trait (in adapters) `PtyTransport { write, resize, kill }`
    so command handlers test against a fake without opening a real pty. (Adapters-level seam,
    NOT a Core port.) Recommended for TDD of the command layer.
  - ANSI-strip / `OutputSignal` decode is M2 (status detection) — DEFER; M1 renders raw.
- **TS/vitest unit-testable**: a `useTerminal`/`usePty` hook mirroring `usePingPong`'s mock
  pattern (mock `@tauri-apps/api/core` invoke + Channel, `@tauri-apps/api/event` listen).
  Assert spawn-on-mount, send_input-on-onData, resize-on-fit, write-on-channel-message.
  `vi.mock('@xterm/xterm')`. jsdom already configured.
- **Integration/manual** (maps to exit criteria): real vim/htop/`git log --oneline --graph`,
  real SIGWINCH reflow, scrollback retention, ConPTY — manual acceptance checklist in apply/verify.

## 7. Risks / open questions
- **Cross-platform**: M1 macOS-first; ConPTY untested in CI → Windows best-effort, not gated.
- **OPEN (pty-layer.md)**: `text_window`/quiesce DEFER (feed OutputSignal/status = M2). IPC
  batching → DECIDE in M1 (size-OR-time hybrid flush; validate vs htop).
- **clippy -D**: `must_use` on spawn handle; reuse the read buffer (no needless clones in hot
  loop); justified `.expect` only for poisoned mutex per M0 convention, else return `Result`;
  read loop MUST be on its own `std::thread`, never the command's async context.
- **CI-only**: cargo-deny core scope must stay green — confirm NO new dep added to
  `crates/core`. `portable-pty` in `crates/adapters` is fine.
- **Capabilities**: `core:default` covers invoke of custom commands + Channel; `core:event:default`
  already present if events path chosen. Verify no extra capability needed.
- **Test runner**: `ui/package.json` `test` = `vitest run`; confirm the actual command
  (`pnpm -C ui test` vs `npm`) in apply/verify.
- **VibeLens**: `show_diff_explanation` after edits is an apply-phase obligation (CLAUDE.md).

## Next
`sdd-propose` locks the M1 boundary: concrete `PtyAdapter` in adapters, registry-shaped
`tauri::State`, Channel + raw-bytes transport, hybrid-flush batching, **no Core changes**.
