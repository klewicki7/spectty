# Architecture Decision Records

This directory records significant architectural decisions made for Spectty. Each ADR
captures what was decided, why, the tradeoffs consciously accepted, and what
alternatives were considered and rejected.

ADRs are **append-only by default**. When a decision is reversed, a new ADR
supersedes the old one; the old one is updated to `Status: Superseded` with a link
to its replacement.

## What counts as an ADR

- A decision that shapes multiple layers of the codebase
- A technology choice where meaningful alternatives existed
- A constraint that would be costly or risky to reverse later
- A pattern adopted as a project-wide convention

Day-to-day implementation choices that are cheap to reverse do not need an ADR.

## Numbering convention

Sequential, zero-padded to four digits: `0001`, `0002`, … Files are named
`NNNN-short-kebab-title.md`. Numbers are never reused, even if an ADR is superseded.

## Status legend

| Status | Meaning |
|---|---|
| **Proposed** | Under discussion; not yet ratified. |
| **Accepted** | Decision is in effect. |
| **Superseded** | Replaced by a later ADR (link provided). |

## Index

| ADR | Title | Status |
|---|---|---|
| [ADR-0001](0001-gui-over-tui.md) | GUI desktop app over TUI multiplexer | Accepted |
| [ADR-0002](0002-tauri-over-electron.md) | Tauri + Rust over Electron + Node | Accepted |
| [ADR-0003](0003-hexagonal-architecture.md) | Hexagonal (Ports & Adapters) architecture | Accepted |
| [ADR-0004](0004-agent-agnostic-core.md) | Agent-agnostic Core behind `AgentRunner` port | Accepted |
| [ADR-0005](0005-build-on-gentle-ai-stack.md) | Build on gentle-ai/engram stack behind `PersistencePort` | Accepted |
| [ADR-0006](0006-spectty-agent-protocol.md) | Spectty Agent Protocol: MCP tools + hook injection | Accepted |
| [ADR-0007](0007-living-spec-pane.md) | Living Spec pane as a steerable contract | Accepted |
| [ADR-0008](0008-agent-centric-cockpit.md) | Agent-centric cockpit; communications orbit as Panels | Accepted |

---

## Template

Copy this block to start a new ADR.

```markdown
# ADR-NNNN: <title>

- Status: Proposed
- Date: YYYY-MM-DD
- Deciders: project owner

## Context

<!-- What situation, constraint, or requirement prompted this decision? -->

## Decision

<!-- What was decided. One clear statement, then elaboration. -->

## Consequences

**Positive**
- …

**Negative**
- …

**Neutral**
- …

## Alternatives considered

### <Alternative name>
<!-- What it is and why it was not chosen. -->
```
