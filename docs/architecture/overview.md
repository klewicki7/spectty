# Architecture Overview

Spectty follows **Hexagonal Architecture** (Ports & Adapters). The reason is concrete, not
academic: the product's value is the *domain* (sessions, agents, explained diffs,
worktree orchestration), and that domain must not be entangled with which terminal
library, git binary, or agent CLI we happen to use. We will swap all of those.

See [ADR-0003](../decisions/0003-hexagonal-architecture.md) for the decision rationale.

## The three layers

```
┌─────────────────────────────────────────────────────────────────────┐
│  UI layer  (web: React + xterm.js)                                  │
│  Composable PANEL system — layout is user-configurable:             │
│    • Terminal+Agent panel  (PTY output + agent controls)            │
│    • Diff panel            (VibeLens / DiffExplanation)             │
│    • Spec panel            (living Spec — intent, plan, progress)   │
│    • Git panel             (branches, worktrees, checkpoints)       │
│    • Comms panel           (notifications, cost, status) — post-MVP │
│  Dumb: holds no business rules, only presentation + input.          │
│  Talks to the backend ONLY through the Tauri bridge.                │
└──────────────────────────────────▲──────────────────────────────────┘
                    Tauri commands  │  Tauri events
┌──────────────────────────────────▼──────────────────────────────────┐
│  CORE  (Rust domain — pure, no I/O)                                 │
│  Entities:  Session (+ Spec), Workspace, Worktree, DiffExplanation  │
│             Panel (layout intent), CostMetrics                      │
│  State:     AgentStatus machine, Spec task progress                 │
│  Ports (traits the core depends on):                                │
│    AgentRunner · GitPort · FileWatchPort · DiffExplainerPort        │
│    NotifierPort · ClockPort · PersistencePort · ProvisioningPort    │
│  → fully unit-testable with fake adapters, zero OS access           │
└──────────────────────────────────▲──────────────────────────────────┘
                     implements    │
┌──────────────────────────────────▼──────────────────────────────────┐
│  ADAPTERS  (Rust — the outside world)                               │
│  PtyAdapter (portable-pty) · GitAdapter (git2/CLI)                  │
│  FileWatcher (notify) · McpClient → VibeLens (DiffExplainerPort)   │
│  EngramAdapter → engram daemon (PersistencePort)                    │
│  ProvisionerAdapter (Spectty Agent Protocol injector)               │
│  Notifier (OS notifications) · per-agent runners                    │
└─────────────────────────────────────────────────────────────────────┘
```

## The dependency rule (non-negotiable)

**Dependencies point inward.** The Core defines traits (Ports); Adapters implement them.
The Core never imports `portable_pty`, `git2`, `tauri`, `engram`, or any agent's name. If
you find a `use tauri::`, a `use engram::`, or a string `"claude"` inside the Core, that
is a bug.

Why this matters for *this* product specifically:
- We will support multiple **agents** → agent specifics live behind `AgentRunner`.
- We will likely swap the **diff explainer** (VibeLens MCP today, maybe a local model
  later) → behind `DiffExplainerPort`.
- We **build on the engram persistence stack** but Core must never couple to it directly
  → all persistence flows through `PersistencePort`; the `EngramAdapter` is the sole
  concrete implementation in MVP.
- We want the domain **testable without a GUI, a repo, or a real agent** → fakes for
  every port.

## Two-tier agent cooperation

Agents cooperate with Spectty at one of two tiers (see [Agent Protocol](agent-protocol.md)
for the full specification):

| Tier | How the agent signals Spectty | Who uses it |
|---|---|---|
| **Cooperative** | MCP tools injected at launch: `spectty_spec`, `spectty_diff`, `spectty_approval`, `spectty_status`, `spectty_cost` — structured signals, no PTY scraping | Claude Code, future first-class agents |
| **Generic** | PTY scraping + idle-timeout heuristics — no injection required | Any CLI agent, legacy tooling |

Each `AgentRunner` adapter declares its tier via `fn tier(&self) -> AgentTier`. The Core
handles both but surfaces richer Spec progress and cost data for Cooperative agents.

## Where each requirement lives

| User requirement | Layer / component |
|---|---|
| Dev cockpit: composable panel workspace | UI (Panel layout) + Core `Panel` / `Session` registry |
| Navigate windows & sessions | UI (Window/Pane/Tab) + Core `Session` registry |
| Living Spec — intent → plan → live progress | Core `Spec` entity + `PersistencePort` + Spec UI panel |
| Plan-approval gate before code executes | Core `Spec` approval-gate flag + `AgentRunner` (Cooperative) |
| Changes visible per window (VibeLens / Diff) | Core `DiffExplanation` + `DiffExplainerPort` + Diff panel |
| Excellent with code agents | Core `AgentRunner` port + per-agent adapters + `ProvisioningPort` |
| Spectty Agent Protocol injection | `ProvisioningPort` + `ProvisionerAdapter` (per-agent, per-scope) |
| Parallel isolated agents | Core `Worktree` + `GitPort` |
| Notice a blocked agent | Core `AgentStatus` machine + `NotifierPort` + Comms panel |
| Persist Sessions, Specs, cost, checkpoints | `PersistencePort` → `EngramAdapter` |

## Runtime shape

- **One Rust backend process** owns all Sessions, PTYs, watchers, and state. It is the
  single source of truth.
- **One web UI** (the Tauri webview) renders state and forwards user intent.
- Communication is asynchronous and event-driven: the backend pushes Session/PTY/diff/Spec
  events; the UI pushes commands (spawn session, send input, approve, merge).
  Detailed in [Data Flow](data-flow.md).

## Real-time event stream gap

engram is a **store, not a pub/sub bus**. It has no native subscribe/notify mechanism.
Spectty bridges this gap with a **polling + subscribe layer** in the `EngramAdapter` that
periodically queries engram (HTTP `:7437`) for Spec and progress updates and fans them out
as Core domain events. This is the primary technical risk in the persistence stack and
must be resolved before the Spec panel is wired. See [Data Flow](data-flow.md) for the
event pipeline design.

## Concurrency model

Each Session owns an async task tree on the Tokio runtime: a PTY read loop, a status
detector, and (debounced) a file-watch → diff-explain pipeline. Sessions are isolated;
a crash in one must never take down another. The Core coordinates them through an
in-memory `SessionRegistry`; adapters do the blocking/IO work off the domain thread.
