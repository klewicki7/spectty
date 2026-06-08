# Competitive Analysis

> Prior art, honest gaps, and Spectty's defensible position. See
> [Problem Statement](problem-statement.md) and [Vision](vision.md).

---

## The two categories

**Agent-orchestration tools** — purpose-built for running AI coding agents. Closer to
Spectty's mission, but with meaningful gaps.

**Terminals and multiplexers** — where developers actually run agents today. Strong PTY
fundamentals, zero agent-awareness.

---

## Comparison table

| Tool | What it does | Gap for AI-agent supervision | How Spectty differs |
|---|---|---|---|
| **Conductor** | GUI app for orchestrating Claude Code tasks across a project; shows task status and output | Claude Code–only; no explained diffs; not a real terminal (you don't drive the agent from inside it) | Terminal-native PTY; agent-agnostic; VibeLens baked in |
| **Crystal** | macOS app for Claude Code with a chat-style interface and diff review | Claude Code–only; positions as a chat UI, not a supervision terminal; diff review is manual | Multi-agent parallel supervision; agent-agnostic; live explained diffs, not manual review |
| **Claude Squad** | CLI tool that manages multiple Claude Code sessions using tmux under the hood | tmux-based (no GUI supervision surface); Claude Code–only; no explained diffs; no cost tracking | Purpose-built GUI desktop app; agent-agnostic; VibeLens panel; Dashboard; CostMetrics |
| **Vibe Kanban** | Kanban board that maps AI agent tasks to cards, tracking status per task | Board metaphor, not a terminal; no live PTY; not suited for supervising running sessions | Terminal-native; live PTY + VibeLens in one surface; real supervision, not task tracking |
| **Warp** | Modern terminal with AI completions, command search, and block-based output | No concept of agent sessions; AI features are shell-completion helpers, not agent supervision | Spectty is not a better shell — it is a session supervisor; orthogonal tools |
| **tmux** | Terminal multiplexer: split panes, named windows, session persistence | No agent-awareness; no status detection; no diff layer; no cost tracking; manual everything | Replaces the multiplexer with a purpose-built supervision layer on top of the same PTY primitives |
| **Ghostty** | Fast, native terminal emulator focused on performance and correctness | Same gap as any general terminal: no agent session model, no supervision | Spectty is not a faster terminal; Ghostty is the better choice if you just want a great shell |
| **Zellij** | Terminal multiplexer with a plugin system and layout manager | Plugin system is flexible but still no agent-session semantics out of the box; needs custom plugins for any supervision feature | Spectty ships agent-session semantics, VibeLens, and worktree isolation as first-class, not plugins |

> ❓ OPEN: Conductor's current feature set (as of mid-2025) may have evolved — verify
> before publishing externally. The description above is based on public information
> available through the knowledge cutoff.

> ❓ OPEN: Crystal's positioning is shifting rapidly. Verify whether it now supports
> non-Claude agents or has added multi-session supervision before using this table in
> marketing material.

> ❓ OPEN: Vibe Kanban may have pivoted or changed scope — flag for verification before
> any public competitive claims.

---

## What the table shows

**Agent-orchestration tools** (Conductor, Crystal, Claude Squad) prove the market demand
but share two structural constraints:

1. **Claude Code lock-in.** Each is purpose-built for one agent. When Claude Code
   releases a breaking change, or when a user wants to use Aider or a custom agent,
   there is no path.
2. **Not a terminal.** None of them give you a real PTY where you can interact with the
   agent in the terminal's native mode. They wrap or observe agents rather than *being*
   the surface you work in.

**Terminals and multiplexers** (Warp, tmux, Ghostty, Zellij) have strong PTY
fundamentals but zero agent awareness. The developer must build the supervision layer
themselves — or, more often, simply live without it.

Spectty's position is the gap between these two categories: **terminal-native + agent-agnostic
+ explained diffs built in**.

---

## The gentle-ai / engram ecosystem — complementary, not a competitor

> See [`../research/gentle-ai-stack.md`](../research/gentle-ai-stack.md) for the full
> stack analysis (file to be created).

**gentle-ai** and **engram** fill a different moment in the developer workflow:

| | gentle-ai / engram | Spectty |
|---|---|---|
| **When** | Install-time / session-start configuration | Runtime cockpit — you live here |
| **Role** | Configures agents with rules, skills, memory | Supervises agents with PTY + structured protocol |
| **Persistent memory** | Engram provides the memory store | Spectty is engram-native: reads/writes via `PersistencePort` |
| **Agent injection** | gentle-ai CLI writes agent configs (CLAUDE.md, etc.) | Spectty's Provisioner writes per-session MCP tools + skills in the same native format |
| **Relationship** | Spectty **builds on top of** this stack | — |

The key insight: gentle-ai teaches agents *how to behave*; Spectty gives the developer a
cockpit to *observe, steer, and review* what those agents do. They are designed to coexist —
Spectty's Provisioner copies gentle-ai's injection patterns rather than replacing them.

Spectty is **engram-native**: engram is a runtime dependency behind `PersistencePort`,
zero obligation on Spectty's source (MIT license, external dep). The real-time event layer
Spectty adds — polling/subscribe over engram — is the #1 technical capability gentle-ai's
stack does not provide (engram is a store, not pub/sub).

---

## Spectty's defensible position

Spectty owns the intersection nobody else is building:

1. **Terminal-native.** You interact with the agent in a real PTY — the same interaction
   model the agent was designed for. No proxy layer, no JSON API wrapper. This means any
   agent that runs in a terminal runs in Spectty.

2. **Agent-agnostic by design.** The `AgentRunner` Port means no hardcoded agent. Claude
   Code today, Cursor CLI and Codex tomorrow, your in-house agent next year — same shell,
   same supervision UI, no code fork. The agent-specific tools cannot replicate this
   without a ground-up rewrite of their assumptions.

3. **VibeLens baked in, not bolted on.** Explained diffs are a first-class domain object
   (`DiffExplanation`), not a post-hoc plugin. Every session gets a VibeLens panel. The
   domain model is designed around "you never read a raw diff again." This is a
   structural advantage; competitors would have to redesign their core data model to
   match it.

4. **Living Spec pane — the cockpit contract.** The structured SPEC → DIFF → WHY triad,
   with a plan-approval gate and mid-flight steering, is not present in any competing
   tool. It is Spectty's unique position: a runtime contract between developer and agent,
   visible and permanent at once.

The combination — PTY terminal + agent-agnostic port + live explained diffs + living Spec
pane + engram-native persistence — is not something any existing tool replicates. Each
competitor owns one or two of these; Spectty is the only design that requires all of them
simultaneously.
