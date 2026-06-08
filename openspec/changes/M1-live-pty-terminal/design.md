# M1 — Live PTY Terminal: Technical Design

> Status: design (the HOW at architectural level). Consumed by `sdd-tasks`.
> Reads: proposal (obs 784), explore (obs 783). Built against the **real** merged M0 code
> (verified on disk: `src-tauri/src/commands/ping.rs`, `crates/adapters/src/persistence/in_memory.rs`,
> `ui/src/hooks/usePingPong.ts`, `capabilities/default.json`, `deny.toml`).

## 0. Design Goals & Non-Goals

**Goal**: ship one real PTY-backed terminal pane that renders `vim`/`htop`/`git log --graph`,
tracks window resize (SIGWINCH), and retains scrollback — proving the Tauri bridge carries
high-frequency byte I/O without jank.

**Non-Goals (M2+)**: `AgentRunner`, `AgentStatus`, `OutputSignal`/ANSI-strip, `SessionRegistry`
in Core, `PtyPort` trait in Core, multi-session UI, `LaunchSpec`/named agents, Provisioner.

**Hard invariant**: `crates/core/Cargo.toml` gains NOTHING. `portable-pty` lands in
`crates/adapters` + `src-tauri` only. `cargo deny --manifest-path crates/core/Cargo.toml check bans`
stays green because the scoped closure never sees `portable-pty`.

---

## 1. Architecture Approach

**Pattern**: hexagonal, but M1 deliberately keeps the PTY as a **concrete adapter + Tauri-state
concern** — NOT a Core port. Layering:

```
ui (xterm.js, React 19)
   │  invoke(pty_spawn/send_input/pty_resize/pty_kill)   +   Channel<Vec<u8>>  +  listen("pty_exit")
   ▼
src-tauri  (commands/pty.rs + PtyRegistry state)   ← the ONLY tauri-aware layer
   │  owns PtyState behind Mutex<HashMap<PtyId, PtyState>>; spawns the read thread; wires the Channel
   ▼
crates/adapters/src/pty/   (PtyAdapter over portable-pty, pure Coalescer, PtyTransport seam)
   │  no tauri, no tokio
   ▼
portable-pty 0.9.0   (openpty / ConPTY)
```

`crates/core` is untouched. The dependency arrow from `src-tauri` and `adapters` to `core` is
unchanged; no new arrow points *into* core.

### ADR-style decisions (each maps to a ratified proposal decision)

- **ADR-1 (D1) — No Core port in M1.** PtyAdapter is concrete in `crates/adapters/src/pty/`,
  implements no Core trait. *Rationale*: M1 needs only raw-byte streaming + write + resize + kill;
  the `OutputSignal`/`PtyPort` consumer (`AgentRunner::detect_status`) is M2. A trait with no Core
  consumer is YAGNI and would be redesigned at M2. *Rejected*: adding `PtyPort` to core now (couples
  the trait to the byte-pump, then rewrites for the agent contract) — and adding a `PtyPort` to
  *adapters* (lower value than a concrete adapter; extract at M2 when `AgentRunner` needs the seam).

- **ADR-2 (D2) — Output transport = `tauri::ipc::Channel<Vec<u8>>`, raw bytes.** High-frequency
  output flows through a per-spawn `Channel`; low-frequency lifecycle uses a Tauri **event**
  (`pty_exit`). *Rationale*: Channel is the v2 high-throughput Rust→FE stream; lower overhead than
  the global event bus, and scales to one-channel-per-session at M2. Raw `Vec<u8>` → `Uint8Array`
  fed straight into `term.write()` (xterm 6 accepts `Uint8Array`) avoids base64 cost. *Rejected*:
  `pty_output` global event (JSON-serialized per emit, webview IPC pressure under `htop`);
  `number[]` encoding (≈6–8 bytes/byte). *Documented fallback*: base64 string IF Channel-binary
  proves awkward — never `number[]`.

