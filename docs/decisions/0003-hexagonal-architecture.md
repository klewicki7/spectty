# ADR-0003: Hexagonal (Ports & Adapters) architecture

- Status: Accepted
- Date: 2026-06-07
- Deciders: project owner

## Context

Spectty's product value is its domain: Sessions, agent supervision, VibeLens explained
diffs, Worktree isolation, AgentStatus detection. That domain is not the PTY library, not
the git backend, not the diff explainer service, not the notification system. All of those
are implementation details that **will change**:

- The terminal library may be swapped (`portable-pty` → another, or a custom fork).
- The diff explainer is VibeLens MCP today; a local model or a different MCP server is a
  plausible near-term swap.
- We will add agents without wanting to touch Session orchestration code.
- The domain must be testable without a running PTY, a real git repo, or a live agent
  process — otherwise iteration speed collapses and confidence in refactors is low.

Entangling the domain with any of these concrete dependencies would make all of the above
painful and risky. The domain must be isolated and pure.

## Decision

Adopt **Hexagonal Architecture (Ports & Adapters)** as the structural pattern for the
Rust backend.

**The three layers:**

```
UI (React + xterm.js)
       │  Tauri commands / events
       ▼
CORE  ─── pure Rust domain; no I/O; defines Ports (traits)
       ▲
       │  implements
ADAPTERS ─── touch the outside world; implement Ports
```

**Ports defined by the Core** (the Core depends on these traits, never on implementations):

| Port | Responsibility |
|---|---|
| `AgentRunner` | Launch an agent, detect its status, parse costs, provide quick actions |
| `GitPort` | Worktree create/delete, branch ops, diff generation |
| `FileWatchPort` | Watch a directory for changes, debounce, notify Core |
| `DiffExplainerPort` | Turn a git diff into a `DiffExplanation` (calls VibeLens MCP or equivalent) |
| `NotifierPort` | Send OS-level notifications when an agent is `AwaitingInput` |
| `ClockPort` | Current time (makes time-dependent domain logic testable) |

**The dependency rule** (non-negotiable): dependencies point inward. The Core never
imports `portable_pty`, `git2`, `tauri`, `notify`, or any agent name. If a `use tauri::`
or a literal `"claude"` appears inside the Core, that is a bug. See
[Architecture Overview](../architecture/overview.md).

**Adapters** (implement Ports, live outside the Core):
- `PtyAdapter` — wraps `portable-pty`; implements `AgentRunner`'s launch lifecycle
- `GitAdapter` — wraps `git2` or shell-out; implements `GitPort`
- `FileWatcher` — wraps the `notify` crate; implements `FileWatchPort`
- `McpClient` — calls VibeLens MCP over stdio/HTTP; implements `DiffExplainerPort`
- `Notifier` — OS notification via `tauri`'s notification plugin; implements `NotifierPort`
- Per-agent runners — implement `AgentRunner` per agent (Claude Code, Generic, etc.)

**Testing strategy:** every port has a fake (in-memory) adapter in a `test_support` crate.
Domain unit tests never touch the OS. Integration tests wire real adapters in a temp
directory. E2E tests run the full Tauri app.

## Consequences

**Positive**
- The domain (`Session`, `AgentStatus`, `DiffExplanation`, `Worktree`) is unit-testable
  with fakes — no PTY, no git repo, no network required.
- Swapping the diff explainer backend (VibeLens → local model) is one new `DiffExplainerPort`
  adapter; the Core does not change.
- Adding a new agent is one new `AgentRunner` implementation; the session orchestration
  code is untouched.
- Compile-time enforcement: module visibility + Rust's trait system make accidental
  coupling visible immediately.
- New contributors can understand and test the Core independently of the Tauri wiring.

**Negative**
- More upfront structure. "Just add a Tauri command that calls git2 directly" is faster
  for the first 10 features; discipline is needed to route through ports instead.
- More boilerplate: every cross-boundary operation needs a port method, a fake, and a
  real adapter. This is real overhead.
- Trait object dispatch (`dyn AgentRunner`) adds a small runtime cost relative to
  monomorphized code. Not measurable in practice for session-count workloads, but worth
  naming.
- Cognitive overhead for contributors unfamiliar with Hexagonal — the "why can't I just
  call git2 here?" question will come up.

**Neutral**
- The Tauri command layer sits outside the Core as an application-level orchestrator; it
  is not an "adapter" in the strict sense but is also not domain logic.
- The hexagonal boundary does not dictate module layout within the Core; internal
  structure (entities, state machines, use-case handlers) is a separate concern.

## Alternatives considered

### Layered architecture (no strict port discipline)

A traditional layered approach: `domain → service → infrastructure`, but without the rule
that domain may never import infrastructure types. In practice this becomes "mostly
layered" — domain starts importing `git2::Repository` for convenience, services grow
`tauri::AppHandle` parameters, and the domain becomes untestable without the full app.

**Why not chosen:** Spectty's domain is the differentiating product work. If it cannot be
tested in isolation, iteration speed and refactor confidence degrade exactly where the
most work happens. The "faster start" of skipping strict ports is paid back with interest
in 6–12 months when swapping the diff explainer or adding a third agent becomes a
multi-week refactor.

### "Just put it in the Tauri commands"

Implement everything directly in `#[tauri::command]` handlers — no Core abstraction, no
ports, business logic in the command layer.

**Why not chosen:** This is the path of least resistance that eventually produces an
untestable monolith. The Tauri command handlers are the UI boundary; mixing domain rules
there means you cannot test those rules without running the full Tauri app. Given that
the domain is the entire product (session lifecycle, status detection, worktree
orchestration), making it untestable is unacceptable.
