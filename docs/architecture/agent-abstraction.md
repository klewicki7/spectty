# Agent Abstraction

This is the single most important architectural decision for "specializing in code
agents" (the user's requirement #3). Spectty must work *excellently* with terminal code
agents — and that means treating them as **plugins behind a contract**, never as
hardcoded `claude` calls. See [ADR-0004](../decisions/0004-agent-agnostic-core.md).

## The contract

The Core depends on a single port. Everything agent-specific implements it.

```rust
trait AgentRunner: Send + Sync {
    /// How to launch this agent in a PTY: program, args, env, cwd.
    fn launch_spec(&self, ctx: &LaunchContext) -> LaunchSpec;

    /// Inspect a window of recent PTY output (+ process state) and decide the
    /// current AgentStatus. This is how we detect "AwaitingInput" per agent.
    fn detect_status(&self, signal: &OutputSignal) -> Option<AgentStatus>;

    /// Best-effort token/cost extraction from agent output or sidecar logs.
    fn parse_cost(&self, signal: &OutputSignal) -> Option<CostDelta>;

    /// Optional: how to answer a known prompt (e.g. send "y\n" to approve).
    fn quick_actions(&self, status: &AgentStatus) -> Vec<QuickAction>;

    /// Identity / metadata for the UI.
    fn descriptor(&self) -> AgentDescriptor; // name, icon, capabilities

    /// Which cooperation tier this agent operates at.
    fn tier(&self) -> AgentTier;

    /// Provisioning: how to inject the Spectty Agent Protocol for this agent.
    /// Returns None for Generic-tier agents that need no injection.
    fn provisioner(&self) -> Option<Box<dyn Provisioner>>;
}
```