- **ADR-3 (D3) — Dedicated `std::thread` read loop per PTY (deviation from `pty-layer.md`).**
  `pty-layer.md` says `spawn_blocking`. We use a long-lived `std::thread`. *Rationale*:
  `spawn_blocking` is for short tasks; this loop lives the PTY's whole life and would pin a Tokio
  blocking-pool worker forever, starving it. A dedicated thread is the WezTerm-style pattern and
  keeps the Tokio runtime unblocked. **This is the one explicit deviation; called out for verify.**

- **ADR-4 (D4) — Registry-shaped state now.** `tauri::State<Mutex<HashMap<PtyId, PtyState>>>`
  keyed by an id returned from `pty_spawn`, one entry in M1. *Rationale*: near-zero-cost seam that
  becomes M2's `SessionRegistry` without a command-signature rewrite. This is a Tauri-state concern,
  NOT the Core `SessionRegistry` entity. *Rejected*: `Mutex<Option<PtyState>>` (forces a breaking
  signature change at M2).

- **ADR-5 (D5) — Hybrid size-OR-time flush.** Coalesce in the read loop; flush on size (~8–16KB)
  OR time (~8–16ms ≈ 60–120Hz), whichever first. *Rationale*: never emit per read syscall (`htop`
  floods); the threshold + rate-cap is the ring-buffer the doc asks for. *Rejected*: pure event-rate
  cap and pure byte-threshold (each janky at one extreme). Constants are M1 tunables, validated
  empirically at acceptance.

- **ADR-6 (D6) — Strict-TDD seams.** Pure `Coalescer` (push/drain) + pure spawn-input builder
  unit-tested without a PTY; command layer tested against a `PtyTransport` fake; `useTerminal` hook
  tested in vitest mirroring `usePingPong` (mock invoke/Channel/xterm).

---

## 2. Module / File Layout

### New Rust files — `crates/adapters/src/pty/`
```
crates/adapters/src/pty/
├── mod.rs          # pub use; module docs; re-exports PtyAdapter, Coalescer, PtyTransport, PtySpawnConfig, PtyHandle
├── coalescer.rs    # PURE batcher (no PTY, no tauri) — the primary TDD unit
├── transport.rs    # PtyTransport trait (write/resize/kill) — the fake seam for command tests
├── config.rs       # PtySpawnConfig + default_shell() (per-OS) — PURE builder, TDD unit
└── adapter.rs      # PtyAdapter: portable-pty spawn, reader/writer handles, resize, kill
```
`crates/adapters/src/lib.rs` gains `pub mod pty;` and re-exports
`pub use pty::{PtyAdapter, PtySpawnConfig, Coalescer, PtyTransport, PtyHandle};`.

### New Rust files — `src-tauri/src/`
```
src-tauri/src/
├── commands/
│   ├── mod.rs      # + `pub mod pty;`
│   └── pty.rs      # pty_spawn / send_input / pty_resize / pty_kill  (+ PtyExit event payload)
└── pty_state.rs    # PtyRegistry (Mutex<HashMap<PtyId, PtyState>>), PtyState, PtyId
```
`src-tauri/src/lib.rs`: register the 4 commands in `generate_handler!` and `.manage(PtyRegistry::default())`.

### New UI files — `ui/src/`
```
ui/src/
├── components/
│   └── Terminal.tsx      # xterm mount: container ref, fit, ResizeObserver, dispose+kill
├── hooks/
│   └── useTerminal.ts    # spawn/Channel/send_input/resize/kill orchestration (mirrors usePingPong)
└── pty/
    └── ipc.ts            # thin typed wrappers: spawnPty/sendInput/resizePty/killPty + Channel setup
ui/tests/unit/
├── useTerminal.test.ts   # vitest: mock @tauri-apps/api core(+Channel)/event, vi.mock('@xterm/xterm')
```
`ui/src/App.tsx`: render `<Terminal />` (replaces the M0 ping button as the primary view; ping kept
or removed per tasks — design leaves ping intact, Terminal mounts alongside).
xterm CSS imported once in `Terminal.tsx`: `import "@xterm/xterm/css/xterm.css";`.

