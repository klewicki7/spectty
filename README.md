# Spectty

**The terminal for people who code with AI agents.**

Spectty is a desktop terminal built around one workflow: directing AI CLI agents —
Claude Code, Cursor CLI, Codex CLI, Aider — while supervising several of them in
parallel. Where general-purpose terminals treat the shell as the unit of work, Spectty
treats the **Agent Session** as the unit of work. Navigation, isolation, visibility,
and notifications are all organized around that.

## Four pillars

- **Agent-agnostic core.** Agents are plugins defined by a small contract. Claude Code
  today, your in-house agent tomorrow — same shell, same supervision UI.
- **VibeLens — explained diffs, always visible.** A dedicated panel below each terminal
  shows what the agent changed and *why*, per file, live. You never read a raw diff.
- **Worktree isolation by default.** Every session gets its own git worktree so parallel
  agents on the same repo never conflict.
- **Supervision over execution.** A Dashboard and pulsing indicators tell you instantly
  which agent is blocked waiting for you, which finished, and which errored — before you
  lose the thread.

## Status

Greenfield — documentation-first. The codebase does not exist yet; architecture and
product decisions are being captured in `docs/` before a line of code is written.
Milestone M0 will scaffold the monorepo.

## Everything else

→ **[docs/README.md](docs/README.md)** — the full documentation map: product vision,
architecture, engineering guides, design principles, and decision records.
