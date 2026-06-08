# M1 — Live PTY Terminal — Delta Spec

> SDD spec phase. Consumes `sdd/M1-live-pty-terminal/proposal` (obs #784) and the
> AUTHORITATIVE roadmap M1 exit criteria (`docs/product/roadmap.md` → "M1 — Live PTY
> Terminal"). Drives `sdd-tasks` (with `sdd-design`). Artifact store: HYBRID
> (engram `sdd/M1-live-pty-terminal/spec` + this file).
>
> This is a DELTA spec: it states WHAT MUST be true after M1 is applied, on top of the
> archived M0 baseline (`hexagonal-core`, `tauri-bridge`, `persistence-port`,
> `monorepo-scaffold`, `ci-pipeline`, `onboarding-tooling`). It describes outcomes, NOT
> implementation. RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.
>
> Each requirement is tagged with its verification class:
> - **[unit]** — assertable under Strict TDD (`cargo test --workspace` / `pnpm -C ui test`)
>   without a real PTY or a running app.
> - **[manual]** — real-app manual acceptance check; maps to a roadmap exit criterion;
>   the `sdd-verify` pass/fail gate.
> - **[ci]** — enforced by the existing CI gates (cargo build, clippy, cargo-deny).

The M0 baseline proved the stack wires together (`ping → pong`) but has no terminal. M1
delivers a real terminal in the UI, backed by a real PTY, rendering live output correctly.
It MUST do so WITHOUT changing the Core and WITHOUT building any M2+ agent machinery.

---

## Capability: pty-adapter

A PTY adapter in `crates/adapters` owns a real pseudo-terminal: it spawns a shell, streams
output bytes, accepts input, resizes, and kills the process. It is a concrete adapter — it
implements no Core trait in M1.

### Requirement: Adapter spawns a shell in a real PTY  [unit]
The PTY adapter MUST be able to open a real pseudo-terminal (via `portable-pty`) and spawn
a shell process attached to it, given a spawn input carrying program, working directory,
and initial size (columns and rows). The spawn input MUST be a minimal internal type
(program + cwd + cols + rows); it MUST NOT be named or shaped as an agent type (no
`LaunchSpec`, no `AgentSpec`) — that is the M2 anti-pattern guard.

#### Scenario: Spawn-input construction assembles the correct command (pure, no PTY)
- **Given** a spawn input with a program, a working directory, and an initial size
- **When** the spawn-input-to-command construction runs
- **Then** it MUST produce command inputs (program, cwd, size) matching the spawn input,
  asserted on a small intermediate struct AND NOT on `portable-pty` internals (so the test
  needs no real PTY)

#### Scenario: Default shell falls back per operating system
- **Given** the platform's default-shell resolution
- **When** no explicit program is provided
- **Then** on Unix it MUST resolve `$SHELL`, falling back to `/bin/bash`, AND on Windows it
  MUST resolve to `cmd.exe` or PowerShell

### Requirement: Adapter streams output bytes off a dedicated read thread  [unit]
The adapter MUST read PTY output on a dedicated `std::thread` per PTY (NOT on a Tauri
command's async context and NOT via `tokio::spawn_blocking`), so the blocking read loop
never blocks the async runtime. Output MUST be delivered as raw bytes (`Vec<u8>` /
`Uint8Array` shape), NOT as `number[]` and NOT, by default, base64-encoded.

#### Scenario: Output is delivered as raw bytes
- **Given** the adapter's output transport
- **When** a chunk of PTY output is forwarded toward the UI
- **Then** it MUST be carried as raw bytes (a binary payload), AND MUST NOT be encoded as a
  JSON array of integers (`number[]`); base64 is permitted ONLY as an explicit fallback

### Requirement: Read loop coalesces output before emitting (hybrid size-OR-time flush)  [unit]
The read loop MUST NOT emit one IPC message per read syscall. A pure coalescer/batcher MUST
accumulate bytes and flush on whichever fires first: a size threshold OR a time tick. The
flush interval and maximum chunk size MUST be M1 constants. The coalescing logic MUST be a
pure unit (no PTY, no Tauri) so its flush behavior is RED→GREEN testable.

#### Scenario: Coalescer flushes when the size threshold is reached
- **Given** a coalescer with a configured size threshold
- **When** pushed bytes accumulate to or beyond that threshold
- **Then** it MUST drain a chunk AND that chunk MUST NOT exceed the configured maximum chunk
  size (a larger accumulation MUST be split into max-size chunks)

#### Scenario: Coalescer flushes on the time tick even below the size threshold
- **Given** a coalescer holding fewer bytes than the size threshold
- **When** a flush tick fires
- **Then** it MUST drain the buffered bytes (time-based flush) so output is not stalled
  waiting to fill the buffer

#### Scenario: Coalescer with no buffered bytes flushes nothing
- **Given** a coalescer with an empty buffer
- **When** a flush tick fires
- **Then** it MUST NOT emit an empty chunk

### Requirement: Adapter accepts input, resizes, and kills  [unit]
The adapter MUST expose write (forward input bytes to the PTY master), resize (apply a new
columns/rows size, which MUST raise SIGWINCH on POSIX), and kill (terminate the child and
stop the read thread). These operations MUST be reachable through a minimal internal
adapters-level transport seam (e.g. `PtyTransport { write, resize, kill }`) so the command
layer can be tested against a fake WITHOUT opening a real PTY. This seam is an adapters
concern; it MUST NOT be a Core port.

#### Scenario: Command-layer operations drive the transport via a fake (no PTY)
- **Given** a fake transport implementing the internal `write` / `resize` / `kill` seam
- **When** the command-layer handlers for input, resize, and kill run against the fake
- **Then** each handler MUST invoke the corresponding transport operation with the inputs it
  received, asserted without a real PTY

#### Scenario: Resize applies the new size to the PTY  [manual]
- **Given** a live PTY running an interactive program
- **When** a resize to new columns/rows is applied
- **Then** the PTY MUST receive the new size AND SIGWINCH MUST be raised so the program
  reflows to the new dimensions

---

## Capability: pty-bridge

The Tauri bridge exposes the PTY lifecycle as commands, streams output over a Channel, and
emits a low-frequency lifecycle exit event. It owns the live PTY behind a registry-shaped
Tauri state.

### Requirement: Bridge exposes the PTY lifecycle commands  [unit]
The `src-tauri` bridge MUST expose, as Tauri v2 commands registered in
`generate_handler!`, the full PTY lifecycle: `pty_spawn` (open a shell; returns a PTY id),
`send_input` (forward input bytes/string to the PTY), `pty_resize` (apply new cols/rows),
and `pty_kill` (terminate the PTY by id). Commands MUST take owned types only (no `&str` in
async commands) and MUST return `Result<_, _>` with a serializable error, following the M0
`ping` convention.

#### Scenario: All four PTY commands are registered in the handler
- **Given** the `src-tauri` `generate_handler!` registration
- **When** the registered command set is inspected
- **Then** `pty_spawn`, `send_input`, `pty_resize`, and `pty_kill` MUST each be present
  (an unregistered command silently fails at invoke, so registration is the guard)

#### Scenario: pty_spawn returns a PTY id that the other commands key on
- **Given** a `pty_spawn` invocation
- **When** it succeeds
- **Then** it MUST return a PTY id AND `send_input` / `pty_resize` / `pty_kill` MUST each
  accept that id to target the spawned PTY

### Requirement: Output streams over a per-spawn Channel; lifecycle exit is an event  [unit]
High-frequency PTY output MUST be carried over a `tauri::ipc::Channel` of raw bytes opened
per `pty_spawn`, NOT over the global Tauri event bus. Low-frequency lifecycle MUST be a
Tauri v2 event (e.g. `pty_exit { code }`) emitted via the `Emitter` trait (matching the M0
v2 emit convention), so the UI can observe child exit.

#### Scenario: Output uses a Channel, lifecycle uses an event
- **Given** the bridge transport wiring
- **When** the output path and the exit path are inspected
- **Then** output MUST flow through a per-spawn `ipc::Channel` of raw bytes AND the exit
  notification MUST be a Tauri v2 event using `AppHandle::emit` via `Emitter` (NOT a removed
  v1 emit signature)

#### Scenario: PTY exit emits the lifecycle event  [manual]
- **Given** a running PTY whose shell process exits
- **When** the child terminates
- **Then** a `pty_exit` event carrying the exit code MUST be emitted AND the UI MUST be able
  to observe it

### Requirement: Bridge owns the PTY behind a registry-shaped state  [unit]
The bridge MUST own the live PTY behind a registry-shaped `tauri::State` keyed by PTY id
(conceptually `Mutex<HashMap<PtyId, PtyState>>`), even though M1 holds exactly one entry.
Each entry MUST retain the writer, the child handle (for kill), and the read-thread stop
handle. This state is a Tauri-state seam; it MUST NOT be modeled as the Core
`SessionRegistry` entity (that is deferred to M2). Mutex poisoning MUST follow the M0
`in_memory` convention (justified `.expect`, or mapped to a serializable command error).

#### Scenario: State is keyed by PTY id (registry shape)
- **Given** the Tauri state that owns the PTY
- **When** its shape is inspected
- **Then** it MUST be keyed by PTY id so commands look the PTY up by id, AND it MUST NOT
  import or depend on a Core `SessionRegistry` type

---

## Capability: terminal-ui

An xterm.js terminal is mounted in a React Pane, renders live ANSI output, forwards
keystrokes, tracks resize, retains scrollback, and supports copy/paste.

### Requirement: xterm.js is mounted in a Pane and wired to the PTY via a hook  [unit]
The UI MUST mount an `@xterm/xterm` 6 `Terminal` inside a Pane component, with the
PTY wiring expressed through a `useTerminal` (or equivalent) hook mirroring the M0
`usePingPong` mock pattern. The hook MUST: invoke `pty_spawn` on mount with a Channel,
forward `term.onData` to `send_input`, drive `pty_resize` from fit, write Channel output
into `term.write`, and on unmount dispose the terminal and invoke `pty_kill`. React 19
named imports MUST be used; manual `useMemo`/`useCallback` MUST NOT be added (the compiler
handles memoization).

#### Scenario: Hook spawns on mount and tears down on unmount
- **Given** the `useTerminal` hook with `invoke`, `Channel`, and `@xterm/xterm` mocked
- **When** the component mounts and later unmounts
- **Then** `pty_spawn` MUST be invoked on mount AND on unmount the terminal MUST be disposed
  AND `pty_kill` MUST be invoked

#### Scenario: Keystrokes forward to the PTY
- **Given** the mounted terminal with mocked `invoke`
- **When** `term.onData` yields input data
- **Then** the hook MUST invoke `send_input` carrying that data

#### Scenario: Channel output is written to the terminal
- **Given** the mounted terminal with a mocked Channel and mocked `term.write`
- **When** the Channel delivers an output chunk
- **Then** the hook MUST call `term.write` with the received bytes

### Requirement: Terminal tracks resize via fit  [unit]
The UI MUST track Pane size changes (via a `ResizeObserver` and the fit addon) and, on
each fit, read the resulting columns/rows and invoke `pty_resize` so the PTY size follows
the rendered size. An initial size MUST be established at/before spawn.

#### Scenario: Fit drives a pty_resize invoke
- **Given** the mounted terminal with mocked `invoke` and a mocked fit addon
- **When** a fit is triggered yielding new columns/rows
- **Then** the hook MUST invoke `pty_resize` with those columns and rows

### Requirement: Terminal renders ANSI and colors  [manual]
The terminal MUST render ANSI escape sequences and colors correctly, feeding raw bytes
into `term.write` (no FE-side ANSI stripping — xterm renders raw). M1 MUST NOT include any
`OutputSignal` / ANSI-strip-for-status decode path (that is M2).

#### Scenario: Interactive programs render and behave correctly
- **Given** a shell open in the Pane
- **When** the user runs `vim`, `htop`, and `git log --oneline --graph`
- **Then** each MUST render correctly (full-screen TUI layout, colors, cursor) AND behave
  correctly (keystrokes reach the program, the screen updates live without jank)

### Requirement: Terminal retains configurable scrollback  [manual]
The terminal MUST be configured with a scrollback buffer of a configurable length (a
constant), and output exceeding one screen MUST remain accessible by scrolling back.

#### Scenario: Scrollback is retained beyond one screen
- **Given** a shell open in the Pane
- **When** the shell produces more output than fits on one screen
- **Then** earlier output MUST remain retrievable by scrolling back up to the configured
  scrollback limit

### Requirement: Terminal supports copy and paste  [manual]
The terminal MUST support copying selected text and pasting from the system clipboard,
using xterm selection plus the clipboard addon (OSC 52). Copy/paste MUST NOT require any
extra Tauri capability beyond what in-webview selection already permits.

#### Scenario: Copy and paste use the system clipboard
- **Given** a shell open in the Pane with text on screen
- **When** the user selects text to copy and later pastes
- **Then** the selection MUST reach the system clipboard on copy AND clipboard contents
  MUST be delivered to the PTY on paste

---

## Capability: hexagonal-core (delta — invariant preserved)

This is a guard delta on the archived `hexagonal-core` baseline: M1 adds PTY machinery
 to adapters and the bridge while leaving the Core untouched.

### Requirement: Core is unchanged and gains no PTY/runtime dependency  [ci]
`spectty-core` MUST NOT gain any new dependency in M1. In particular it MUST NOT depend on
`portable-pty`, `tokio`, `tauri`, or any agent/tool crate. `portable-pty` MUST land only in
`crates/adapters` and/or `src-tauri`. The Core MUST remain `serde` + `thiserror` only, and
the core-scoped `cargo-deny` gate MUST stay green. No `PtyPort` trait and no `OutputSignal`
type may be added to the Core in M1.

#### Scenario: Core manifest still lists no PTY or runtime crate
- **Given** the `spectty-core` `Cargo.toml` after M1
- **When** its dependency list is inspected
- **Then** it MUST NOT include `portable-pty`, `tokio`, `tauri`, or any agent/tool crate,
  AND it MUST remain limited to `serde` + `thiserror`

#### Scenario: core-scoped cargo-deny stays green after M1
- **Given** the M1 changes applied (PTY in adapters / src-tauri, Core untouched)
- **When** the core-scoped `cargo-deny` boundary gate runs in CI
- **Then** it MUST exit 0 with no forbidden-dependency findings AND `cargo build` MUST
  succeed

#### Scenario: clippy stays clean on the hot path (guard)  [ci]
- **Given** the M1 read-loop and spawn code under the M0 `clippy -D warnings` gate
- **When** clippy runs in CI
- **Then** it MUST report no warnings (the hot read loop reuses its buffer with no redundant
  `Vec` clone; the spawn handle is `must_use`; any `.expect` is the justified poisoned-mutex
  case per the M0 convention)

---

## Roadmap exit criteria (acceptance gate)

These three checks are the verbatim roadmap M1 exit criteria and the `sdd-verify` pass/fail
gate. They are real-app manual-acceptance checks (no unit substitute). Each maps to a
`[manual]` requirement above.

### Requirement: M1 satisfies all three roadmap exit criteria  [manual]

#### Scenario: (1) Open a shell and run real interactive programs
- **Given** the running app
- **When** the user opens a shell in the Pane and runs `vim`, `htop`, and
  `git log --oneline --graph`
- **Then** each MUST render and behave correctly

#### Scenario: (2) Resize the window; PTY and rendering track the new size
- **Given** a shell open in the Pane
- **When** the user resizes the window
- **Then** the rendered terminal MUST reflow to the new size AND the PTY MUST receive the
  matching new size (SIGWINCH), so the running program redraws correctly

#### Scenario: (3) Scrollback is retained beyond one screen
- **Given** a shell open in the Pane
- **When** the shell produces more output than fits on screen
- **Then** the earlier output MUST be retained and reachable by scrolling back

---

## Cross-platform stance

### Requirement: macOS MUST pass; Windows/ConPTY is best-effort  [manual]
M1 acceptance MUST pass on macOS. The Windows/ConPTY path SHOULD work (via `portable-pty`)
but MUST NOT be a CI-gated requirement for M1; Windows regressions MUST NOT block M1
acceptance. The default-shell fallback MUST be per-OS (Unix `$SHELL` → `/bin/bash`; Windows
`cmd.exe` / PowerShell).

#### Scenario: macOS acceptance is gating, Windows is best-effort
- **Given** the M1 acceptance run
- **When** acceptance is evaluated per platform
- **Then** all three exit criteria MUST pass on macOS AND a Windows/ConPTY failure MUST NOT
  block M1 (best-effort, ungated)

---

## Out of scope (NO requirements in M1 — M2+)

The following carry NO M1 requirements and MUST NOT be built in M1:

- **`AgentRunner`** port/trait and any per-agent runner — M2.
- **`AgentStatus`** state machine (Starting → Idle → Running → AwaitingInput / Completed /
  Error) — M2.
- **`OutputSignal`** decode path / ANSI-strip-for-status state machine (`text_window`,
  `is_active`, `last_byte_at`, quiesce window) — M2. xterm renders raw ANSI; M1 needs no
  stripper.
- **`SessionRegistry` in Core** — M2. (M1 uses a registry-*shaped* `tauri::State`, which is
  a Tauri-state seam, NOT the Core entity.)
- **Multi-session UI** (tabs, panes, switcher, named sessions) — M4. M1 is a single
  terminal.
- **Provisioner / `ProvisioningPort`** — M2.
- **`LaunchSpec` as a named, agent-modelled type** — deferred; M1 uses a minimal internal
  spawn input.
- **`PtyPort` trait in Core** — M2.
- **`text_window` size / quiesce threshold tuning** — feeds OutputSignal/status = M2.
