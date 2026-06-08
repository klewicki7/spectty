# Spectty Agent Protocol

The Spectty Agent Protocol defines how Spectty makes cooperative agents
structurally aware of their operating context — the live Spec, the current cost,
and the approval gate — instead of relying on scraping PTY output after the fact.

Related: [domain-model.md](domain-model.md) · [agent-abstraction.md](agent-abstraction.md) ·
[spec-pane.md](../product/spec-pane.md) · [stack-integration.md](stack-integration.md) ·
[../research/gentle-ai-stack.md](../research/gentle-ai-stack.md)

---

## Why a protocol, not just a PTY scraper

PTY scraping (reading output, matching regexes, guessing when an agent is
waiting) is the Generic fallback — it works, but it is fragile and lossy.
Cooperative agents that speak the protocol give Spectty:

- **Structured progress**: JSON task lists, not paragraph summaries.
- **Explicit approval gates**: the agent calls `spectty_approval` rather than
  printing "Do you want to…" into the terminal.
- **Live cost reporting**: token counts pushed as they accumulate.
- **Spec pane fidelity**: the Spec updates incrementally as the agent marks
  tasks done, not only after a batch completes.

The protocol does not require agents to be rewritten. Three-layer injection
(described below) adds capabilities to any agent that supports MCP tools,
hook callbacks, and a skill/rule file — without modifying the agent's source.

---

## Tool suite

Five MCP tools form the structured surface. Each is exposed by the Spectty
backend via its own MCP server (stdio, one per session — see
[stack-integration.md](stack-integration.md)).

### `spectty_spec`

Report structured plan progress. The canonical path for updating the Spec pane.

```json
{
  "name": "spectty_spec",
  "description": "Push plan progress to the Spectty Spec pane. Call this after each task transition.",
  "inputSchema": {
    "type": "object",
    "required": ["session_id", "spec"],
    "properties": {
      "session_id":  { "type": "string" },
      "spec": {
        "type": "object",
        "properties": {
          "proposal":   { "type": "string", "description": "One-sentence goal" },
          "tasks": {
            "type": "array",
            "items": {
              "type": "object",
              "required": ["id", "title", "status"],
              "properties": {
                "id":     { "type": "string" },
                "title":  { "type": "string" },
                "status": { "enum": ["pending", "in_progress", "done", "skipped"] },
                "notes":  { "type": "string" }
              }
            }
          },
          "apply_progress": {
            "type": "object",
            "properties": {
              "completed": { "type": "array", "items": { "type": "string" } },
              "current":   { "type": "string" },
              "remaining": { "type": "array", "items": { "type": "string" } }
            }
          }
        }
      }
    }
  }
}
```

The task status model is intentionally generalized from the SDD artifact model
(proposal → spec → tasks → apply-progress → verify). Agents that do not use
SDD can still express their own task lists with the same structure.

**Effect:** Spectty backend persists the spec payload via `PersistencePort`
(engram upsert), then emits `spec_updated` → Spec pane re-renders.

---

### `spectty_diff`

Trigger a VibeLens diff explanation on demand from the agent. Equivalent to
what Spectty's FileWatcher pipeline does automatically, but agent-initiated.

```json
{
  "name": "spectty_diff",
  "description": "Request a diff explanation for the current session's worktree.",
  "inputSchema": {
    "type": "object",
    "required": ["session_id"],
    "properties": {
      "session_id": { "type": "string" },
      "hint":       { "type": "string", "description": "Optional one-line context for the explainer (e.g. 'refactored auth module')" }
    }
  }
}
```

This is the same `DiffExplainerPort` path as the file-watch pipeline;
`spectty_diff` is just the agent-initiated entry point. The backend emits
`diff_updated` on completion.

---

### `spectty_approval`

Raise a structured approval gate. Replaces ad-hoc "Do you want to…" output
with a typed request that Spectty surfaces directly in the UI.

