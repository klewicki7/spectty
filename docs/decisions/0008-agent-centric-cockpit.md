# ADR-0008: Agent-centric Cockpit, Communications as Orbiting Panels

- Status: Accepted
- Date: 2026-06-07
- Deciders: project owner

## Context

Spectty's stated goal is a **daily workspace** — something the developer opens in the
morning and works from all day. That implies more than an agent terminal: it implies
email, Slack, calendar, and task management. The owner wants those things.

The tension is real: "excellent everything" is the super-app death-trap. Slack does Slack
better than any terminal. Superhuman does email better than any terminal. Building
co-equal communication features alongside agent supervision means competing with
category-dominant tools on their home turf, before the core product is proven.

Meanwhile, the core product — AI agent supervision — has no excellent existing tool.
Claude Squad, Conductor, and Crystal are Claude-Code-specific. Spectty's differentiated
ground is agent-agnostic supervision with the core triad (Spec → Diff → Why).

## Decision

Spectty is a dev **cockpit** with the **AI agent at the center of gravity**.
Communications (Slack, Gmail, calendar) orbit as the "while-the-agent-works" layer,
added after the core is solid, and never co-equal in the MVP.

### Center of gravity: the agent loop

The defensible, differentiated core is:

1. **Agent loop**: launch, supervise, cost-track, approve — the `AgentRunner` abstraction
   ([ADR-0004](0004-agent-agnostic-core.md))
2. **Explained diff**: Diff pane with AI-generated rationale for every change before it
   lands (the "Why" of the triad)
3. **Living Spec pane**: steerable contract, plan approval gate, live progress
   ([ADR-0007](0007-living-spec-pane.md))

These three are the MVP. They are the reason a developer would choose Spectty over a
raw terminal + Claude Code.

### Orbiting layer: communications as Panels

Spectty is architected as a **window manager of composable Panels**. Each Panel is an
isolated view with its own adapter:

- `AgentPanel` — the primary panel; always present
- `DiffPanel` — explained diff view
- `SpecPanel` — living spec pane
- `SlackPanel` — (post-MVP) ambient Slack digest; not a Slack replacement
- `GmailPanel` — (post-MVP) ambient mail triage
- `CalendarPanel` — (post-MVP) day-at-a-glance

Communications Panels are explicitly **ambient and read-oriented in MVP form**: they
surface what needs attention while the agent works, not a full client experience.
They are added only after the agent-loop core is shipped and validated.

### Why "cockpit" is the right frame

A cockpit is not a super-app. It does not do everything excellently. It surfaces the
instruments the pilot needs, in one place, while the primary job is flying the plane.
The plane here is the AI agent doing the work. Everything else is instrumentation.

This framing also resolves the roadmap question: new Panels are always evaluated as
"does this help the developer while the agent works?" If no, it does not belong.

## Consequences

**Positive**
- Focused MVP: three features (agent loop + diff + spec) is achievable and differentiable.
- The Panel/Adapter architecture gives a coherent expansion model: adding Gmail in Month 6
  does not require touching the agent core.
- The "while-the-agent-works" filter is a concrete product heuristic for evaluating
  every future feature request.
- Deferred comms excellence avoids competing with Slack and Superhuman before the core
  is proven.

**Negative**
- The cockpit vision is only compelling if the agent-loop core is excellent. A mediocre
  agent supervision experience with nice Slack integration is not a cockpit — it is a
  distraction wrapper. The core must be held to a high bar.
- Deferring communications features means Spectty is not the daily workspace on day one
  for users who primarily want the communications layer. That is a conscious tradeoff.

**Neutral**
- The Panel abstraction is a UI-layer concern and does not affect the Core or the
  Hexagonal boundary ([ADR-0003](0003-hexagonal-architecture.md)). Each Panel adapter
  is an infrastructure adapter behind a port.
- "Ambient and read-oriented" for communications Panels is a scope definition, not a
  permanent ceiling. A later ADR can promote a Panel to a fuller experience if the
  core is proven and the demand is clear.

## Alternatives considered

### Super-app with co-equal communications

Build Slack, Gmail, and agent supervision as equal pillars from the start. Full
communication client experience alongside agent supervision.

**Why not chosen:** Scope explosion before the core is validated. Competing with
Slack and Superhuman on their terms is a losing position for a new product. The
super-app model requires excellence across many domains simultaneously — that is not
achievable with a solo or small team before proving the core value. This is the
canonical "build everything, ship nothing" trap.

### Single-purpose agent terminal (no cockpit vision)

Build only the agent supervision layer. No Panels, no communications, no window manager.
Pure agent terminal.

**Why not chosen:** This gives up the cockpit differentiation and limits the
addressable daily-workflow surface. The Panel model costs almost nothing to define
architecturally and keeps the cockpit vision alive without requiring immediate
implementation of every panel. A pure agent terminal is a feature, not a product.
