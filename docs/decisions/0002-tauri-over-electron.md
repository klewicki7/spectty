# ADR-0002: Tauri + Rust over Electron + Node

- Status: Accepted
- Date: 2026-06-07
- Deciders: project owner

## Context

[ADR-0001](0001-gui-over-tui.md) established that Spectty is a GUI desktop app. The next
question is what runtime powers that app.

The backend has concrete, non-trivial demands:

- **PTY management at scale.** 3–5 agent sessions means 3–5 concurrent PTYs, each with
  read/write loops and backpressure. PTY errors in one session must not crash others.
- **Worktree orchestration.** `git worktree add`, file-watching per worktree, debounced
  diff-explain triggers. This is IO-heavy and concurrent.
- **Long-lived process.** Spectty stays open all day. RAM and idle CPU matter continuously,
  not just at startup.
- **File-watching.** One `notify` watcher per Session's Worktree root, debounced, fanned
  out to the diff-explain pipeline.

The frontend is fixed: React + xterm.js. That is the same in any webview-based desktop
framework — Tauri and Electron both deliver a Chromium-backed webview for xterm.js.

## Decision

Use **Tauri** with a **Rust** backend.

The Tauri bridge (commands and events) is the only boundary between the backend and the
UI. The Core and all Adapters are written in Rust. See
[Architecture Overview](../architecture/overview.md) for the layer diagram.

Key reasons:

1. **`portable-pty` is battle-tested Rust.** It is the PTY crate behind WezTerm — one of
   the most capable terminal emulators available. Node-based PTY libraries are solid but
   `portable-pty` has a deeper track record for edge cases (resize, signal handling,
   platform quirks).

2. **Rust's ownership model + Tokio make concurrent session management safe.** Each
   Session's async task tree (PTY loop, status detector, file-watch pipeline) is isolated
   at the type level. A panic in one task does not propagate to others without explicit
   supervision. Achieving the same isolation in Node requires careful worker thread
   management and is less structurally enforced.

3. **RAM and startup.** A Tauri app ships the system webview (macOS: WebKit, Linux: WebKitGTK,
   Windows: WebView2); there is no bundled Chromium. Baseline RAM for the Rust process is
   order-of-magnitude lower than an Electron app. For a tool open all day with multiple
   sessions, this is a real quality-of-life difference, not a benchmark vanity metric.

4. **Hexagonal architecture fits Rust.** Ports are traits; Adapters are structs that
   implement them; dependency injection happens at `App` construction. Rust's type system
   enforces the dependency rule at compile time — if a Core type accidentally imports
   `tauri`, it will not compile with the correct module boundaries. See
   [ADR-0003](0003-hexagonal-architecture.md).

## Consequences

**Positive**
- Lower steady-state RAM (~50–80 MB Rust process vs. ~150 MB+ Electron baseline).
- Faster cold start (no V8 initialization for the backend logic).
- `portable-pty` handles terminal edge cases that have already been solved for WezTerm.
- Concurrent session safety is structurally enforced by Rust's type system and Tokio's
  task model.
- No bundled Chromium; smaller distributable (~5–15 MB Rust binary + frontend assets vs.
  ~150 MB Electron bundle).

**Negative**
- **FFI friction.** Anything not natively available in Rust (e.g. a JS library with no
  Rust equivalent) requires a Tauri command round-trip or an FFI binding. Node libraries
  are easier to prototype with.
- **Rust compile times.** Incremental compiles are fast, but cold builds and heavy
  dependency changes are slower than Node.
- **Smaller ecosystem for desktop niceties.** Auto-update, crash reporting, and analytics
  SDKs have more polished Electron integrations. Tauri's plugin ecosystem is growing but
  younger.
- **Fewer contributors can touch the backend.** Rust expertise is less common than Node.
  This is a real hiring/contributor constraint.
- WebKitGTK on Linux can be a packaging headache (version skew across distros).

**Neutral**
- The xterm.js integration is identical in both frameworks — it lives in the React layer
  and talks to the backend via a bridge regardless.
- The Tauri bridge (commands/events) plays the same role as Electron's IPC; the
  communication pattern is familiar.

## Alternatives considered

### Electron + Node (node-pty)

The fastest path to an MVP. `node-pty` is mature, the Electron ecosystem has every
desktop integration library imaginable, and the entire team likely knows JavaScript/Node
already.

**Why not chosen:** Electron bundles Chromium — ~150 MB in the distributable and
~150 MB+ of RAM for a process that stays open all day. For a supervision tool running 5
parallel sessions, that overhead is felt continuously. More importantly, achieving safe
concurrent PTY management and worktree orchestration in Node requires careful architecture
that Rust gives structurally. The PTY isolation, file-watching, and git operations are
exactly the kind of IO-heavy concurrent workloads where Rust's ownership model prevents
whole classes of bugs (use-after-free in PTY buffers, data races across session tasks).
The short-term productivity gain of Node does not outweigh the long-term operational cost.

### Go + webview2 / Wails

Go has good concurrency primitives and the `creack/pty` library. Wails is the Go
equivalent of Tauri.

**Why not chosen:** The PTY story in Go is less battle-tested than `portable-pty`, and
Go lacks Rust's compile-time memory safety guarantees for the concurrent task isolation
we need. Adding Go as a second backend language would also fragment the codebase without
a clear upside — the domain model and hexagonal structure we want map naturally to Rust
traits and structs.