```json
{
  "name": "spectty_approval",
  "description": "Request user approval before a risky action. Blocks the agent until the user responds.",
  "inputSchema": {
    "type": "object",
    "required": ["session_id", "action_id", "description"],
    "properties": {
      "session_id":  { "type": "string" },
      "action_id":   { "type": "string", "description": "Stable ID for this gate (used to resume)" },
      "description": { "type": "string", "description": "Human-readable description of the action requiring approval" },
      "risk_level":  { "enum": ["low", "medium", "high"], "default": "medium" },
      "options":     {
        "type": "array",
        "items": { "type": "string" },
        "description": "Allowed responses; defaults to ['approve', 'deny']"
      }
    }
  }
}
```

**Effect:** transitions `AgentStatus` → `AwaitingInput` (structured variant),
emits `status_changed` with `quick_actions` populated from `options`. The user's
response is delivered via the existing `approve_prompt` command, using the same
`action_id`. The backend unblocks the agent by writing the selected option to
the PTY.

> ❓ OPEN: Blocking the MCP call until the user responds requires holding a
> pending future in the backend. Define the timeout and cancellation policy
> (e.g. 10-minute wall-clock timeout → auto-deny with a `timeout` reason).

---

### `spectty_status`

Push a short status string that appears in the session's status bar. Useful for
progress messages that do not warrant a full Spec update.

```json
{
  "name": "spectty_status",
  "description": "Push a transient status message to the session badge and status bar.",
  "inputSchema": {
    "type": "object",
    "required": ["session_id", "message"],
    "properties": {
      "session_id": { "type": "string" },
      "message":    { "type": "string", "description": "Short status (≤80 chars)" },
      "phase":      { "type": "string", "description": "Optional phase label (e.g. 'planning', 'coding', 'verifying')" }
    }
  }
}
```

**Effect:** emits `status_changed` with the message embedded in the payload.
Does not persist; it is a transient display hint.

---

### `spectty_cost`

Report accumulated token cost directly. Used when the agent has better cost
data than Spectty's PTY-scraping cost parser can extract.

```json
{
  "name": "spectty_cost",
  "description": "Push accumulated token/cost metrics for this session.",
  "inputSchema": {
    "type": "object",
    "required": ["session_id", "delta"],
    "properties": {
      "session_id": { "type": "string" },
      "delta": {
        "type": "object",
        "properties": {
          "input_tokens":  { "type": "integer" },
          "output_tokens": { "type": "integer" },
          "cache_read_tokens": { "type": "integer" },
          "estimated_usd": { "type": "number" }
        }
      },
      "model": { "type": "string", "description": "Model ID used for this delta (e.g. 'claude-sonnet-4-5')" }
    }
  }
}
```

**Effect:** `CostMetrics` updated via the `AgentRunner` cost path, persisted,
`cost_updated` event emitted.

---

## Three-layer injection

No agent natively speaks the Spectty protocol. Injection adds the three layers
without touching the agent's source.

```
┌──────────────────────────────────────────────────────────┐
│ Layer 1 — MCP tools                                       │
│   spectty_spec / spectty_diff / spectty_approval          │
│   spectty_status / spectty_cost                           │
│   Registered in the agent's MCP config per native format  │
│   (Claude Code: .mcp.json; Cursor: mcp.json; etc.)        │
└──────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────┐
│ Layer 2 — hook additionalContext                           │
│   At prompt time, Spectty injects the current session_id, │
│   the live Spec JSON (truncated), and a one-line reminder  │
│   ("use spectty_spec after every task transition").        │
│   Claude Code uses the hooks.additionalContext callback;   │
│   other agents use their nearest equivalent.              │
└──────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────┐
│ Layer 3 — SKILL.md / rules file                           │
│   A managed SKILL.md (or .cursor/rules, etc.) defines     │
│   WHEN and HOW to call the tools: after each task, before  │
│   a destructive action, at session end. This is the        │
│   "memory" for cooperative behavior across compactions.   │
└──────────────────────────────────────────────────────────┘
```

