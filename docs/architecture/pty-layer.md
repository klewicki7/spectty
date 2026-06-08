# PTY Layer

The PTY layer is the lowest-level adapter in Spectty. It is responsible for spawning agent
processes in a real pseudo-terminal, streaming their output, and translating that stream
into two parallel views: raw bytes for the UI renderer and a decoded `OutputSignal` for
the status detector. It implements an internal `PtyPort` trait used by `AgentRunner` and
the Session supervisor.

See [overview.md](overview.md) for the dependency rule: the Core never imports
`portable_pty`. The PTY is adapter territory.

---

## Responsibilities

| Responsibility | Where |
|---|---|
| Spawn a `LaunchSpec` in a PTY | `PtyAdapter::spawn` |
| Async read loop — drive PTY output | Tokio task per Session |
| Decode raw bytes → `OutputSignal` | Inside the read loop, before forwarding |
| Write user keystrokes → PTY master | `PtyAdapter::write` |
| Resize (cols/rows) | `PtyAdapter::resize` |
| Kill / wait for process exit | `PtyAdapter::kill`, `PtyAdapter::wait` |

---

## Spawning a LaunchSpec

`LaunchSpec` comes from `AgentRunner::launch_spec()`. It carries the executable path,
arguments, environment variables, and working directory. `PtyAdapter` does not know which
agent it runs — it receives a `LaunchSpec`, nothing more. This is the anti-pattern guard
from [agent-abstraction.md](agent-abstraction.md).

```rust
// Shape only — not final signatures.
struct LaunchSpec {
    program: PathBuf,
    args: Vec<OsString>,
    env: HashMap<OsString, OsString>,
    cwd: PathBuf,
    cols: u16,
    rows: u16,
}

impl PtyAdapter {
    pub fn spawn(spec: LaunchSpec) -> Result<PtyHandle> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system.openpty(PtySize {
            cols: spec.cols,
            rows: spec.rows,
            ..Default::default()
        })?;
        let cmd = CommandBuilder::new(&spec.program)
            .args(&spec.args)
            .env_clear()
            .envs(&spec.env)
            .cwd(&spec.cwd);
        let child = pair.slave.spawn_command(cmd)?;
        Ok(PtyHandle { master: pair.master, child, .. })
    }
}
```

`portable-pty` is the same crate used by WezTerm. It abstracts `openpty(2)` on POSIX and
`ConPTY` on Windows, so Spectty inherits that cross-platform work for free.

---

## Async read loop

The PTY master's read side is blocking I/O. Tokio's `spawn_blocking` bridges it into the
async world without blocking the runtime thread pool.

```
┌───────────────────────────────────────────────────────────────────────┐
│  Tokio task: PTY Read Loop (one per Session)                          │
│                                                                       │
│  spawn_blocking ──▶ read(pty_master) → Vec<u8>                        │
│                              │                                        │
│                    ┌─────────▼──────────┐                             │
│                    │  decode_output()   │  strip/interpret ANSI       │
│                    │  → OutputSignal    │  accumulate text window     │
│                    └────┬──────────┬───┘                              │
│                         │          │                                  │
│              raw bytes  │          │  OutputSignal                    │
│                         ▼          ▼                                  │
│           pty_output    │    status_tx (mpsc)                         │
│           Tauri event   │    → AgentRunner::detect_status()           │
│           (→ xterm.js)  │    → CostMetrics update                    │
└─────────────────────────┴──────────────────────────────────────────── ┘
```

Two consumers receive data from each read:

1. **Raw bytes → Tauri event `pty_output`** — sent as-is to the UI, where xterm.js
   renders them with full ANSI fidelity. No processing here.
2. **`OutputSignal` → agent status / cost pipeline** — decoded, windowed, passed to the
   per-agent `AgentRunner` impl for status detection and cost parsing.

The two paths are independent. The UI can render blazing-fast because it never waits for
status detection. Status detection never sees raw ANSI — it sees a structured signal.

---

## `OutputSignal` — the decoded view

`OutputSignal` is what the Core (via `AgentRunner`) sees. The PTY adapter produces it;
the Core consumes it. Raw ANSI never crosses the adapter boundary.

