# Spectty — Documentation

> **Spectty** is the **cockpit a developer lives in to build
> software by directing AI agents**. AI CLI agents (Claude Code, Cursor, Codex, Aider)
> are the center of gravity; the living **Spec**, explained **diffs** (VibeLens), and —
> in orbit — communications are all in one surface. Spectty builds **on top of** the
> MIT-licensed gentle-ai / engram stack, behind its own ports.

Read the [Glossary](glossary.md) first — every doc uses its terms exactly.

## Map

### Product — *what we are building and why*
- [Vision](product/vision.md) — the cockpit bet, the core triad, build-on-stack.
- [Problem Statement](product/problem-statement.md) — the pain we remove.
- [Personas](product/personas.md) — who uses Spectty.
- [Features](product/features.md) — MVP scope + backlog, by priority.
- [Spec Pane](product/spec-pane.md) — the living dev↔agent contract (the soul). **★**
- [Competitive Analysis](product/competitive-analysis.md) — prior art and our edge.
- [Roadmap](product/roadmap.md) — milestones M0–M5+.

### Architecture — *how it is built*
- [Overview](architecture/overview.md) — hexagonal layering + the ports.
- [Stack Decisions](architecture/stack-decisions.md) — Tauri + Rust + xterm.js + engram.
- [Stack Integration](architecture/stack-integration.md) — building on gentle-ai/engram behind ports. **★**
- [Agent Protocol](architecture/agent-protocol.md) — injected MCPs + skills; structured cooperation. **★**
- [Domain Model](architecture/domain-model.md) — Session, Spec, Workspace, Worktree, Panel.
- [Agent Abstraction](architecture/agent-abstraction.md) — the agent-agnostic port + provisioning.
- [PTY Layer](architecture/pty-layer.md) — the terminal adapter.
- [VibeLens Integration](architecture/vibelens-integration.md) — explained diffs.
- [Session & Worktree Model](architecture/session-worktree-model.md) — isolation.
- [Data Flow](architecture/data-flow.md) — events, persistence, the event-stream gap.

### Decisions — *the record of why*
- [ADR index](decisions/README.md) — architecture decision records (0001–0008).

### Engineering — *how we work*
- [Getting Started](engineering/getting-started.md) — toolchain + first run.
- [Project Structure](engineering/project-structure.md) — folder layout.
- [Conventions](engineering/conventions.md) — naming, commits, style.
- [Testing Strategy](engineering/testing-strategy.md) — what we test and how.

### Design — *how it feels*
- [UX Principles](design/ux-principles.md) — the rules of the interface.
- [Layout & Panels](design/layout-and-panels.md) — windows, panes, the Panel system.
- [Keybindings](design/keybindings.md) — keyboard-first navigation.

### Research — *what we learned from prior art*
- [Gentle AI / engram stack](research/gentle-ai-stack.md) — investigation + what Spectty reuses. **★**

> **★** = added/changed in the consolidation of the product/architecture decisions made
> while designing Spectty as a cockpit on the gentle-ai stack.

## Status

Greenfield. Documentation-first: decisions are captured here before code is written.
The name (Spectty) and the six core MVP product decisions are locked. Remaining `> ❓ OPEN:` flags are intentional implementation deferrals (resolved at M0/coding) and minor external verifications.