> **Superseded for M2+ (2026-06-08, change `M2-spawn-agent-provisioner`, ADR D7/R9).**
> The `provisioner()` method shown above did NOT ship on the M2 `AgentRunner` trait.
> Provisioning moved to a **separate Core port**, `ProvisioningPort`
> (`crates/core/src/ports/provisioning.rs`), because injection/retraction is a
> session-lifecycle concern, not a per-output-tick concern. Generic agents skip it via
> `AgentDescriptor.requires_provisioning == false` — no `Option`/trait-method ceremony on
> the runner. The shipped trait has five methods (`launch_spec`, `detect_status`,
> `parse_cost`, `quick_actions`, `descriptor`) and **no `provisioner()`**. The CODE is the
> source of truth (`crates/core/src/ports/agent_runner.rs`). See
> [ADR-0004 → Amendment](../decisions/0004-agent-agnostic-core.md#amendment--superseded-for-m2-provisioning-is-a-sibling-core-port-not-a-runner-method).

`OutputSignal` is a normalized, decoded view of recent PTY output plus process state —
adapters never parse raw ANSI; the PTY adapter pre-decodes it.

## Why a trait, not a config file (yet)

Detecting "this agent is waiting for me" is genuinely agent-specific and sometimes
heuristic (matching a known prompt string, watching for a cursor at a `?` line, noticing
output went idle after a question). That logic is *behavior*, so it lives in code behind
the trait. A declarative config layer can come later (see below), but the contract is the
foundation.

## Built-in agent adapters (planned)

| Agent | Tier | Launch | Status detection signal | Cost source |
|---|---|---|---|---|
| **Claude Code** | Cooperative | `claude` | permission prompts, "Do you want…" lines, idle-after-question; `spectty_status` MCP signals take precedence | parses its usage output + `spectty_cost` |
| **Cursor CLI** | Cooperative (future) | `cursor-agent` | its prompt markers | tbd |
| **Codex CLI** | Generic | `codex` | approval prompts | tbd |
| **Aider** | Generic | `aider` | `>` prompt, confirmation prompts | its token report |
| **Generic** | Generic | user-supplied cmd | idle-timeout heuristic only | none |

The **Generic** adapter is the safety net: any CLI agent works at a basic level (run +
idle detection) even before someone writes a first-class adapter.

## Capabilities, not assumptions

`AgentDescriptor` advertises capabilities so the UI degrades gracefully:

```rust
struct AgentCapabilities {
    reports_cost: bool,
    structured_permissions: bool,  // can we offer one-key approve?
    supports_resume: bool,
    emits_diff_signals: bool,      // does it tell us when it edited files?
    tier: AgentTier,               // Cooperative | Generic
}

enum AgentTier {
    /// Agent has Spectty MCP tools injected and emits structured signals.
    Cooperative,
    /// Agent is driven purely by PTY scraping + heuristics.
    Generic,
}
```

If `reports_cost == false`, the Dashboard shows "n/a" instead of guessing. If
`emits_diff_signals == false`, VibeLens falls back to the FileWatcher to know when to
re-explain. Cooperative agents surface richer Spec progress and reduce PTY-scraping
false-positives.

## Provisioning & the Spectty Agent Protocol

Cooperative agents require the Spectty MCP tool suite to be injected before launch. This
is the responsibility of the `ProvisioningPort` (Core-owned trait) and its adapter, the
**ProvisionerAdapter**. Full specification in [agent-protocol.md](agent-protocol.md).

### What gets injected

Each Cooperative agent receives five MCP tools at launch:

| Tool | Purpose |
|---|---|
| `spectty_spec` | Read/update the living Spec (intent, tasks, approval gate) |
| `spectty_diff` | Trigger a VibeLens diff explanation (= Spectty's DiffExplainerPort) |
| `spectty_approval` | Signal approval-gate decisions back to Spectty |
| `spectty_status` | Emit structured status signals (AwaitingInput, Running, Done) |
| `spectty_cost` | Report token/cost deltas |

### How injection works

The `ProvisionerAdapter` injects in the **agent's native configuration format** using
managed markers and atomic writes with backups. Scope is **global by default**; project-
scoped injection is used when the config is versioned alongside the repo.

```rust
trait Provisioner: Send + Sync {
    /// Inject Spectty tools into the agent's config, idempotently.
    fn inject(&self, scope: ProvisioningScope) -> Result<()>;

    /// Remove Spectty tools on session close / user request.
    fn retract(&self, scope: ProvisioningScope) -> Result<()>;

    /// Called after a session reconnect — re-assert markers are intact.
    fn refresh(&self) -> Result<()>;
}

enum ProvisioningScope { Global, Project(PathBuf) }
```

The Core never reads agent config files directly; the `ProvisionerAdapter` owns all
file I/O. The Core calls `ProvisioningPort::provision(agent, scope)` and receives a
`ProvisioningHandle` it can use to retract on session teardown.

### Patterns from gentle-ai

The provisioner design is modelled on the injection patterns established in the
gentle-ai/engram stack. Spectty's provisioner **coexists** with gentle-ai's own
provisioner (e.g. for Claude Code) — they each own distinct managed-marker regions and
do not overwrite each other.

## Extension path (future)

1. **Phase 1 (MVP):** Claude Code (Cooperative) + Generic. Provisioner for Claude Code only.
2. **Phase 2:** a declarative manifest (`agent.toml`: command, prompt regexes, cost regex,
   MCP tool list) that produces a runner without recompiling — covers 80% of agents.
3. **Phase 3:** WASM/plugin runners for agents needing real logic.

## Anti-patterns (forbidden)

- `if agent == "claude"` anywhere in the Core. Forbidden.
- Status detection regexes scattered across the UI. They belong in the agent adapter.
- The PTY adapter knowing which agent it runs. It runs a `LaunchSpec`, nothing more.
- Agent config file I/O in the Core. All provisioning goes through `ProvisioningPort`.
- Assuming all agents are Cooperative. Generic fallback must always work.

> ✅ DECIDED (MVP agents): Claude Code (Cooperative tier, full Spectty Agent Protocol) + the Generic adapter (PTY-scraping fallback, any CLI). Cursor CLI, Codex CLI, and Aider are fast-follows, not MVP.

> ❓ OPEN: Provisioner conflict resolution — if gentle-ai's provisioner and Spectty's
> provisioner both target Claude Code's config, define the canonical marker ownership
> boundaries. See [agent-protocol.md](agent-protocol.md).
