# Stack Decisions

The technology choices below are **locked** for the foreseeable future. Each entry
explains the *why* so future contributors understand the constraints, not just the
selections. For the architectural framing that governs these choices, see
[overview.md](overview.md) and [ADR-0003](../decisions/0003-hexagonal-architecture.md).

---

## Tauri — desktop shell

> See [ADR-0002](../decisions/0002-tauri-over-electron.md) for the full comparison.

Tauri wraps the system's native WebView (WebKit on macOS, WebView2 on Windows) instead of
bundling Chromium. The tradeoffs that matter for Spectty:

| Dimension | Tauri | Electron |
|---|---|---|
| Binary size | ~10 MB | ~150 MB |
| Memory baseline | ~30 MB | ~100 MB |
| OS WebView quirks | Yes (CSS/JS parity risk) | None (Chromium) |
| Native Rust backend | First-class | Foreign (Node subprocess) |
| PTY / process control | Rust directly | Node child_process |

Spectty's value is the Rust backend — PTY management, git, file watching, MCP client. Tauri
makes Rust the primary language with the UI as a thin renderer, not an afterthought. The
webview rendering surface is also predictable enough for a terminal UI: xterm.js does its
own canvas/WebGL rendering and is not affected by CSS quirks.

---

## Rust + Tokio — backend runtime

Rust is chosen for three concrete reasons:

1. **PTY and process control without a GC pause.** Terminal throughput requires handling
   bursts of PTY output at speed. GC pauses in Java/Go would introduce jitter visible as
   terminal stutter.
2. **`portable-pty` is Rust.** The best cross-platform PTY crate is native Rust; avoiding
   a language boundary simplifies the integration.
3. **Memory safety without a runtime.** Multiple concurrent Sessions (each with read loops,
   file watchers, and MCP calls) in a single process benefit from Rust's ownership model
   — no shared-mutable-state bugs at the cost of a GC.

**Tokio** is the async runtime because:
- `portable-pty` wraps blocking I/O in threads; Tokio's `spawn_blocking` bridges this cleanly.
- All adapters (file watch, git, MCP HTTP) are async-native or easy to wrap.
- One runtime per process; Sessions share the thread pool without spawning OS threads per Session.

The Hexagonal Core is pure synchronous Rust (no `async` in domain types). Only adapters
are async. This keeps the domain testable without `#[tokio::test]` boilerplate everywhere.

---

## React + Vite + xterm.js — UI layer

**React** is the component model. The UI is event-driven and read-mostly (it renders state
pushed from the backend); React's unidirectional data flow maps naturally to that shape.
No server-side rendering, no Next.js — Vite produces a static bundle that Tauri loads.

**Vite** is the build tool: fast HMR during development, straightforward Tauri integration
via `@tauri-apps/cli`.

**xterm.js** renders the terminal surface. It is the standard in the space: VS Code's
terminal, Hyper, and Wezterm's web UI all use it or a variant. Key reasons:

- WebGL renderer (`xterm-addon-webgl`) for high-throughput output without CPU bottleneck.
- Canvas renderer (`xterm-addon-canvas`) as fallback on systems without WebGL.
- Fit addon (`xterm-addon-fit`) for PTY resize: measures the container's pixel dimensions
  and reports correct `cols`/`rows` to the backend `resize_pty` command.
- Search addon (`xterm-addon-search`) for in-terminal text search.
- Serializes and writes raw PTY bytes directly — the backend streams them without
  transformation, keeping latency minimal.

xterm.js does **not** parse ANSI for domain purposes. Any ANSI interpretation needed for
status detection happens in the Rust `PtyAdapter`, not the UI. See [pty-layer.md](pty-layer.md).

> ❓ OPEN: Confirm the `xterm-addon-webgl` version and whether it is stable enough under
> Tauri's WebKit on macOS/Sonoma. Canvas renderer must be the documented fallback.

---

## `notify` crate — file watching

The `notify` crate provides cross-platform filesystem event notifications (FSEvents on
macOS, inotify on Linux, ReadDirectoryChangesW on Windows). It is the de facto standard
for Rust file watching.

Spectty wraps it behind `FileWatchPort` so the Core sees only `FileChanged { path, kind }`
events. The adapter handles debouncing (500 ms default window) before emitting to the port.
This prevents the diff pipeline from firing on every intermediate write during a rapid
multi-file edit.

> ❓ OPEN: Pin the `notify` version once the crate stabilises its v6/v7 async API surface.

---

## Git: `git2` vs. shelling out

Two options for git operations (worktree creation, diff extraction, merge, branch listing):

| | `git2` (libgit2 bindings) | Shell out to `git` CLI |
|---|---|---|
| API | Strongly typed Rust | String-based, error-prone |
| Dependencies | libgit2 C library (static link) | Requires git installed on host |
| Diff quality | Lower-level, more control | Matches user's git config (diff drivers, `.gitattributes`) |
| Worktree ops | `libgit2` worktree support is incomplete as of 2024 | Full support |
| Portability | No git binary needed | git must be in PATH |

**Recommendation:** use `git2` for read-heavy operations (status, diff extraction, branch
listing) and **shell out to `git` CLI for worktree operations** (`git worktree add/remove`,
`git merge`). Worktree support in libgit2 is the documented weak point; using the CLI for
those operations avoids brittle workarounds while keeping diff parsing in `git2` for type
safety.

