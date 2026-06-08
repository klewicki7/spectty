# Project Structure

Spectty is a Cargo workspace monorepo. The layout reflects the hexagonal split directly:
domain Core in one crate, Adapters in another, Tauri shell in a third, React UI alongside.

---

## Folder tree

```
spectty/
├── Cargo.toml                  # workspace root — lists all crates
├── package.json                # pnpm workspace root
├── pnpm-workspace.yaml
│
├── crates/
│   ├── core/                   # pure domain — no I/O, no Tauri, no agent names
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── entities/       # Session, Workspace, Worktree, DiffExplanation, ...
│   │       ├── ports/          # trait definitions: AgentRunner, GitPort, ...
│   │       ├── state/          # AgentStatus state machine, SessionRegistry
│   │       └── use_cases/      # application logic (spawn session, approve, merge)
│   │
│   └── adapters/               # everything that touches the outside world
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── pty/            # PtyAdapter — wraps portable-pty
│           ├── git/            # GitAdapter — git2 + CLI for worktrees/diffs
│           ├── watcher/        # FileWatcher — notify crate, debounced
│           ├── mcp/            # McpClient — DiffExplainerPort via VibeLens MCP
│           ├── notifier/       # OS notifications (macOS, later Linux)
│           └── agents/         # per-agent AgentRunner impls: claude, cursor, codex, aider
│
├── src-tauri/                  # Tauri shell — the Bridge between Rust and the web UI
│   ├── Cargo.toml              # depends on core + adapters
│   ├── tauri.conf.json
│   ├── build.rs
│   └── src/
│       ├── main.rs
│       ├── commands/           # #[tauri::command] handlers (spawn, send_input, approve, ...)
│       └── events/             # event emitters pushed to the UI (session_update, pty_data, ...)
│
├── ui/                         # React + Vite + xterm.js
│   ├── package.json
│   ├── vite.config.ts
│   ├── src/
│   │   ├── main.tsx
│   │   ├── components/         # atomic + composite UI components
│   │   │   ├── Terminal/       # xterm.js wrapper
│   │   │   ├── VibeLensPanel/  # DiffExplanation renderer
│   │   │   ├── SessionSidebar/ # session list + status indicators
│   │   │   └── Dashboard/      # cross-session overview
│   │   ├── hooks/              # React hooks for Tauri event subscriptions
│   │   ├── store/              # client-side state (no business logic)
│   │   └── types/              # TypeScript types mirroring Rust domain structs
│   └── tests/
│       ├── unit/               # Vitest component tests
│       └── e2e/                # Playwright E2E flows
│
└── docs/                       # all documentation (you are here)
    ├── glossary.md
    ├── architecture/
    ├── engineering/
    ├── design/
    ├── product/
    └── decisions/
```

---

## What belongs where

### `crates/core`

**Domain only.** Session lifecycle, AgentStatus transitions, DiffExplanation assembly,
worktree rules, CostMetrics accumulation. Port trait definitions.

**Hard rules — zero tolerance:**
- No `use portable_pty`, `use git2`, `use tauri`, or any agent string literal.
- No async runtime coupling (`tokio` internals); async is fine but keep it runtime-agnostic.
- No file system access; no network calls.
- No reference to any specific agent name (Claude Code, Aider, etc.).

If the Core can't be tested with nothing but fake structs and `std`, something is wrong.

### `crates/adapters`

Implements every Port the Core defines. Owns all the messy OS interaction: PTY I/O,
git subprocess calls, file watching, MCP client, OS notifications.

**Rules:**
- Depends on `crates/core` (for the port traits it implements). Never the reverse.
- Each adapter module stands alone; adapters must not import each other.
- Integration tests live here (see [Testing Strategy](testing-strategy.md)).

### `src-tauri`

The Bridge. Wires adapters into the Tauri event loop, exposes `#[tauri::command]`
functions to the UI, and emits events back. Contains almost no logic — it is plumbing.

**Rules:**
- Depends on `crates/core` + `crates/adapters`. No domain logic here.
- A command handler should do: validate input → call a use case → return/emit result.
- No business rules, no domain decisions.

### `ui/`

React + xterm.js. Presentation only.

**Rules:**
- Communicates with the backend **only** through Tauri's `invoke()` and `listen()`.
- No business logic. If you find a state machine in a React component, move it to Core.
- Types in `ui/src/types/` must mirror the Rust structs — keep them in sync manually
  until a code-gen step is set up.

---

## Where tests live

| Test kind | Location |
|---|---|
| Core unit tests | `crates/core/src/` (inline `#[cfg(test)]` modules) |
| Adapter integration tests | `crates/adapters/src/` (inline) + `crates/adapters/tests/` |
| UI component tests (Vitest) | `ui/tests/unit/` |
| E2E tests (Playwright) | `ui/tests/e2e/` |

> ❓ OPEN: Decide whether to add a top-level `tests/` integration suite that exercises
> the full stack (Tauri + Core + Adapters) using a headless webview or a mock bridge.
> Tracked for M3+.

---

## The dependency rule — visualized

```
ui/  →  (Tauri bridge)  →  src-tauri  →  crates/adapters  →  crates/core
                                       ↗
                           crates/adapters
```

`crates/core` has **no outbound arrows** except to `std` and zero-dependency utility
crates. Anything else is a violation of the hexagonal boundary.

---

## See also

- [Architecture Overview](../architecture/overview.md) — the three-layer model
- [Conventions](conventions.md) — naming and style rules
- [Testing Strategy](testing-strategy.md) — test pyramid details