### Manifest / config deltas
- `crates/adapters/Cargo.toml`: `+ portable-pty = "0.9.0"`.
- `src-tauri/Cargo.toml`: `+ portable-pty = "0.9.0"` (for the registry/read-thread wiring types).
- `crates/core/Cargo.toml`: **NO CHANGE** (the gate).
- `deny.toml`: **NO CHANGE** — `portable-pty` is not in the core-scoped closure. (Verify task only.)
- `ui/package.json`: `+ "@xterm/xterm": "^6.0.0"`, `+ "@xterm/addon-fit": "^0.11.0"`,
  `+ "@xterm/addon-clipboard": "^0.2.0"`.
- `capabilities/default.json`: **NO CHANGE expected** — see §7.

---

## 3. Rust Signatures (concrete, code-shaped)

### `crates/adapters/src/pty/config.rs` — pure
```rust
/// Inputs needed to open a shell PTY. PURE data — no portable-pty types leak here,
/// so it is trivially unit-testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySpawnConfig {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

impl PtySpawnConfig {
    /// Build an open-shell config using the per-OS default shell.
    pub fn shell(cols: u16, rows: u16, cwd: Option<String>) -> Self { /* ... */ }
}

/// $SHELL → /bin/bash (unix) ; %COMSPEC%/cmd.exe (windows). Pure given an env getter.
pub fn default_shell(get_env: impl Fn(&str) -> Option<String>) -> String { /* ... */ }
```
> `default_shell` takes an env getter so the test injects `$SHELL`/absent without touching process env
> (one-assertion-per-test friendly).

### `crates/adapters/src/pty/coalescer.rs` — pure, the primary TDD unit
```rust
use std::time::{Duration, Instant};

/// PURE hybrid size-OR-time byte batcher. No PTY, no tauri, no threads.
/// The read loop owns one and calls push() after each read, then drain_due() on a tick.
pub struct Coalescer {
    buf: Vec<u8>,
    max_chunk: usize,       // size threshold (~8–16KB)
    flush_interval: Duration, // time threshold (~8–16ms)
    last_flush: Instant,
}

impl Coalescer {
    pub fn new(max_chunk: usize, flush_interval: Duration, now: Instant) -> Self { /* ... */ }

    /// Append bytes. Returns Some(chunk) IMMEDIATELY if the size threshold is reached
    /// (splitting at max_chunk; the remainder stays buffered). None otherwise.
    #[must_use]
    pub fn push(&mut self, bytes: &[u8], now: Instant) -> Option<Vec<u8>> { /* ... */ }

    /// Flush if the time threshold elapsed since last flush and the buffer is non-empty.
    #[must_use]
    pub fn drain_due(&mut self, now: Instant) -> Option<Vec<u8>> { /* ... */ }

    /// Unconditional final flush (called on EOF / kill before the thread exits).
    #[must_use]
    pub fn drain_all(&mut self) -> Option<Vec<u8>> { /* ... */ }
}
```
> `Instant` is injected (`now` params) so time-tick tests are deterministic — no `sleep`.
> Hot-loop discipline: `buf` is reused across pushes; the only allocation is the returned chunk
> (`std::mem::take` / `split_off`), satisfying clippy `redundant_clone`/`-D warnings`.

### `crates/adapters/src/pty/transport.rs` — the command-test seam
```rust
/// Minimal adapters-level seam so the command layer can be tested against a fake
/// WITHOUT opening a real PTY. NOT a Core port (lives in adapters). Object-safe.
pub trait PtyTransport: Send {
    fn write(&mut self, bytes: &[u8]) -> Result<(), PtyError>;
    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtyError>;
    fn kill(&mut self) -> Result<(), PtyError>;
}
```

