# Problem Statement

> The pain Spectty removes. See also [Vision](vision.md), [Personas](personas.md),
> and [Competitive Analysis](competitive-analysis.md).

---

## Context

AI CLI agents (Claude Code, Cursor CLI, Codex CLI, Aider) went from novelty to daily
workflow in 2025. The next natural step — running several in parallel, each on a different
task or branch — is already happening. Developers do it today in raw tmux splits or
multiple terminal windows.

The tooling never caught up. Stock terminals and tmux were designed for a human typing
commands. When the human becomes a supervisor and the agent types the commands, every
workflow assumption breaks.

---

## The five concrete pains

### 1. You miss a blocked agent

An agent stops and waits — for a file permission, a clarification prompt, a destructive-
action confirmation — and you have no idea. You are looking at a different pane. The
agent sits frozen for minutes (or hours). When you finally notice, you have lost the
thread of what it was doing and why it stopped.

**This is the #1 failure mode of running multiple agents in parallel.** Nothing in tmux
or a stock terminal tells you an agent is `AwaitingInput` rather than just thinking.

### 2. You read raw diffs to understand what an agent did

An agent finishes a task. To decide whether to accept its work you open `git diff`, scroll
through hundreds of lines of unified diff, and try to reconstruct the intent. For every
file. For every session. Multiply that by five parallel agents and you spend more time
reading diffs than directing agents.

There is no automatic "what did this agent do and WHY" — you have to build that picture
from scratch every time.

### 3. Parallel agents collide on the same files

Two agents working on the same repository edit the same file. One overwrites the other's
work. The second agent either errors out or silently produces a corrupt result. You
discover this after the fact, during a merge or a test run.

git worktrees exist precisely to avoid this, but nothing in the agent workflow creates and
manages them automatically. Developers either forgo isolation (and deal with collisions)
or wire up worktrees by hand (and deal with the overhead).

### 4. No per-agent cost or token visibility

Each agent session burns tokens. When you run five in parallel you have no aggregate view
of what they are spending. You discover overruns at the end of the day — or at the end of
the month on an API bill. There is no live "this session has spent $0.34 and counting"
visible in the context of the session's work.

### 5. No fast session navigation

Switching between five agent sessions in tmux requires either remembering pane numbering,
navigating a tree of windows, or keeping a mental map of "session 3 is on auth, session 4
is on tests." There is no named, status-aware navigation surface. Finding the session that
finished five minutes ago is a hunt.

---

## Jobs-to-be-done

| When I… | I want to… | So I can… |
|---|---|---|
| run 3–5 agents in parallel | know **instantly** which one is blocked | unblock it in seconds, not minutes |
| an agent finishes a task | understand **what it changed and why** without reading a diff | decide accept/reject in one glance |
| multiple agents work on the same repo | have each one **isolated** from the others automatically | merge or discard each agent's work independently |
| a session is burning tokens | see **live cost** per session in context | stay inside my budget without surprises |
| I need to switch between sessions | navigate by **name and status** | jump to the right session without a mental map |

---

## Why existing tools don't solve it

General-purpose terminals (Warp, Ghostty) and multiplexers (tmux, Zellij) are optimized
for a human driving a shell. They have no concept of agent sessions, no status detection,
no worktree management, and no diff explanation layer — because they were not built for
this workflow.

Agent-specific tools (Conductor, Crystal, Claude Squad) address some supervision gaps but
are GUI-app-shaped and hardcoded to one agent. They do not provide a terminal-native
experience, they do not degrade gracefully to other agents, and they do not solve the
explained-diff problem.

The full picture is in [Competitive Analysis](competitive-analysis.md).
