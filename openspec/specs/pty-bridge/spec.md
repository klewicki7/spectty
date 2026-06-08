# Capability: pty-bridge

> Living baseline spec. Established by change `M1-live-pty-terminal` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The Tauri bridge exposes the PTY lifecycle as commands, streams output over a per-spawn
`ipc::Channel`, and emits a low-frequency lifecycle exit event. It owns the live PTY behind
a registry-shaped Tauri state. This extends the M0 `tauri-bridge` baseline with the PTY
command surface; it reuses the same v2 emit convention.

## Requirement: Bridge exposes the PTY lifecycle commands
The `src-tauri` bridge MUST expose, as Tauri v2 commands registered in
`generate_handler!`, the full PTY lifecycle: `pty_spawn` (open a shell; returns a PTY id),
`send_input` (forward input bytes/string to the PTY), `pty_resize` (apply new cols/rows),
and `pty_kill` (terminate the PTY by id). Commands MUST take owned types only (no `&str` in
async commands) and MUST return `Result<_, _>` with a serializable error, following the M0
`ping` convention.

### Scenario: All four PTY commands are registered in the handler
- **Given** the `src-tauri` `generate_handler!` registration
- **When** the registered command set is inspected
- **Then** `pty_spawn`, `send_input`, `pty_resize`, and `pty_kill` MUST each be present
  (an unregistered command silently fails at invoke, so registration is the guard)

### Scenario: pty_spawn returns a PTY id that the other commands key on
- **Given** a `pty_spawn` invocation
- **When** it succeeds
- **Then** it MUST return a PTY id AND `send_input` / `pty_resize` / `pty_kill` MUST each
  accept that id to target the spawned PTY

## Requirement: Output streams over a per-spawn Channel; lifecycle exit is an event
High-frequency PTY output MUST be carried over a `tauri::ipc::Channel` of raw bytes opened
per `pty_spawn`, NOT over the global Tauri event bus. Low-frequency lifecycle MUST be a
Tauri v2 event (e.g. `pty_exit { code }`) emitted via the `Emitter` trait (matching the M0
v2 emit convention), so the UI can observe child exit.

> Wire-shape note (resolved risk R1): a bare `Vec<u8>` sent over a Tauri v2
> `ipc::Channel` arrives on the JS side as a `number[]` (JSON array), NOT a `Uint8Array`.
> The frontend MUST decode defensively (see `terminal-ui`). The raw-`Response` path would
> yield an `ArrayBuffer`; the baseline ships the bare-`Vec<u8>` path with FE decode.

### Scenario: Output uses a Channel, lifecycle uses an event
- **Given** the bridge transport wiring
- **When** the output path and the exit path are inspected
- **Then** output MUST flow through a per-spawn `ipc::Channel` of raw bytes AND the exit
  notification MUST be a Tauri v2 event using `AppHandle::emit` via `Emitter` (NOT a removed
  v1 emit signature)

### Scenario: PTY exit emits the lifecycle event exactly once (manual acceptance)
- **Given** a running PTY whose shell process exits
- **When** the child terminates
- **Then** a `pty_exit` event carrying the exit code MUST be emitted exactly once AND the UI
  MUST be able to observe it

## Requirement: Bridge owns the PTY behind a registry-shaped state
The bridge MUST own the live PTY behind a registry-shaped `tauri::State` keyed by PTY id
(conceptually `Mutex<HashMap<PtyId, PtyState>>`), even though M1 holds exactly one entry.
Each entry MUST retain the writer, the child handle (for kill), and the read-thread stop
handle. This state is a Tauri-state seam; it MUST NOT be modeled as the Core
`SessionRegistry` entity (that is deferred to M2). Mutex poisoning MUST follow the M0
`in_memory` convention (justified `.expect`, or mapped to a serializable command error); a
poisoned registry MUST NOT crash the UI.

### Scenario: State is keyed by PTY id (registry shape)
- **Given** the Tauri state that owns the PTY
- **When** its shape is inspected
- **Then** it MUST be keyed by PTY id so commands look the PTY up by id, AND it MUST NOT
  import or depend on a Core `SessionRegistry` type