### `crates/adapters/src/pty/adapter.rs` — concrete portable-pty impl
```rust
use std::io::{Read, Write};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// Error surfaced across the adapter; maps to String at the command boundary (like ping).
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("pty open failed: {0}")] Open(String),
    #[error("pty spawn failed: {0}")] Spawn(String),
    #[error("pty io failed: {0}")] Io(#[from] std::io::Error),
    #[error("pty resize failed: {0}")] Resize(String),
    #[error("unknown pty id: {0}")] UnknownId(String),
    #[error("registry mutex poisoned")] Poisoned,
}

/// Owns the portable-pty master + child. Read side is handed out as a Box<dyn Read+Send>
/// for the dedicated read thread; write side stays here behind the registry's Mutex.
pub struct PtyAdapter {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child:  Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtyAdapter {
    /// Open a PTY and spawn the program. Returns the adapter + the cloned reader the
    /// caller moves into the read thread.
    pub fn spawn(cfg: &PtySpawnConfig) -> Result<(Self, Box<dyn Read + Send>), PtyError> { /* ... */ }
}

impl PtyTransport for PtyAdapter {
    fn write(&mut self, bytes: &[u8]) -> Result<(), PtyError> { self.writer.write_all(bytes)?; Ok(()) }
    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtyError> { /* master.resize(PtySize{..}) */ }
    fn kill(&mut self)  -> Result<(), PtyError> { /* child.kill() */ }
}
```

### `src-tauri/src/pty_state.rs` — registry-shaped state (ADR-4)
```rust
use std::collections::HashMap;
use std::sync::Mutex;
use spectty_adapters::PtyAdapter;

/// Opaque id returned by pty_spawn. M1 always has one; M2 SessionRegistry reuses the key.
pub type PtyId = String; // uuid-ish string; serde-friendly across IPC

/// Per-PTY live handles owned by the Tauri side.
pub struct PtyState {
    pub adapter: PtyAdapter,        // write/resize/kill (PtyTransport)
    pub stop: std::sync::Arc<std::sync::atomic::AtomicBool>, // read-thread shutdown flag
    pub reader_thread: Option<std::thread::JoinHandle<()>>,   // joined on kill/drop
}

/// Registry-shaped state. One entry in M1.
#[derive(Default)]
pub struct PtyRegistry(pub Mutex<HashMap<PtyId, PtyState>>);
```

### `src-tauri/src/commands/pty.rs` — commands (errors as `String`, like ping)
```rust
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use crate::pty_state::{PtyId, PtyRegistry, PtyState};

/// Low-frequency lifecycle event payload (raw bytes go via Channel, NOT here).
#[derive(Clone, serde::Serialize)]
pub struct PtyExit { pub id: PtyId, pub code: Option<i32> }

#[tauri::command]
pub async fn pty_spawn(
    app: AppHandle,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    on_output: Channel<Vec<u8>>,   // raw bytes → Uint8Array on FE (ADR-2)
    registry: State<'_, PtyRegistry>,
) -> Result<PtyId, String> { /* open adapter, spawn read thread w/ Coalescer + Channel, insert, return id */ }

#[tauri::command]
pub fn send_input(id: PtyId, data: Vec<u8>, registry: State<'_, PtyRegistry>) -> Result<(), String> { /* write */ }

#[tauri::command]
pub fn pty_resize(id: PtyId, cols: u16, rows: u16, registry: State<'_, PtyRegistry>) -> Result<(), String> { /* resize */ }

#[tauri::command]
pub fn pty_kill(id: PtyId, registry: State<'_, PtyRegistry>) -> Result<(), String> { /* set stop, kill, join, remove */ }
```
> `pty_spawn` is `async` (owned types only — `Vec<u8>`/`String`/`u16`, never `&str`, per tauri skill).
> `send_input`/`pty_resize`/`pty_kill` are sync (no await; quick lock+act). Error mapping:
> `.map_err(|e| e.to_string())` exactly like `ping`. Mutex lock uses the M0 convention:
> `.lock().map_err(|_| "pty registry mutex poisoned".to_string())?` (command boundary returns the
> error rather than `.expect()`-panicking, since a poisoned registry should not crash the app;
> the in-adapter `.expect("… poisoned")` convention from `in_memory.rs` is reserved for the adapter's
> own internal locks, of which the PTY adapter has none in M1).

