# ADR-0006: Spectty Agent Protocol (MCP Tools + Hook Injection)

- Status: Accepted
- Date: 2026-06-07
- Deciders: project owner

## Context

Spectty needs to know, in near-real time, what an agent is doing: what spec is active,
what progress has been made, what the current cost is, and whether approval is needed.
The naive approach is to scrape PTY output and infer state from text patterns.

PTY scraping works at a basic level but has fundamental limits:

- Every agent formats its output differently. Regexes that work for Claude Code break for
  Aider, and vice versa. Maintenance cost grows with each new agent.
- The Living Spec pane ([ADR-0007](0007-living-spec-pane.md)) requires structured
  progress updates (which task is done, which is in-progress). There is no reliable way
  to extract that structure from narrative PTY output without the agent's cooperation.
- Cost data, spec references, and approval requests are all richer in structured form
  than in scraped text.

At the same time, Spectty cannot require every agent to cooperate — users will run
arbitrary CLI tools that know nothing about Spectty.

## Decision

Spectty defines a **structured agent protocol** and injects it into cooperative agents,
while guaranteeing a scraping-based fallback for non-cooperative agents.

### Protocol suite

Five MCP tools registered under the `spectty_` namespace:

| Tool | Purpose |
|---|---|
| `spectty_spec` | Agent reports the active spec (title, tasks, intent) |
| `spectty_diff` | Agent reports a structured diff summary before applying |
| `spectty_approval` | Agent requests human approval for a risky operation |
| `spectty_status` | Agent pushes a status update (Working / AwaitingInput / Done) |
| `spectty_cost` | Agent reports a cost delta (tokens / USD) |

Tools are **idempotent and optional per call** — an agent that only calls `spectty_cost`
still benefits from cost tracking without adopting the full protocol.

### Three-layer injection

Spectty's `AgentProvisioner` injects the protocol at three layers simultaneously:

1. **MCP tool registration**: adds the `spectty_*` tools to the agent's MCP server list
   in native config format (`.claude/settings.json`, `.aider.conf`, etc.)
2. **`additionalContext` hook**: injects a brief protocol reminder into the agent's
   system prompt on every session start — "you have spectty tools; use them to report
   progress"
3. **SKILL.md injection**: a `spectty-protocol.md` skill injected at project scope
   (and global scope for power users) explains the full contract and when to call each
   tool

### Two-tier model

| Tier | Condition | What Spectty gets |
|---|---|---|
| **Cooperative** | Agent calls `spectty_*` tools | Full structured supervision: living spec, real-time cost, typed approval |
| **Generic** | Agent ignores the tools | PTY scraping fallback via `AgentRunner.detect_status` (see [ADR-0004](0004-agent-agnostic-core.md)) |

The Generic tier is not a degraded experience — it is the same baseline every terminal
multiplexer provides. Cooperative agents unlock the features that differentiate Spectty.

### Provisioning scope

Injection is provisioned **per-agent in native format** at two scopes:

- **Global scope**: once, for all projects — the user's global agent config
- **Project scope**: per repo — overrides and project-specific additions

The `AgentProvisioner` UI shows which agents are provisioned and at which scope,
and handles format differences transparently (JSON for Claude Code, YAML for Aider, etc.).

## Consequences

**Positive**
- Cooperative agents provide structured, reliable supervision without fragile text
  parsing. The Living Spec pane becomes tractable.
- The protocol is additive: adopting one tool gives partial benefit without requiring
  full adoption.
- Per-agent native format provisioning means the user does not need to manually edit
  agent configs.
- The Generic fallback guarantees any CLI agent works on day one.

**Negative**
- Per-agent provisioning logic to maintain: each new first-class agent requires
  understanding its config format and injection points.
- Agents that ignore MCP tools (or run in environments where MCP is blocked) fall back
  to the Generic tier with no way to upgrade without agent-side changes.
- The `spectty_` MCP server must stay running for the protocol to work — it is a new
  process to manage in Spectty's session lifecycle.

**Neutral**
- The `AgentRunner` port ([ADR-0004](0004-agent-agnostic-core.md)) handles Generic-tier
  scraping. The protocol layer sits above it: if structured data arrives via MCP, it
  takes precedence; if not, the runner's `detect_status` fires.
- The injection pattern mirrors gentle-ai's provisioning conventions ([ADR-0005](0005-build-on-gentle-ai-stack.md)) but lives entirely in Spectty's own codebase.

## Alternatives considered

### Pure PTY scraping

Detect all state from terminal output only. No MCP tools, no injection.

**Why not chosen:** Works at baseline but cannot support the Living Spec pane's
structured progress model. Every new agent requires new regexes. The maintenance
surface grows unbounded, and the quality ceiling is low — scraping will always
produce false positives and miss structured data that the agent has internally
but never prints.

### Require all agents to cooperate (no Generic fallback)

Only support agents that implement the full `spectty_*` protocol. Reject others.

**Why not chosen:** This would exclude every agent that ships before Spectty gains
traction, which is every agent today. A cockpit that works with only one or two agents
is not a cockpit. The Generic tier keeps the product useful from day one and creates
the right incentive: agents gain richer Spectty integration by adopting the protocol,
but users are never blocked.
