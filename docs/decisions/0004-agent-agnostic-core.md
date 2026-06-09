# ADR-0004: Agent-agnostic Core behind the `AgentRunner` port

- Status: Accepted
- Date: 2026-06-07
- Deciders: project owner

## Context

Spectty's stated requirement is to "specialize in code agents." Paradoxically, that requires
the Core to know **nothing** about any specific agent. Here is why:

- The market of AI CLI coding agents is young and fast-moving: Claude Code, Cursor CLI,
  Codex CLI, Aider are all plausible first-class targets, and new ones will appear.
- Competitors (Conductor, Crystal, Claude Squad) are **Claude-Code-specific**. That is
  the gap Spectty occupies: the agent-agnostic supervision layer. Hardcoding Claude Code
  would erase that differentiation.
- "Specializing" means excelling at supervision primitives (status detection, cost
  tracking, permission approval, diff explanation) for *any* agent, not duplicating the
  agent's own capabilities.
- The `AwaitingInput` detection — the most critical signal — is inherently agent-specific:
  Claude Code signals it differently from Aider, which signals it differently from a
  generic CLI. This is behavior, not config, and it must be encapsulated per agent without
  leaking into the Core.

## Decision

The Core defines a single `AgentRunner` port (Rust trait). Every agent-specific detail
lives behind it. The Core contains zero agent names, zero agent-specific regexes, and zero
`if agent == "..."` branches.

The `AgentRunner` contract (canonical definition in
[Agent Abstraction](../architecture/agent-abstraction.md)):

```rust
trait AgentRunner: Send + Sync {
    fn launch_spec(&self, ctx: &LaunchContext) -> LaunchSpec;
    fn detect_status(&self, signal: &OutputSignal) -> Option<AgentStatus>;
    fn parse_cost(&self, signal: &OutputSignal) -> Option<CostDelta>;
    fn quick_actions(&self, status: &AgentStatus) -> Vec<QuickAction>;
    fn descriptor(&self) -> AgentDescriptor;
}
```

`detect_status` is the critical method: given a normalized window of recent PTY output
(`OutputSignal`), each adapter returns the current `AgentStatus` or `None` (no change).
The `AwaitingInput` detection lives entirely here — the Core just acts on the returned
status.

**Planned adapters:**

| Agent | Adapter | Status signal | Cost source |
|---|---|---|---|
| Claude Code | `ClaudeCodeRunner` | Permission prompt patterns, "Do you want…" lines | Usage output lines |
| Generic CLI | `GenericRunner` | Idle-timeout heuristic only | None |
| Cursor CLI | `CursorRunner` (future) | Prompt markers (TBD) | TBD |
| Codex CLI | `CodexRunner` (future) | Approval prompt patterns | TBD |
| Aider | `AiderRunner` (future) | `>` prompt, confirmation lines | Token report |

The **Generic adapter** is the safety net: any CLI agent works at a baseline level (run +
idle timeout detection) before a first-class adapter exists.

**Extension path:**
- MVP: `ClaudeCodeRunner` + `GenericRunner`
- Phase 2: declarative `agent.toml` manifest (command, prompt regexes, cost regex)
  → auto-generates a runner without recompiling, covering ~80% of future agents
- Phase 3: WASM/plugin runners for agents needing custom logic

**Forbidden patterns (enforced by code review and architecture tests):**
- `if agent == "claude"` anywhere in the Core
- Agent-specific regex in the UI layer or session orchestration
- The PTY adapter knowing which agent it runs (it runs a `LaunchSpec`, nothing more)

## Consequences

**Positive**
- Adding a new agent type does not touch the Core or the session orchestration code —
  only a new `AgentRunner` implementation.
- The product's multi-agent positioning is structurally enforced, not just a policy.
- The Generic adapter means "new agent support" has a baseline on day one, with a
  progressive enhancement path to first-class support.
- `AgentDescriptor.capabilities` lets the UI degrade gracefully: if `reports_cost ==
  false`, the Dashboard shows "n/a" instead of wrong data.
