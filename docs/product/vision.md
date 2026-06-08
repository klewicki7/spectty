# Vision

> **Spectty is the cockpit a developer lives in to build software by directing AI agents.**

## The one-line bet

General-purpose terminals (iTerm, Warp, Ghostty) and multiplexers (tmux) were designed
for a human typing commands. A growing slice of engineering is now **directing AI
agents** that type the commands for you — and that work has different primitives:
sessions you supervise rather than shells you drive, a *spec* you steer rather than a
prompt you fire and forget, diffs you review rather than files you edit, and dead time
(while an agent works) you want to spend *without leaving the surface*.

**Spectty treats the AI coding session as the unit of work, and the cockpit as the place
you live.** You open Spectty in the morning and you can work: agents running, the task
you gave them always in view, what they changed explained in plain language, and — over
time — your communications right there too. No app-switching, no lost context.

## The soul: the core triad

Everything orbits one loop. Today, with any agent, you lose two-thirds of it: you fire a
prompt (the spec evaporates in the scroll), you get code back (you read a raw diff), and
the *why* never exists. Spectty makes all three **visible and permanent at once**:

```
   ┌──────────┐      ┌──────────┐      ┌──────────┐
   │   SPEC   │ ───▶ │   DIFF   │ ───▶ │   WHY    │
   │ what I   │      │ what it  │      │  the     │
   │ asked    │      │ did      │      │ reason   │
   └──────────┘      └──────────┘      └──────────┘
   living contract    VibeLens         explanation
```

This is what nobody has, and it is the center of gravity. See [Spec Pane](spec-pane.md).

## Center of gravity: the agent. Everything else orbits.

A deliberate, load-bearing decision (see [ADR-0008](../decisions/0008-agent-centric-cockpit.md)):

- **The AI agent is the center.** The agent loop + explained diff + living spec is Spectty's
  defensible edge — nobody has it.
- **Communications (Slack, Gmail) orbit.** They are the "while-the-agent-works" layer:
  when an agent runs, you triage Slack/email — co-locating that turns dead time into
  useful time *without breaking focus*. But Gmail and Slack are commodity, solved by huge
  teams; Spectty does **not** compete head-on with Superhuman or Slack. Comms are orbiting
  Panels, added after the core is excellent, never co-equal in the MVP.

Spectty is a **window manager of composable Panels** — terminal+agent, Diff, Spec, Git, and
later comms. The same UI primitive renders them all. See [Layout & Panels](../design/layout-and-panels.md).

## How agents cooperate: the Spectty Agent Protocol

Spectty does not *spy* on agents by scraping terminal output (fragile, per-agent regex).
It **gives them tools and instructs them**: Spectty injects a suite of MCP tools + skills
so the agent reports its plan, progress, approval requests, status, and cost in
**structured form**. This is what makes the living Spec pane actually work — and it is a
generalization of the VibeLens pattern already running in this repo. See
[Agent Protocol](../architecture/agent-protocol.md).

## We build on a stack that already exists — we don't reinvent it

Spectty stands on the MIT-licensed [gentle-ai / engram stack](../research/gentle-ai-stack.md)
(by Gentleman-Programming) — but behind Spectty's own ports, so the dependency is
quarantined and swappable:

- **engram** for persistence (behind `PersistencePort`).
- **gentle-ai's injection patterns** copied for the Provisioner (Spectty's own, coexisting).
- **the SDD artifact model** adopted and generalized for the living Spec.

Crucially, that stack is an install-time **configurator** with **no runtime, no cockpit,
no live spec pane, no cost/activity surface**. Spectty is exactly the runtime they lack:
**complementary, not a competitor.** See [Stack Integration](../architecture/stack-integration.md).

## What Spectty is NOT

- **Not a better generic terminal.** It optimizes for directing agents, not typing commands.
- **Not an agent.** It is the cockpit you sit in while agents write code. It does not write code.
- **Not an IDE.** No editor, no debugger. Review-steer-merge, not edit-in-place.
- **Not a super-app.** Comms orbit the agent; we will not build a mediocre everything.

## North-star experience

> You open Spectty. Three sessions are running. One finished and waits for review — its Spec
> pane shows every task ticked, its VibeLens panel the 4 files it touched and why. One is
> mid-task, its plan visibly advancing. One hit a permission prompt (it called
> `spectty_approval`) and is pulsing for your attention. While they work, your Slack panel is
> right there. You approve the blocked one with a keystroke, skim the finished one's
> explained diff against the spec you gave it, accept it, and it merges its worktree. You
> never read a raw diff, never lost an agent, never left the surface.

## Success looks like

- A user comfortably directs **3–5 agents in parallel** without losing the thread.
- "What did I ask, what did it do, and why?" is answered **at a glance** — the triad.
- Adding a **new agent** is a provisioning task, not a code fork.
- The agent's plan and progress are **structured and live**, never scraped.

> ✅ DECIDED (solo-first): The primary audience is the solo AI-first developer running 3–5 agents in parallel. Team / shared-supervision features (shared Dashboard, sync backend) are a post-MVP horizon, not in the MVP.