Both live behind `GitPort` — the Core does not know which is used.

> ❓ OPEN: Validate `git2` worktree limitations against the worktree lifecycle defined in
> [session-worktree-model.md](session-worktree-model.md). If `git2` adds full support,
> consolidate to pure `git2`.

---

## MCP client — VibeLens integration

Spectty embeds an MCP client to call the VibeLens `show_diff_explanation` tool. The client
lives in the `McpAdapter` and is the concrete implementation of `DiffExplainerPort`.

**Approach:** a lightweight HTTP/JSON-RPC client — no heavy MCP SDK needed. The VibeLens
MCP surface is a single tool invocation; a thin async HTTP client (`reqwest`) calling the
local MCP server suffices. If the MCP spec requires session/capability negotiation,
implement only the required subset.

> ❓ OPEN: Confirm whether VibeLens runs as a local stdio-mode MCP server (in which case
> the client spawns it as a subprocess and communicates over stdin/stdout) or as an HTTP
> server. This changes the transport but not `DiffExplainerPort`'s shape.

> ❓ OPEN: Evaluate `rmcp` (Rust MCP SDK) vs. hand-rolled JSON-RPC over `reqwest`. If
> `rmcp` is stable and actively maintained, prefer it to avoid re-implementing protocol
> negotiation.

---

## OS notifications

OS notifications are the delivery mechanism for the `AwaitingInput` and `Error` states —
the two states that require immediate human attention.

**Approach:** the `notify-rust` crate provides cross-platform desktop notifications
(libnotify on Linux, NSUserNotification / UNUserNotificationCenter on macOS, Windows Toast
on Windows). It wraps behind `NotifierPort` so the Core emits domain events and the
adapter handles the OS API.

> ❓ OPEN: Tauri ships its own notification plugin (`tauri-plugin-notification`). Evaluate
> whether using the Tauri plugin is preferable to `notify-rust` for consistency with the
> Tauri permission model. Decision deferred; both are valid behind `NotifierPort`.

---

## engram — persistence backend

> See [ADR-0005](../decisions/0005-build-on-gentle-ai-stack.md) for the build-on-stack
> rationale.

Spectty uses **engram** as the concrete persistence store behind `PersistencePort`. The
Core never imports engram; all access flows through the `EngramAdapter`.

| Dimension | Detail |
|---|---|
| License | MIT — no restrictions |
| Interface | Dual: stdio-mode MCP server **and** HTTP API at `:7437` |
| Storage | SQLite + FTS5 — local, zero-ops, full-text searchable |
| Scope | Runtime dependency (adapter layer only) |
| Core coupling | None — engram is invisible to the Hexagonal Core |

**Why engram?** The gentle-ai/engram stack already provides a proven, portable,
file-based persistence layer with full-text search. Building on it avoids designing
a bespoke store from scratch and gives Spectty's Spec and progress artifacts the same
searchable persistence that gentle-ai tools use for memory.

**Build-on rationale:** Spectty adopts engram's storage and gentle-ai's provisioning
patterns (per-agent, managed-marker injection). Spectty's provisioner **coexists** with
gentle-ai's own provisioner — each owns distinct marker regions. See
[stack-integration.md](stack-integration.md) for coexistence details.

**Key gap — pub/sub:** engram is a **store, not an event bus**. It has no native
subscribe/push mechanism. The `EngramAdapter` bridges this with a polling layer that
queries `:7437` on a short interval and fans Spec/progress updates into Core domain
events. This is the #1 technical risk in the persistence stack. See
[data-flow.md](data-flow.md) for the event pipeline design.

> ❓ OPEN: Determine the safe polling interval for the engram HTTP API under realistic
> Spec update rates (agent writes a task-state update every few seconds during apply).
> A long-poll or webhook endpoint in engram would eliminate this concern — evaluate
> whether engram's roadmap includes one.

---

## Crate / dependency table

| Concern | Crate / library | Layer |
|---|---|---|
| Desktop shell | `tauri` v2 | Tauri |
| Async runtime | `tokio` (full features) | Adapters |
| PTY | `portable-pty` | `PtyAdapter` |
| File watching | `notify` | `FileWatcher` adapter |
| Git (read/diff) | `git2` | `GitAdapter` |
| Git (worktrees/merge) | `git` CLI via `std::process` | `GitAdapter` |
| MCP client | `reqwest` + JSON-RPC (or `rmcp`) | `McpAdapter` |
| Persistence | engram daemon via HTTP `:7437` | `EngramAdapter` |
| Agent provisioning | `ProvisionerAdapter` (file I/O, managed markers) | `ProvisionerAdapter` |
| OS notifications | `notify-rust` or `tauri-plugin-notification` | `NotifierAdapter` |
| Serialization | `serde` + `serde_json` | All layers |
| Error handling | `anyhow` (adapters), `thiserror` (Core) | All layers |
| UI framework | React 19 + Vite | UI |
| Terminal renderer | xterm.js | UI |
| PTY resize | xterm-addon-fit | UI |
| Terminal renderer | xterm-addon-webgl / xterm-addon-canvas | UI |
| Terminal search | xterm-addon-search | UI |

**Known baselines:** Rust 1.89, Node 23, pnpm.

> ❓ OPEN: Lock specific semver pins for all crates once the prototype validates the
> integration points (especially `portable-pty`, `notify`, `git2`, `tauri` v2).