- Agent-specific detection logic is co-located with the agent adapter and testable in
  isolation.

**Negative**
- Per-agent detection logic to implement and maintain: each new first-class adapter
  requires someone to study that agent's output patterns and write/test the detection.
- The Generic adapter's idle-timeout heuristic will produce false `AwaitingInput` signals
  for slow agents — acceptable for a generic fallback, not for a first-class experience.
- The `OutputSignal` normalization layer (stripping ANSI, windowing output) is shared
  infrastructure that must be robust; bugs there affect all agent adapters.

**Neutral**
- The `AgentRunner` trait is a Port in the Hexagonal sense ([ADR-0003](0003-hexagonal-architecture.md));
  this decision is the specific application of that pattern to agents.
- The declarative manifest path (Phase 2) does not change the trait contract — manifests
  compile down to a generic `ManifestRunner` that implements `AgentRunner`.

## Alternatives considered

### Hardcode Claude Code first, generalize later

Implement session management, status detection, and cost parsing with Claude Code
hard-wired. Add an abstraction layer when a second agent is needed.

**Why not chosen:** "Generalize later" is the correct call when the abstraction is
speculative. Here it is not — the abstraction IS the product. The entire differentiation
from Claude Squad, Conductor, and Crystal is that Spectty is not Claude-Code-specific. If we
ship a Claude-Code-hardcoded MVP and then try to extract the abstraction under live
product pressure, we will have a harder refactor and a harder story to tell. The
`AgentRunner` trait costs almost nothing to define upfront; not defining it costs the
product's positioning.

### Configuration-only approach (no trait, just TOML)

Define agents entirely through a config file (command, env, prompt regexes, cost regex)
with no code. No Rust trait; just a config-driven runner.

**Why not chosen:** `detect_status` is the hardest problem and it is often heuristic —
matching a known prompt string is easy, but detecting "`AwaitingInput` because the agent
went idle after outputting a question" requires stateful logic. A pure config file cannot
express stateful behavior. We retain the trait as the foundation and add a declarative
layer on top in Phase 2 for the 80% of cases that are regex-matchable.

## Amendment — Superseded for M2+ (provisioning is a sibling Core port, not a runner method)

- Date: 2026-06-08
- Driver: change `M2-spawn-agent-provisioner` (design ADR **D7 / risk R9**)

The original sketch of the `AgentRunner` contract (see
[Agent Abstraction](../architecture/agent-abstraction.md)) carried a provisioning
method on the runner trait — `fn provisioner(&self) -> Option<Box<dyn Provisioner>>`.
**M2 supersedes that mechanism.** Provisioning now lives behind a **separate Core port,
`ProvisioningPort`** (`crates/core/src/ports/provisioning.rs`), NOT on `AgentRunner`.

Rationale:

- **Provisioning is a session-lifecycle concern, not a per-output-tick concern.** Inject
  happens once on session create, retract once on session close — it does not belong next
  to `detect_status`/`parse_cost`, which run per `OutputSignal`. Keeping it off the runner
  trait keeps `AgentRunner` cohesive (launch + observe + describe).
- **Generic agents skip it cleanly without a trait method.** `AgentDescriptor` carries
  `requires_provisioning: bool`; the composition root decides whether to inject. A Generic
  agent simply has `requires_provisioning == false`, so no `Option`/`None` ceremony leaks
  into the runner contract.
- **The agent-agnostic intent of this ADR is preserved.** The Core still contains zero
  agent names and zero `if agent == "..."` branches. Only the MECHANISM moved: from a
  method on `AgentRunner` to a sibling port. The actual `AgentRunner` trait shipped in M2
  has five methods (`launch_spec`, `detect_status`, `parse_cost`, `quick_actions`,
  `descriptor`) and **no `provisioner()`**.

**The code is the source of truth** — `crates/core/src/ports/agent_runner.rs` and
`crates/core/src/ports/provisioning.rs`. The `provisioner()` shape shown in
`agent-abstraction.md` is historical; that doc carries the same amendment note.