### Read-thread wiring (inside `pty_spawn`, ADR-3 + ADR-5)
```rust
// reader: Box<dyn Read + Send> from PtyAdapter::spawn
// on_output: Channel<Vec<u8>> (cheaply clonable handle)
// stop: Arc<AtomicBool>
let handle = std::thread::Builder::new()
    .name(format!("pty-read-{id}"))
    .spawn(move || {
        let mut coalescer = Coalescer::new(MAX_CHUNK, FLUSH_INTERVAL, Instant::now());
        let mut buf = [0u8; READ_BUF];               // reused, no per-iter alloc
        loop {
            if stop.load(Ordering::Relaxed) { break; }
            match reader.read(&mut buf) {
                Ok(0) => break,                       // EOF: shell exited
                Ok(n) => {
                    if let Some(chunk) = coalescer.push(&buf[..n], Instant::now()) { let _ = on_output.send(chunk); }
                    if let Some(chunk) = coalescer.drain_due(Instant::now())     { let _ = on_output.send(chunk); }
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        if let Some(chunk) = coalescer.drain_all() { let _ = on_output.send(chunk); }
        let _ = app.emit("pty_exit", PtyExit { id, code: None }); // low-freq lifecycle event
    })?;
```
> A blocking `read()` will not observe `stop` until it returns; `pty_kill` therefore ALSO calls
> `child.kill()` which closes the master and unblocks `read()` with EOF/err — `stop` is the
> belt-and-suspenders for the post-kill iteration. The thread is `JoinHandle`-tracked and joined in
> `pty_kill`/`Drop` so no detached thread leaks. (`drain_due` is also nudged by a short read-timeout
> if portable-pty's reader supports it; otherwise the time-flush rides on the next `read` return —
> acceptable for M1, validated against `htop`.)

---

## 4. Transport — exact wiring (ADR-2)

**Spawn handshake**:
1. FE constructs `const channel = new Channel<Uint8Array>()` and `channel.onmessage = (bytes) => term.write(bytes)`.
2. FE `invoke<string>("pty_spawn", { cols, rows, cwd, onOutput: channel })` → resolves to `PtyId`.
   (Tauri auto-maps the `on_output` Rust param ↔ `onOutput` JS key, camelCase.)
3. Rust opens the adapter, spawns the read thread holding the `Channel` clone, inserts `PtyState`
   under the new id, returns the id.

**Output path (high-freq)**: read thread → `Coalescer` → `Channel::send(Vec<u8>)` →
`channel.onmessage(bytes: Uint8Array)` → `term.write(bytes)`. xterm 6 `write()` accepts
`Uint8Array` directly — no string decode, no base64.

**Input path**: `term.onData((data: string) => sendInput(id, data))`. The TS wrapper encodes with
`new TextEncoder().encode(data)` → `number[]`/`Uint8Array` → `invoke("send_input", { id, data })`;
Rust receives `Vec<u8>` and `writer.write_all`. (onData already yields correct control sequences.)

**Resize path**: `ResizeObserver → fit() → invoke("pty_resize", { id, cols, rows })` → `master.resize` (SIGWINCH).

**Exit (low-freq)**: Rust `app.emit("pty_exit", { id, code })`; FE `listen("pty_exit", …)` marks the
pane closed. This is the ONLY event; everything high-frequency is on the Channel.

**Payload encoding decision (PINNED)**: raw `Vec<u8>` ↔ `Uint8Array` over the Channel. Base64 is the
documented fallback ONLY if Channel-binary proves awkward in practice. `number[]` is forbidden.