All three layers are written and maintained by the **Provisioner** (see below).
Layers 1 and 3 are static (written once per provisioning); Layer 2 is dynamic
(re-injected at each prompt via the refresh hook).

---

## Two-tier model

### Tier 1 — Cooperative

The agent supports MCP, hooks, and rule files. Spectty injects all three
layers. The agent calls `spectty_spec` / `spectty_approval` / etc. at the
right moments because the SKILL.md instructs it to and the hook reinforces
the instruction.

Spectty's status detection for Cooperative agents relies primarily on
`spectty_approval` calls (structured `AwaitingInput`) rather than PTY pattern
matching. The Generic PTY scraper still runs as a fallback in case the agent
misses a call.

**Cooperative agents (planned):** Claude Code, Cursor Agent, any MCP-capable
agent with a rule/skill file mechanism.

### Tier 2 — Generic

The agent does not support MCP or structured rules. Spectty falls back to
PTY scraping: the `AgentRunner::detect_status` implementation matches known
output patterns (prompts, approval lines, idle-after-question heuristic) and
parses cost from output. No `spectty_*` tools are available.

The Generic tier provides baseline value — the cockpit still shows terminal
output, file diffs, basic status, and OS notifications — but the Spec pane
shows only the last text summary (or nothing) rather than live structured
progress.

**Generic agents:** Aider, Codex CLI, any CLI agent without MCP support.

> ❓ OPEN: Should the Generic tier inject a simplified SKILL.md via a shell
> `SPECTTY_SESSION_ID` env var, giving the agent a hint to emit
> machine-readable markers in its output? Low-cost fallback worth exploring.

---

## The Provisioner

The Provisioner writes and maintains the injection artifacts for each agent in
its native config format. Patterns are copied from the gentle-ai CLI provisioner
(see [../research/gentle-ai-stack.md](../research/gentle-ai-stack.md));
Spectty ships its own implementation that coexists with any existing gentle-ai
provisioner rather than depending on or replacing it.

### Per-agent native format adapters

| Agent | MCP config | Rule/skill file | Hook config |
|---|---|---|---|
| Claude Code | `.mcp.json` (global `~/.claude/` or project) | `CLAUDE.md` / `skills/*.md` | `hooks.additionalContext` in settings |
| Cursor | `.cursor/mcp.json` | `.cursor/rules/*.mdc` | n/a (no hook equivalent yet) |
| Codex CLI | `~/.codex/config.toml` `[mcp_servers]` | `~/.codex/instructions.md` | n/a |
| Generic | env `MCP_SERVERS` (if supported) | none | none |

Each adapter knows the exact file paths, JSON/TOML structures, and key names
for its agent. The Core sees only a `ProvisioningPort` trait; adapters handle
the format details.

### Managed-section markers

Spectty writes only into clearly marked sections so it does not clobber user
content:

```
# spectty:managed:start — DO NOT EDIT (managed by Spectty)
… spectty-injected content …
# spectty:managed:end
```

Files that do not yet contain a managed section have one appended. Files that
already contain a `spectty:managed` block have only that block replaced.

### Atomic writes

All config writes follow: write to `.tmp` file → `fsync` → atomic rename.
This prevents a crash mid-write from leaving a partially-written config that
breaks the agent's startup.

### Backup before write

Before the first write to any file, the Provisioner copies the existing file to
`<file>.spectty.bak`. A UI action ("Reset to pre-Spectty config") restores the
backup and removes the managed section.

### Refresh hook + SHA fingerprint cache

The Layer 2 `additionalContext` injection must re-fire at every prompt, not
just at provisioning time (the Spec changes between prompts). The Provisioner
registers a refresh hook with the agent's hook mechanism. To avoid writing
identical content on every prompt, the Provisioner keeps a SHA-256 hash of the
last-written context string; it skips the write if the hash is unchanged
(fingerprint cache).

