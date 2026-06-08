# ADR-0001: GUI desktop app over TUI multiplexer

- Status: Accepted
- Date: 2026-06-07
- Deciders: project owner

## Context

Spectty's core workflow is **supervising AI coding agents**, not typing shell commands.
That workflow needs:

1. **VibeLens panel** — a per-session, live explained-diff view that is always visible
   alongside the terminal. It needs to render structured, per-file summaries with rich
   formatting (syntax-highlighted diffs, status badges, cost readouts).
2. **Dashboard** — a glanceable cross-session overview that surfaces `AgentStatus` at a
   glance and pulses when an agent is `AwaitingInput`. At 3–5 agents in parallel, spatial
   layout and color are load-bearing, not decorative.
3. **Split panes, tabs, and keyboard navigation** — these exist in both GUI and TUI, but
   in a GUI they can coexist with panels that go beyond text cells.

A TUI can render text. It cannot render a sidebar with proportional layout, variable-width
columns, or non-cell-aligned UI elements without heroic workarounds that ultimately
recreate a subset of a real layout engine — with maintenance cost and ceiling hit.

The **terminal emulator itself** (xterm.js) is a solved problem; we embed it. The product
value lives entirely in the agent-aware layer around it.

## Decision

Build Spectty as a **native GUI desktop application** (Tauri + React/xterm.js), not as a
TUI multiplexer à la tmux or Zellij. The terminal area is one rendered region inside the
app; the VibeLens panel, Dashboard, and navigation chrome are separate UI regions that
share the same window.

## Consequences

**Positive**
- VibeLens panel has no layout constraints; can show per-file summaries, line counts, AI
  rationale, and status badges with real typography.
- Dashboard can use color, spatial position, and animation to surface `AwaitingInput`
  states without encoding everything into text characters.
- Future features (agent picker, diff review, one-click merge, Checkpoint restore) are
  standard UI patterns rather than TUI hacks.
- xterm.js gives near-perfect terminal emulation out of the box; no need to reimplement a
  terminal state machine in Rust/Go.

**Negative**
- Heavier than a TUI: ships with a webview, a Tauri runtime, and a Rust backend. Binary
  size and startup time are higher.
- **Does not run over plain SSH.** A user ssh-ing into a remote box cannot use Spectty's UI
  there (only the agent CLI itself). If remote-first supervision becomes a requirement, a
  separate web UI or a forwarded Tauri window would be needed.
- Requires OS-level webview (macOS/Linux/Windows). No headless server use case.
- Contributing to the frontend requires knowing React, not just Rust.

**Neutral**
- Keyboard-first navigation is still the design intent; a mouse-free workflow is
  achievable in a GUI.
- The terminal emulation quality is equivalent to or better than most TUIs because
  xterm.js is the same engine behind VS Code's integrated terminal.

## Alternatives considered

### TUI in Rust + ratatui

A terminal-native multiplexer written in Rust using
[ratatui](https://github.com/ratatui-org/ratatui). Lighter binary, works over SSH,
familiar to CLI-first users.

**Why not chosen:** ratatui is cell-based. The VibeLens panel is the product's
differentiating feature — it must show rich per-file diff explanations that look like a
sidebar, not a scrolling log. Approximating that in a text grid is possible but produces
a degraded experience that undermines the product's thesis. The Dashboard's spatial
"which agent is blocked" scan also benefits from proportional layout. We would have spent
significant effort recreating a layout engine that Tauri/React give us for free.

### TUI in Go + Bubbletea

Same tradeoffs as ratatui. Go's concurrency model is friendlier than Rust for some teams,
but the fundamental text-cell ceiling is the same. Additionally, the rest of the backend
(pty, git, file-watching) is Rust — a Go TUI would add a second language boundary without
solving the layout problem.

### tmux / Zellij plugin

Extending an existing multiplexer (e.g. a Zellij plugin that renders a VibeLens pane).
Rejected early: we would be working inside another program's layout model, plugin API, and
release cadence. The agent-awareness we need permeates the entire session lifecycle, not
just one pane — you cannot own the supervision layer as a plugin.
