# Glossary

> Canonical vocabulary for the project. **Every other document and the code must
> use these terms exactly.** If a concept is not here, add it here first, then use it.

The product is named **Spectty** — a blend of **Spec** (the living contract at its core)
and **tty** (the terminal). When docs say "Spectty", "the app", or "the terminal", they
mean this product.

---

## Core domain terms

### Agent
An external CLI coding tool that Spectty drives — Claude Code, Cursor CLI, Codex CLI,
Aider, etc. Spectty is **agent-agnostic**: an Agent is defined by a small contract
(how to launch it, how to detect that it is waiting for the user, how to read its
token/cost usage), never hardcoded. See [Agent Abstraction](architecture/agent-abstraction.md).

### Session
One running unit of work: a single **Agent** process, attached to one **PTY**,
operating on one **Workspace** (optionally isolated in a **Worktree**), with its own
**VibeLens panel**, **AgentStatus**, and **CostMetrics**. A Session is the central
entity of the domain. It has a lifecycle (see **AgentStatus**).

### Workspace
The git repository (project root) a Session operates on. One Workspace can host many
Sessions at once (each typically in its own Worktree).

### Worktree
A git worktree that gives a Session an isolated branch and working copy, so multiple
Agents can run in parallel on the same Workspace without stepping on each other.
See [Session & Worktree Model](architecture/session-worktree-model.md).

### PTY
The pseudo-terminal that backs a Session. It is what makes the Agent believe it runs
in a real interactive terminal. Provided by the `portable-pty` Rust crate. This is an
**adapter**, never referenced by the domain core directly.

### AgentStatus
The lifecycle state of a Session, derived from observing the Agent's output:
- `Starting` — process spawned, not yet ready.
- `Idle` — ready, no active task.
- `Running` — actively working (producing output).
- `AwaitingInput` — **blocked waiting for the user** (a permission prompt, a
  question). This is the state Spectty must surface loudly.
- `Completed` — task finished cleanly.
- `Error` — process crashed or exited non-zero.

### DiffExplanation
The domain object behind the **VibeLens panel**: a structured, per-file summary of
what changed and *why*. Fields: file path, lines added/removed, change kind, and an
AI-generated rationale. Built from a git diff plus an analysis pass.
See [VibeLens Integration](architecture/vibelens-integration.md).

### VibeLens panel
The always-visible per-Session view that shows the live **DiffExplanation** — what
the Agent changed and the reason — instead of a raw diff. The signature feature of Spectty.

### CostMetrics
Per-Session tracking of token usage and estimated cost over the Session's lifetime.

### Checkpoint
A snapshot of a Workspace/Worktree state taken before an Agent makes changes, enabling
one-click rollback of a Session's work.

---

## UI terms

### Window
A top-level OS window of the app. Holds one or more **Panes**.

### Pane
A rectangular region inside a Window that renders exactly one **Session** (its terminal
+ VibeLens panel). Panes can be split and tiled.

### Tab
A switchable container of Sessions/Panes within a Window. Used for fast navigation
("navigate between windows and sessions easily").

### Dashboard
The cross-Session overview: every Session, its **AgentStatus**, Workspace, and
**CostMetrics** at a glance. The place you notice "Agent X is `AwaitingInput`".

---

## Architecture terms

### Core (domain)
Pure Rust logic with no I/O: entities (Session, Workspace, Worktree, DiffExplanation),
state machines (AgentStatus), and the **ports** they depend on. Fully unit-testable.

### Port
A trait the Core defines and depends on, implemented by an **Adapter** in the outer
layer (e.g. `AgentRunner`, `GitPort`, `Notifier`). Lets the Core stay ignorant of the OS.

### Adapter
The concrete implementation of a Port that touches the outside world: `PtyAdapter`,
`GitAdapter`, `FileWatcher`, `McpClient` (VibeLens), `Notifier`.

### Bridge (Tauri command / event)
The boundary between the Rust backend and the web UI. UI → backend via Tauri
**commands**; backend → UI via Tauri **events**. See [Data Flow](architecture/data-flow.md).

---

## Product & platform terms