### Global vs. project scope

| Scope | When used | Effect |
|---|---|---|
| **Global** (default) | Tool is not version-controlled; user preference | Writes to `~/.claude/`, `~/.cursor/`, etc. |
| **Project** | Spec / rules should be versioned with the repo | Writes to `.mcp.json`, `CLAUDE.md` in the workspace root |

Spectty uses Global by default and promotes to Project scope when the user opts
in ("version this agent config with the repo").

---

## How the protocol powers Spectty's UI panels

### Spec pane

The Spec pane displays the live `Spec` JSON from the most recent `spectty_spec`
call. Spectty maintains a polling loop over `PersistencePort` (engram HTTP
`:7437`) at a configurable interval (default 2 s) — this is the primary
mechanism bridging the stateless engram store and the live UI. When the agent
calls `spectty_spec`, the payload is stored; the poll loop detects the change and
emits `spec_updated`, which the Spec pane reacts to.

See [stack-integration.md](stack-integration.md) for the engram gap analysis
and the polling vs. push/subscribe design.

### AgentStatus / approval UI

`spectty_approval` transitions `AgentStatus` → `AwaitingInput` (structured).
The `status_changed` event carries `quick_actions` built from the tool's
`options` field. The Dashboard highlights the session; the Pane badge pulses;
the user clicks an action or presses a keybind → `approve_prompt` command →
the backend resolves the pending MCP call → the agent unblocks.

For Generic agents, `AwaitingInput` is still detected via PTY scraping; the
same `status_changed` / `approve_prompt` path fires. The difference is
granularity and reliability: scraping can miss prompts; `spectty_approval`
never can.

### CostMetrics panel

`spectty_cost` delivers precise deltas that accumulate in `CostMetrics`.
For Generic agents, the `AgentRunner::parse_cost` scraper approximates this
from output lines. Both paths write to the same `CostMetrics` struct;
the UI does not distinguish the source.

---

## Gentle-ai pattern lineage

The three-layer injection, managed-section markers, atomic writes,
backup-before-write, and SHA fingerprint cache are all patterns proven in the
gentle-ai CLI codebase. Spectty copies the patterns (MIT license, unrestricted)
and ships its own implementation so:

1. Spectty does not depend on the gentle-ai binary — no tight version coupling.
2. The two provisioners coexist in the same agent config (each has its own
   managed-section marker namespace: `spectty:managed` vs. `gentle-ai:managed`).
3. Spectty can evolve the protocol independently without waiting for upstream
   changes.

See [../decisions/0005-build-on-gentle-ai-stack.md](../decisions/0005-build-on-gentle-ai-stack.md)
for the build-on vs. independent decision rationale.

---

## Anti-patterns (forbidden)

- **Agent name in the Core.** No `if agent == "claude"` anywhere inside the
  Core or its domain types. Agent-specific logic lives exclusively in the
  per-agent `AgentRunner` adapter and the per-agent `Provisioner` adapter.
  Violating this collapses the port boundary — see
  [ADR-0004](../decisions/0004-agent-agnostic-core.md).

- **PTY scraping as primary path for Cooperative agents.** The Generic fallback
  scraper must run, but it must not be the authoritative source for agents that
  speak the protocol. If a Cooperative agent calls `spectty_approval`, that
  call — not any regex match — drives the `AwaitingInput` transition.

- **Mutating agent config outside the Provisioner.** Any write to `.mcp.json`,
  `CLAUDE.md`, or equivalent must go through the Provisioner's atomic-write
  path. Ad-hoc file writes risk corrupting the managed section and breaking the
  agent's startup.

- **Embedding Spectty's business logic in the SKILL.md.** The SKILL.md
  instructs the agent *when* to call which tool; it does not encode policies
  (e.g. "always approve low-risk actions"). Policy lives in the backend where
  it is version-controlled and testable.