---

## 5. xterm.js Mount (React 19, no manual memo)

**`Terminal.tsx`** structure:
```tsx
import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import "@xterm/xterm/css/xterm.css";
import { useTerminal } from "../hooks/useTerminal";

const SCROLLBACK = 5000; // exit criterion: scrollback retained

export function Terminal() {
  const containerRef = useRef<HTMLDivElement>(null);
  useTerminal(containerRef);     // all spawn/IO/resize/cleanup orchestration lives in the hook
  return <div ref={containerRef} className="terminal-pane" />;
}
```
- Named React imports only; **no** `useMemo`/`useCallback`/`forwardRef` (React 19 compiler).
- Addons: `@xterm/addon-fit` (fit-to-container) + `@xterm/addon-clipboard` (OSC 52 programmatic copy).
- `new XTerm({ scrollback: SCROLLBACK, convertEol: false, cursorBlink: true })`.
- `ResizeObserver` on the container → `fitAddon.fit()` → read `term.cols/term.rows` →
  `resizePty(id, cols, rows)` (lightly debounced). Initial size sent at/just before `pty_spawn`.
- Cleanup (`useEffect` return): disconnect observer, `term.dispose()`, `killPty(id)`.

> The hook owns the imperative xterm lifecycle so the component stays declarative and the hook is the
> single vitest target (mirrors how `usePingPong` owns the invoke/listen wiring).

---

## 6. Cargo / Dependency Deltas (exact)

| Manifest | Add | Note |
|---|---|---|
| `crates/adapters/Cargo.toml` | `portable-pty = "0.9.0"` | WezTerm crate; openpty/ConPTY |
| `src-tauri/Cargo.toml` | `portable-pty = "0.9.0"` | registry/read-thread types |
| `crates/core/Cargo.toml` | **nothing** | the gate — stays `serde` + `thiserror` |
| `ui/package.json` deps | `@xterm/xterm ^6.0.0`, `@xterm/addon-fit ^0.11.0`, `@xterm/addon-clipboard ^0.2.0` | scoped `@xterm/*`, not legacy |

**cargo-deny core-scope guarantee**: `cargo deny --manifest-path crates/core/Cargo.toml check bans`
resolves ONLY core's closure; `portable-pty` never appears there, so the gate stays green with
**no `deny.toml` edit**. A tasks-level verify step asserts this explicitly.

---

## 7. Capabilities / Permissions (§ verify)

`capabilities/default.json` currently grants `["core:default", "core:event:default"]`.
- Custom `#[tauri::command]`s are invokable under `core:default` in v2 (no per-command ACL here).
- `tauri::ipc::Channel` rides the same core IPC path → no extra permission.
- `pty_exit` event uses `core:event:default` (already present).
- `@xterm/addon-clipboard` does **in-webview** OSC-52 handling — no Tauri clipboard plugin/permission.

**Expected delta: NONE.** Tasks include a verify step; if invoke is denied at runtime, add the
specific permission then (documented contingency, not a planned change).

---

## 8. Concurrency / Lifecycle

- **Read-thread shutdown**: `stop: Arc<AtomicBool>` + `child.kill()` (closes master → `read` returns
  EOF/err → loop exits). `JoinHandle` stored in `PtyState`; `pty_kill` sets stop, kills, then
  `join()`s. `Drop` for `PtyState` does the same (best-effort) so an app close doesn't leak threads.
- **Mutex poisoning**: command boundary returns `Err("pty registry mutex poisoned")` (no panic — a
  PTY thread panic must not brick the UI). Adapter internals hold no extra locks in M1.
- **Backpressure / cadence**: `Coalescer` hybrid flush (`MAX_CHUNK` ~8–16KB OR `FLUSH_INTERVAL`
  ~8–16ms). `READ_BUF` = 8KB, reused. Constants are M1 module consts, tuned empirically at acceptance.
