# Personas

> Who uses Spectty. Three personas, ordered by priority. See also [Vision](vision.md)
> and [Problem Statement](problem-statement.md).

> ✅ DECIDED: Primary audience is the solo AI-first developer (P1). The tech-lead/team persona is served post-MVP.

---

## P1 — The Solo AI-First Developer (primary)

> "I run three Claude Code sessions on the same repo and I spend half my time figuring
> out which one needs me."

### Context

Works alone or in a small team. Has adopted AI coding agents as the primary way to
produce code — they direct agents more than they type code themselves. Runs 3–5 parallel
agent sessions routinely: one on a feature, one refactoring tests, one on a bug in a
separate branch. Uses a stock terminal + tmux today, or a multiplexer like Zellij. Pays
for Claude Code out of pocket or on a team plan and watches the token bill.

### Goals

- Keep multiple agent sessions moving without losing the thread of each one.
- Know immediately when any agent is blocked, without watching every pane.
- Understand what each agent did without reading raw diffs.
- Keep parallel agents from stepping on each other's work.
- Maintain a lightweight cost picture so API bills do not surprise them.

### Frustrations today

- Agents go `AwaitingInput` silently in a tmux pane they are not looking at. Minutes lost.
- Reviewing an agent's work means scrolling through a full `git diff`. Slow and error-prone.
- Running two agents on the same repo without manual worktree setup leads to file conflicts.
- No aggregate cost view — they check the API dashboard reactively, not proactively.
- Navigating five tmux panes by number is disorienting; there is no status-aware overview.

### What Spectty gives them

- A **cockpit they live in**: open Spectty, direct agents, stay in one surface. Not a
  terminal plus five other windows.
- The **core triad** always visible: SPEC (the living contract showing what they asked and
  how the plan is progressing), DIFF (VibeLens — what the agent changed), WHY (the
  rationale). They never read a raw diff or scroll scrollback to understand what happened.
- A **plan-approval gate**: the agent proposes a structured task list before touching
  code; the dev approves, adjusts, or redirects. Steering mid-flight is first-class.
- A Dashboard where `AwaitingInput` and `Error` sessions pulse for attention — they never
  miss a blocked agent again.
- **Dead-time comms** (Slack, Gmail panels) available while the agent is running — no
  context-switching to another app; the agent's cockpit is the whole workspace.
- Automatic Worktree isolation per session — parallel agents on the same Workspace never
  collide.
- Per-session and aggregate CostMetrics visible in context, updated live.
- Named sessions with AgentStatus visible in navigation — jump to the right session by
  name and state.

---

## P2 — The Tech Lead Supervising Agent Work (secondary)

> "My team has five engineers running agents. I need to see what each one is doing
> without asking them in Slack."

### Context

Leads a team of 4–10 engineers, each running their own agent sessions. Is responsible for
code quality, budget, and ensuring agent-produced work does not ship without review.
Currently has zero visibility into what other people's agents are doing until a PR appears
in GitHub. By then the context is cold and the feedback loop is long.

### Goals

- See across all agent sessions on a project — not just their own.
- Catch runaway cost before the monthly bill arrives.
- Review agent-produced work with enough context to give fast, useful feedback.
- Enforce a review gate before agent work merges to main.

### Frustrations today

- Agent work is invisible until a PR lands. No live supervision surface for the team.
- Reviewing a PR produced by an agent is like reviewing any PR — raw diff, no explanation
  of intent. Slower and more error-prone than reviewing human-written code.
- No team-level cost visibility; individuals self-report or don't.
- No standard workflow for "agent produces work → tech lead reviews → merge" — each
  engineer handles it ad-hoc.

### What Spectty gives them

- A Dashboard that shows all sessions across the team's Workspaces, with AgentStatus and
  CostMetrics per session.

  > ✅ DECIDED: Multi-user / shared Dashboard is explicitly post-MVP. The MVP is single-user (solo-first).

- VibeLens explained diffs that make reviews faster: the tech lead sees *what* changed and
  *why* before diving into the diff.
- A review-then-merge flow where agent work lands in a Worktree, the lead reviews the
  DiffExplanation, and approves or rejects before the branch merges.
- Aggregate CostMetrics across sessions to track team-level spend.

---

## P3 — The Agent Power-User / Tinkerer (tertiary)

> "I built a custom shell script that wraps a local LLM into a coding agent. I want it to
> run in Spectty like Claude Code does."

### Context

Deep technical user — could be a solo developer, a researcher, or an internal tooling
engineer. Builds or customizes agents rather than just using packaged ones. May run a
local model (Ollama, LM Studio), a custom wrapper script, or an in-house agent that
encodes company-specific workflows. Wants the Spectty supervision UI without being limited
to first-party agents.

### Goals

- Register a custom or local agent with Spectty using a simple config, not a code fork.
- Get the same Dashboard, VibeLens panel, and worktree isolation for their custom agent
  as Claude Code gets.
- Extend Spectty's status detection for their agent's specific prompt patterns.
- Potentially contribute new agent adapters back to the project.

### Frustrations today

- Every agent-specific tool is hardcoded to one agent — usually Claude Code. Building a
  custom agent means building a custom tool from scratch.
- tmux gives them the PTY they need but zero of the supervision layer.
- Status detection ("is my agent blocked?") has to be hand-wired per agent if they want
  any notification at all.

### What Spectty gives them

- The `AgentRunner` Port means Spectty is **built for this**: a declarative `agent.toml`
  manifest (Phase 2 of the agent abstraction) lets them register a custom agent — command,
  prompt regexes, cost regex — without recompiling.
  See [Agent Abstraction](../architecture/agent-abstraction.md).
- The Generic adapter covers any CLI agent at a baseline level (run + idle-timeout
  detection) before a first-class adapter exists.
- A path from "config file" to "full trait impl" to "WASM plugin" as their needs grow
  more sophisticated.

> ❓ OPEN: The declarative `agent.toml` manifest is Phase 2 (post-MVP). P3 users get
> the Generic adapter in MVP; first-class custom agent support comes with M5+ or as a
> fast-follow. Decide whether to surface this in the MVP feature set at all.
