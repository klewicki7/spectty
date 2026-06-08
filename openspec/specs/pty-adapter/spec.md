# Capability: pty-adapter

> Living baseline spec. Established by change `M1-live-pty-terminal` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

A PTY adapter in `crates/adapters` owns a real pseudo-terminal: it spawns a shell, streams
output bytes, accepts input, resizes, and kills the process. It is a concrete adapter — it
implements no Core trait. It uses `portable-pty`; that dependency lives in the adapter
layer, never in `spectty-core`.

## Requirement: Adapter spawns a shell in a real PTY
The PTY adapter MUST be able to open a real pseudo-terminal (via `portable-pty`) and spawn
a shell process attached to it, given a spawn input carrying program, working directory,
and initial size (columns and rows). The spawn input MUST be a minimal internal type
(program + cwd + cols + rows); it MUST NOT be named or shaped as an agent type (no
`LaunchSpec`, no `AgentSpec`) — that is the M2 anti-pattern guard.

### Scenario: Spawn-input construction assembles the correct command (pure, no PTY)
- **Given** a spawn input with a program, a working directory, and an initial size
- **When** the spawn-input-to-command construction runs
- **Then** it MUST produce command inputs (program, cwd, size) matching the spawn input,
  asserted on a small intermediate struct AND NOT on `portable-pty` internals (so the test
  needs no real PTY)

### Scenario: Default shell falls back per operating system
- **Given** the platform's default-shell resolution
- **When** no explicit program is provided
- **Then** on Unix it MUST resolve `$SHELL`, falling back to `/bin/bash`, AND on Windows it
  MUST resolve to `cmd.exe` or PowerShell

## Requirement: Adapter streams output bytes off a dedicated read thread
The adapter MUST read PTY output on a dedicated `std::thread` per PTY (NOT on a Tauri
command's async context and NOT via `tokio::spawn_blocking`), so the blocking read loop
never blocks the async runtime. Output MUST be delivered as raw bytes (`Vec<u8>` shape),
NOT as `number[]` and NOT, by default, base64-encoded.

### Scenario: Output is delivered as raw bytes
- **Given** the adapter's output transport
- **When** a chunk of PTY output is forwarded toward the UI
- **Then** it MUST be carried as raw bytes (a binary payload), AND MUST NOT be encoded as a
  JSON array of integers (`number[]`); base64 is permitted ONLY as an explicit fallback

## Requirement: Read loop coalesces output before emitting (hybrid size-OR-time flush)
The read loop MUST NOT emit one IPC message per read syscall. A pure coalescer/batcher MUST
accumulate bytes and flush on whichever fires first: a size threshold OR a time tick. The
flush interval and maximum chunk size MUST be M1 constants. The coalescing logic MUST be a
pure unit (no PTY, no Tauri) so its flush behavior is RED→GREEN testable.

### Scenario: Coalescer flushes when the size threshold is reached
- **Given** a coalescer with a configured size threshold
- **When** pushed bytes accumulate to or beyond that threshold
- **Then** it MUST drain a chunk AND that chunk MUST NOT exceed the configured maximum chunk
  size (a larger accumulation MUST be split into max-size chunks)

### Scenario: Coalescer flushes on the time tick even below the size threshold
- **Given** a coalescer holding fewer bytes than the size threshold
- **When** a flush tick fires
- **Then** it MUST drain the buffered bytes (time-based flush) so output is not stalled
  waiting to fill the buffer

### Scenario: Coalescer with no buffered bytes flushes nothing
- **Given** a coalescer with an empty buffer
- **When** a flush tick fires
- **Then** it MUST NOT emit an empty chunk

## Requirement: Buffered output MUST flush within a bounded interval even when the PTY is quiescent
The time-based flush MUST fire on a fixed cadence that is INDEPENDENT of read-syscall
frequency. When the child writes a small burst and then blocks (a quiescent PTY — e.g. a
shell that emits a cursor-position query `ESC[6n` or a prompt fragment, then waits for
input), the buffered bytes MUST be flushed toward the UI within the bounded flush interval;
they MUST NOT be stranded until the next read unblocks. This is the closure of design risk
R3: an implementation that only drains on read-return is non-conformant because it stalls
DSR/CPR-driven prompts (atuin, starship), interactive echo, and tab-completion output.

> Shipped resolution (this baseline): reading and coalescing are decoupled — a dedicated
> read thread forwards each slice over an `mpsc` channel, and a forwarder thread owns the
> coalescer and loops on `recv_timeout(FLUSH_INTERVAL)`, so a `Timeout` drives the time
> flush while the PTY is silent. The per-message forwarder decision is a pure, unit-testable
> function. See archived change `M1-live-pty-terminal` (PR5) and the quiescent-flush pattern.

### Scenario: A lone small write is not stranded while the PTY is quiescent
- **Given** a live PTY whose child writes a small amount of output and then blocks waiting
  for input (no further reads will return until input arrives)
- **When** the bounded flush interval elapses with the PTY silent
- **Then** the buffered bytes MUST be flushed toward the UI within that interval (the
  time-based flush MUST fire on its own cadence, NOT gated on the next read returning)

### Scenario: The quiescent flush emits nothing when the buffer is empty
- **Given** a quiescent PTY with an empty coalescer buffer
- **When** the flush interval elapses
- **Then** no chunk MUST be emitted (a silent PTY with nothing buffered MUST NOT produce a
  spurious empty message)

## Requirement: Adapter accepts input, resizes, and kills
The adapter MUST expose write (forward input bytes to the PTY master), resize (apply a new
columns/rows size, which MUST raise SIGWINCH on POSIX), and kill (terminate the child and
stop the read thread). These operations MUST be reachable through a minimal internal
adapters-level transport seam (e.g. `PtyTransport { write, resize, kill }`) so the command
layer can be tested against a fake WITHOUT opening a real PTY. This seam is an adapters
concern; it MUST NOT be a Core port.

### Scenario: Command-layer operations drive the transport via a fake (no PTY)
- **Given** a fake transport implementing the internal `write` / `resize` / `kill` seam
- **When** the command-layer handlers for input, resize, and kill run against the fake
- **Then** each handler MUST invoke the corresponding transport operation with the inputs it
  received, asserted without a real PTY

### Scenario: Read-thread shutdown joins cleanly with no leaked thread
- **Given** a spawned PTY with a running read thread (and its forwarder)
- **When** the PTY is killed (or dropped)
- **Then** the child MUST be killed first (closing the master so the blocking read returns
  EOF) AND the read thread MUST be joined, so no thread is leaked and no shutdown path
  deadlocks; a `pty_exit` lifecycle notification MUST fire exactly once per spawn

### Scenario: Resize applies the new size to the PTY (manual acceptance)
- **Given** a live PTY running an interactive program
- **When** a resize to new columns/rows is applied
- **Then** the PTY MUST receive the new size AND SIGWINCH MUST be raised so the program
  reflows to the new dimensions