```rust
struct OutputSignal {
    /// Printable text of the recent output window (ANSI stripped).
    text_window: String,
    /// Whether the process is currently producing output (I/O active).
    is_active: bool,
    /// Process exit code if the child has exited.
    exit_code: Option<i32>,
    /// Monotonic timestamp of the most recent byte received.
    last_byte_at: Instant,
}
```

The `text_window` is a rolling buffer of the last N characters of printable text (ANSI
escape sequences stripped). The size is bounded (e.g. 4 KB) to keep detection cheap.
`is_active` toggles based on whether bytes arrived within a short quiesce window (e.g.
200 ms) — used by the Generic adapter's idle-timeout heuristic.

ANSI stripping uses a lightweight state machine, not a full VT parser. The goal is clean
text for regex matching, not pixel-accurate rendering — xterm.js handles the latter.

> ❓ OPEN: Decide the `text_window` size and quiesce threshold empirically against real
> Claude Code output. 4 KB / 200 ms are starting estimates.

---

## Write path — user keystrokes to PTY

User input (keystrokes from xterm.js) arrives as a `send_input` Tauri command, carrying
UTF-8 bytes. The PTY adapter writes them directly to the PTY master:

```rust
impl PtyAdapter {
    pub fn write(&self, data: &[u8]) -> Result<()> {
        self.master.write_all(data)?;
        Ok(())
    }
}
```

No buffering, no transformation. The agent's readline/tty handling inside the PTY takes
care of echoing, line editing, and control sequences. Spectty is transparent here.

---

## Resize handling

When the xterm.js fit addon recalculates the terminal size (on window resize or pane
split), it reports the new `cols` and `rows` to the UI, which sends a `resize_pty` Tauri
command. The adapter calls:

```rust
impl PtyAdapter {
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(PtySize { cols, rows, ..Default::default() })?;
        Ok(())
    }
}
```

The SIGWINCH signal is sent to the child process by `portable-pty` automatically when
`resize` is called on the master. Most CLI agents respond to SIGWINCH and reflow their
output accordingly.

---

## Lifecycle

```
spawn() ──▶ Running read loop
               │
               ├──▶ [output] → dual path (raw / OutputSignal)
               │
               ├──▶ process exits naturally → exit_code Some(0) → AgentStatus::Completed
               │
               ├──▶ process exits non-zero → exit_code Some(n) → AgentStatus::Error
               │
               └──▶ kill() called → SIGKILL → read loop drains → returns
```

`kill()` sends SIGKILL (or TerminateProcess on Windows). The read loop drains any
remaining bytes, then exits. The Session supervisor transitions the status to `Error` on
a non-clean kill unless the Session was explicitly closed by the user (in which case it
transitions to `Completed`).

On **crash** (unexpected process death), the read loop detects `exit_code.is_some()` with
a non-zero code and emits a final `OutputSignal` with `exit_code` set. The supervisor
transitions to `Error` and the `NotifierPort` fires.

---

## Per-Session isolation

Each Session has its own `PtyAdapter` instance and its own Tokio task. There is no shared
PTY state between Sessions. A crash or stall in one Session's read loop cannot affect
another.

The `PtyHandle` is owned by the Session and dropped when the Session closes. Dropping it
closes the PTY master file descriptor, which sends EOF to the slave, which is received by
the agent process as a hangup.

---

## Backpressure and buffering

PTY output can burst (e.g. an agent printing a large file diff). The read loop uses an
internal ring buffer to absorb bursts before emitting Tauri events. Tauri events to the
UI are sent at a rate-limited cadence (e.g. up to 60 frames/s worth of batching) to
avoid flooding the webview's IPC channel.

The `OutputSignal` path is decoupled: it receives updates on a bounded `mpsc` channel.
If the status detector is slower than the PTY output rate, older signals are dropped
(only the latest window matters for detection — we do not accumulate a queue of signals).

> ❓ OPEN: Validate the IPC batching strategy (event rate cap vs. byte-threshold batching)
> against observed xterm.js rendering performance under heavy output loads.