- **Per-OS default shell**: `$SHELL` → `/bin/bash` (unix); `%COMSPEC%` → `cmd.exe`/PowerShell
  (windows, best-effort, not CI-gated — `deny.toml` graph is host-only, macOS-first).

---

## 9. Strict-TDD Plan (RED first)

**Rust — `cargo test --workspace`** (each test names its behavior, one assertion focus):
1. `coalescer_flushes_when_size_threshold_reached` — `push` of `max_chunk+k` returns a `max_chunk`
   chunk, remainder buffered.
2. `coalescer_does_not_flush_below_size_and_time` — small `push` under interval → `None`.
3. `coalescer_drain_due_flushes_after_interval` — inject `now = last_flush + interval` → buffered
   bytes returned; before interval → `None` (deterministic via injected `Instant`).
4. `coalescer_splits_oversized_push_at_max_chunk` — push ≫ `max_chunk` → exact-size chunk, rest kept.
5. `coalescer_drain_all_flushes_remainder_on_eof`.
6. `default_shell_prefers_env_shell` / `default_shell_falls_back_when_unset` (injected env getter).
7. `pty_spawn_config_shell_sets_program_and_size`.
8. **Command layer against `PtyTransport` fake**: `send_input_writes_bytes_to_transport`,
   `pty_resize_forwards_cols_rows`, `pty_kill_invokes_transport_kill`,
   `send_input_unknown_id_returns_err` — a `FakePtyTransport` records calls; no real PTY opened.
   (Command handlers are refactored to take `&mut dyn PtyTransport` via the registry so the fake
   substitutes cleanly.)

**TypeScript — `pnpm --filter ui test` (vitest run)** (mirror `usePingPong.test.ts`):
- `vi.mock("@tauri-apps/api/core", …)` exposing `invoke` spy + a fake `Channel` capturing `onmessage`.
- `vi.mock("@tauri-apps/api/event", …)` for `listen` (`pty_exit`).
- `vi.mock("@xterm/xterm", …)` returning a fake `Terminal` with `open/write/onData/dispose` spies;
  fake `FitAddon`.
- Assertions: `useTerminal_invokes_pty_spawn_on_mount`; `useTerminal_writes_channel_bytes_to_term`
  (fire fake channel message → `term.write` called with the bytes); `useTerminal_invokes_send_input_on_onData`;
  `useTerminal_invokes_pty_resize_on_fit`; `useTerminal_disposes_and_kills_on_unmount`.

**Manual acceptance checklist (cannot be unit-tested — `sdd-apply`/`sdd-verify` gate)**:
- [ ] `vim` opens, edits, and exits cleanly (alt-screen, cursor keys).
- [ ] `htop` renders and refreshes without tearing/lag (validates batching cadence).
- [ ] `git log --oneline --graph` renders colors/graph correctly.
- [ ] Resize the window → PTY reflows (SIGWINCH) and xterm re-fits.
- [ ] Scrollback retained after output exceeds the viewport (≥ `SCROLLBACK` lines).
- [ ] (best-effort) Windows ConPTY smoke if a Windows host is available.

---

## 10. Risks / Assumptions to validate

- **R1** Channel-binary `Vec<u8>` ↔ `Uint8Array` round-trip — confirm in apply; base64 fallback ready.
- **R2** Batching constants are placeholders; tune against `htop`/`vim` at acceptance.
- **R3** time-flush (`drain_due`) cadence depends on `read()` return frequency; if a quiescent PTY
  withholds the final partial flush, add a short reader read-timeout or a tick thread — decided in apply.
- **R4** ConPTY untested in CI (macOS-first); Windows best-effort.
- **R5** clippy `-D warnings`: `#[must_use]` on `Coalescer` returns + spawn handle, no `Vec` clone in
  the hot loop (buffer reused), justified poisoned-mutex handling.
- **R6** VibeLens `show_diff_explanation` is an apply-phase obligation (per CLAUDE.md).