### Cockpit
What Spectty *is*: the runtime surface a developer lives in to build software by
directing AI agents — terminals, the living Spec, explained diffs, and (later)
communications, in one place. Spectty is a cockpit, not a generic terminal, not an
agent, not an IDE. See [Vision](product/vision.md).

### Core triad
The product's soul: **Spec → Diff → Why**. *Spec* = what I asked (the living
contract). *Diff* = what the agent did (VibeLens). *Why* = the explanation. Spectty
makes all three visible and permanent at once. See [Spec Pane](product/spec-pane.md).

### Spec (living contract)
The dev↔agent contract behind the **Spec pane**: the developer seeds intent, the
agent turns it into a plan, and the pane tracks progress **live** (done / in-progress /
pending) and stays **steerable** mid-flight. A plan-approval gate precedes the agent
touching code. Its data model is adopted (generalized) from the SDD artifact pipeline.
See [Spec Pane](product/spec-pane.md).

### Panel
The generalized content a **Pane** can render: a terminal+agent, a VibeLens **Diff**,
the **Spec**, **Git**, or an orbiting **comms** surface (Slack, Gmail). Spectty is a
window manager of composable Panels. See [Layout & Panels](design/layout-and-panels.md).

### Orbiting panels / comms
Communications (Slack, Gmail) and other non-agent surfaces. They **orbit** the
agent-centric core — the "while-the-agent-works" layer — and are added after the core
is excellent. The agent is the center of gravity; comms are never co-equal in the MVP.

## Spectty Agent Protocol

### Spectty Agent Protocol
The suite of MCP tools + injected skills/rules that lets agents cooperate with Spectty
**structurally** instead of Spectty scraping their PTY output. Tools (provisional):
`spectty_spec` (plan + progress → Spec pane), `spectty_diff` (VibeLens, exists),
`spectty_approval` (structured AwaitingInput), `spectty_status`, `spectty_cost`. Injected in
three layers (MCP tools + hook `additionalContext` + `SKILL.md`), a pattern proven by
engram. See [Agent Protocol](architecture/agent-protocol.md).

### Cooperative tier / Generic tier
The two-tier supervision model. **Cooperative** agents (protocol injected) emit
structured signals via MCP — robust. **Generic** agents (no protocol) fall back to PTY
**scraping** heuristics — best-effort. Every `AgentRunner` declares which tier it supports.

### Provisioner (ProvisioningPort)
The component that injects the Spectty Agent Protocol into each agent in its **native
format** (Claude Code `.mcp.json`+`CLAUDE.md`/skills, Cursor `.cursor/rules`, etc.),
using managed-section markers, atomic writes, and backup-before-write. Injection scope
is **global** (user config) by default, **project** (repo) when the artifact belongs
versioned. Patterns copied from the gentle-ai CLI. See [Agent Protocol](architecture/agent-protocol.md).

### Injection scope (global / project)
**Global** = the agent's user-level config (`~/.claude/`, `~/.cursor/`): default,
keeps the repo clean. **Project** = the repo (`.mcp.json`, `.cursor/rules`): only when
the artifact should be versioned/team-shared.

## Persistence & stack integration

### PersistencePort
The Core port for durable state (Sessions, the living Spec, CostMetrics, Checkpoints).
Its default adapter wraps **engram**; the Core never imports engram directly. Swappable.
See [Stack Integration](architecture/stack-integration.md).

### engram (external dependency)
The MIT-licensed Go memory store (SQLite + FTS5, `topic_key` upsert, dual server: stdio
MCP + local HTTP `:7437`) that Spectty builds on for persistence, behind `PersistencePort`.
Spectty is **engram-native**. By Gentleman-Programming.

### gentle-ai stack
The MIT-licensed ecosystem (gentle-ai CLI, engram, SDD, skills) by Gentleman-Programming
that Spectty builds **on top of** — engram as a dependency, gentle-ai's injection **patterns**
copied, the SDD artifact **model** adopted. gentle-ai is an install-time *configurator*;
Spectty is the *runtime cockpit* it lacks — **complementary, not a competitor**. See
[Stack Integration](architecture/stack-integration.md) and [Research](research/gentle-ai-stack.md).
