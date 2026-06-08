# M1 — Live PTY Terminal — Proposal

> SDD propose phase. Consumes `sdd/M1-live-pty-terminal/explore` (obs #783) and the
> AUTHORITATIVE roadmap M1 scope + exit criteria. Drives `sdd-spec` and `sdd-design`.
> Artifact store: HYBRID (engram `sdd/M1-live-pty-terminal/proposal` + this file).

## Intent

**What problem.** M0 proved the stack wires together (Tauri + Rust hexagonal core +
React, engram quarantine enforced, `ping → pong` round-trip). It has no terminal. M1
must deliver a *real* terminal in the UI, backed by a *real* PTY, rendering live output
correctly — the baseline every later milestone (agent spawn, status, the triad) is built
on top of.

**Why now.** M1 is the next vertical slice in the roadmap and the hard prerequisite for
M2 (`AgentRunner`, status detection, Provisioner). Shipping it proves the Bridge can
carry high-frequency PTY I/O without jank — the single riskiest performance assumption in
the whole architecture (pty-layer.md flags IPC flooding as OPEN). We resolve it here, in
isolation, before agent logic complicates the picture.

**What success looks like (acceptance contract — verbatim from roadmap M1 exit criteria):**
1. Open a shell in the Pane; run `vim`, `htop`, `git log --oneline --graph` — each renders
   and behaves correctly.
2. Resize the window; the PTY and rendering track the new size.
3. Scrollback is retained after the shell produces more output than fits on screen.

These three checks are the definition of DONE. They are real-app, manual-acceptance checks
(see Decision 6 / OUT-OF-SCOPE on why they are not unit tests). `sdd-verify` MUST treat
them as the pass/fail gate.

## Scope

### In scope
- **`PtyAdapter`** wrapping `portable-pty`: spawn a shell process, blocking read loop,
  write, resize, kill. New module `crates/adapters/src/pty/`.
- **Tauri command layer** in `src-tauri/`: `pty_spawn`, `send_input` (write), `pty_resize`,
  `pty_kill`, registered in `generate_handler!`. Owned types only.
- **Output transport**: `tauri::ipc::Channel<Vec<u8>>` carrying raw PTY bytes to the UI;
  low-frequency lifecycle as Tauri events (e.g. `pty_exit { code }`).
- **xterm.js Terminal pane** in React (`@xterm/xterm` 6 + `@xterm/addon-fit` +
  `@xterm/addon-clipboard`), feeding raw bytes into `term.write()`.
- **Resize**: `ResizeObserver` → `fitAddon.fit()` → `pty_resize(cols, rows)` → SIGWINCH.
- **Keyboard input**: `term.onData` → `send_input`.
- **Scrollback** buffer (configurable constant).
- **Copy/paste** via system clipboard (xterm selection + ClipboardAddon for OSC 52).
- **Hybrid size-OR-time flush batching** in the read loop (coalesce bytes before IPC).
- **Strict-TDD test seams**: a pure Coalescer/batcher (Rust unit), a command-layer fake
  transport, and a `useTerminal` hook (vitest) mocking `invoke` / `Channel` / xterm.

### Out of scope (M2+ — explicitly NOT built in M1)
- **AgentRunner** port/trait and any per-agent runner — M2.
- **AgentStatus state machine** (Starting → Idle → Running → AwaitingInput / Completed /
  Error) — M2.
- **`OutputSignal` decode path / ANSI-strip state machine** (the `text_window`,
  `is_active`, `last_byte_at`, quiesce window) — M2. xterm renders raw ANSI; M1 needs no
  stripper.
- **`SessionRegistry` in Core** — M2. (M1 uses a registry-*shaped* `tauri::State`, see
  Decision 4 — that is a Tauri-state seam, NOT the Core entity.)
- **Multi-session UI** (tabs, panes, switcher, named sessions) — M4. M1 is a single
  terminal.
- **Provisioner / `ProvisioningPort`** — M2.
- **`LaunchSpec` as a named, agent-modelled type** — deferred; M1 uses a minimal internal
  spawn input (program + cwd + cols + rows). Naming it `LaunchSpec` or shaping it around an
  agent is the M2 anti-pattern guard; do NOT do it here.
- **`PtyPort` trait in Core** — M2 (see Decision 1).
- **`text_window` size / quiesce threshold tuning** (pty-layer.md OPEN) — feeds
  OutputSignal/status = M2.

## Cross-platform stance

M1 ships **macOS-first**. `portable-pty` gives ConPTY on Windows for free, but the Windows
path is **best-effort, NOT CI-gated** (the M0 `deny.toml` graph is host-only; ConPTY is
untested in CI). Default shell is per-OS: `$SHELL` with fallback `/bin/bash` on unix,
`cmd.exe` / PowerShell on Windows. Windows regressions do not block M1 acceptance.

## Approach

A single Rust backend process owns the PTY. `PtyAdapter` (in `crates/adapters`) opens the
pty via `portable_pty::native_pty_system()`, spawns the shell, and runs a **dedicated
`std::thread`** blocking read loop. That loop coalesces bytes through a pure batcher and
flushes raw `Vec<u8>` chunks into a per-spawn `ipc::Channel`. `src-tauri` owns the live
PTY behind a registry-shaped `tauri::State`, keyed by an id returned from `pty_spawn`. The
React Terminal pane opens xterm.js, calls `pty_spawn` with a Channel, pipes
`channel.onmessage → term.write(bytes)`, forwards `onData → send_input`, and drives
`fit → pty_resize`. The Core gets nothing.

```
React Terminal pane (xterm.js)                 src-tauri (commands + State)        crates/adapters
  onData ───────── send_input ──────────────▶  write → master.write_all
  fit ──────────── pty_resize ──────────────▶  resize → master.resize (SIGWINCH)
  pty_spawn(Channel<Vec<u8>>) ──────────────▶  spawn → PtyAdapter::spawn ─────────▶ openpty + shell
  term.write(bytes) ◀── Channel raw bytes ◀──  read thread flushes batched chunks ◀ Coalescer(read loop)
  (listen pty_exit) ◀── Tauri event ────────  child exit
```

## Architectural decisions (each RATIFIED or OVERRIDDEN with one-line rationale)

**D1 — No Core changes in M1. PtyAdapter is a concrete adapter; defer PtyPort/OutputSignal
to M2. RATIFIED.**
Rationale: M1 needs only the raw byte-pump (read/write/resize/kill); the `PtyPort` trait
and `OutputSignal` exist to feed M2's `AgentRunner::detect_status`, so adding them now is a
trait with no Core consumer (YAGNI) and risks designing the seam around the byte-pump and
rewriting it at M2. `PtyAdapter` lives in `crates/adapters/src/pty/` and implements no Core
trait. The engram-style quarantine stays intact: `core` = serde + thiserror only;
`portable-pty` lands in `adapters`/`src-tauri` and is NOT scanned by the core-scoped
`cargo deny`. **Hard constraint: zero new deps in `crates/core`.**

**D2 — Output transport: `tauri::ipc::Channel<Vec<u8>>` for high-freq output + low-freq
lifecycle events; raw bytes (not `number[]` / base64) into `term.write()`. RATIFIED
(overrides pty-layer.md's `pty_output` Tauri *event*).**
Rationale: a per-invoke typed Channel is purpose-built for high-frequency Rust→FE streaming
with lower overhead than the global event bus, and scales cleanly to one-channel-per-spawn
in M2; `number[]` is ~6–8× JSON overhead per byte and base64 needs a FE decode, whereas
`@xterm/xterm` 6 accepts `Uint8Array` in `write()` so raw bytes avoid both. Keep Tauri
events only for low-frequency lifecycle (`pty_exit`). **Fallback if Channel-binary proves
awkward: base64 string — NEVER `number[]`.**

**D3 — Read loop on a dedicated `std::thread`. RATIFIED (deviation from pty-layer.md's
`spawn_blocking`).**
Rationale: `portable-pty`'s read is blocking and the loop lives for the PTY's entire
lifetime; `tokio::spawn_blocking` is sized for short tasks and pinning a never-returning
loop to a blocking-pool slot is the wrong tool. A dedicated `std::thread` per PTY is the
WezTerm-style pattern, keeps the Tokio runtime unblocked, and maps 1:1 onto pty-layer.md's
"one task/thread per Session, isolated."

**D4 — Registry-shaped `tauri::State<Mutex<HashMap<PtyId, PtyState>>>` keyed by an id from
`pty_spawn`, one entry in M1. RATIFIED.**
Rationale: this is the M2 `SessionRegistry` *seam* at near-zero cost — commands already
take/return a `PtyId`, so the single-entry map in M1 becomes the multi-session registry in
M2/M4 without a command-signature rewrite. `PtyState` holds the writer
(`Box<dyn Write + Send>`), the child handle (for kill), and the read-thread stop handle.
This is a Tauri-state concern, explicitly NOT the Core `SessionRegistry` entity (that is
D1's deferral). Mutex poisoning follows the established M0 `in_memory` convention (justified
`.expect` on a poisoned lock, or map to a serializable `Result<_, String>` command error
as `ping` does).

**D5 — Hybrid size-OR-time flush batching for IPC; tunables as M1 constants. RATIFIED
(resolves pty-layer.md's OPEN batching question).**
Rationale: never emit per read-syscall. The read loop accumulates into a buffer and flushes
on **whichever fires first** — a size threshold (~8–16 KB) or a time tick (~8–16 ms,
≈60–120 Hz). This is the ring-buffer + rate-cap the doc describes, bounds webview IPC
pressure under bursty output (the `htop` / `vim` case), and the flush/coalesce logic is
**pure and unit-testable** (D6). Flush interval and max chunk are M1 constants, validated
empirically against `vim` / `htop`; the empirical *tuning* is acceptance-time, the
*mechanism* is locked now.

**D6 — Strict-TDD seams: pure Coalescer/batcher (Rust unit) + command-layer fake transport
+ `useTerminal` hook (vitest). RATIFIED.**
Rationale: Strict TDD is active (`cargo test --workspace`; `pnpm -C ui test` → vitest), so
every M1 behavior must have a RED→GREEN seam reachable WITHOUT a real PTY:
- **Coalescer/RingBatcher** (pure): `push(&[u8])` + drain-on-size / drain-on-tick /
  max-chunk split. No PTY, no tauri. Reuse one buffer in the hot loop (clippy
  `redundant_clone` watch).
- **Spawn-input construction** (pure): assert program/cwd/size assemble the right
  `CommandBuilder` inputs via a small intermediate struct (not on `portable-pty`
  internals).
- **Command-layer fake transport**: a minimal internal adapters-level trait
  (e.g. `PtyTransport { write, resize, kill }`) so command handlers test against a fake —
  this is an adapters seam, NOT a Core port (consistent with D1).
- **`useTerminal` hook (vitest)**: mirrors `usePingPong`'s mock pattern — mock
  `@tauri-apps/api/core` `invoke` + `Channel`, mock xterm (`vi.mock('@xterm/xterm')`).
  Assert spawn-on-mount, `send_input`-on-`onData`, `pty_resize`-on-fit, `term.write` on
  channel message, dispose + `pty_kill` on unmount. jsdom env already configured.
- **Manual/integration (not unit-testable → maps to exit criteria)**: real `vim` / `htop` /
  `git log --graph` render, real SIGWINCH reflow, real scrollback retention, ConPTY. These
  become the `sdd-apply` / `sdd-verify` manual acceptance checklist (the D-Intent contract).

## Risks / open questions (for sdd-spec & sdd-design)
- **R1 — IPC binary path**: raw `Vec<u8>` over `Channel` must feed `Uint8Array` into
  `term.write()` efficiently. If Channel-binary is awkward in practice, fall back to base64
  (D2). `sdd-design` should pin the exact Channel payload shape and the FE decode.
- **R2 — Batching tunables** (size / time) are empirical; `vim` / `htop` validation is an
  acceptance-time activity. Mechanism locked (D5); constants may move.
- **R3 — Cross-platform**: ConPTY untested in CI; Windows best-effort, not gated. Default
  shell differs per-OS — `sdd-spec` should specify the per-OS fallback explicitly.
- **R4 — clippy `-D warnings`** (M0 CI): watch `must_use` on the spawn handle, no
  `Vec` clones in the hot read loop (reuse buffer), `.expect` only per the justified-M0
  poisoned-mutex convention.
- **R5 — Capabilities**: custom commands are allowed by `core:default` invoke in v2 (no
  per-command capability needed); verify ClipboardAddon needs no extra Tauri capability for
  in-webview selection. `sdd-design` to confirm.
- **R6 — VibeLens MCP hook**: CLAUDE.md requires `show_diff_explanation` after edits — an
  `sdd-apply` obligation, noted for downstream phases.
