# Stack Integration — Building on engram and gentle-ai

Spectty deliberately builds *on top of* the engram and gentle-ai stack rather
than re-implementing equivalent infrastructure. This document explains what each
component contributes, how Spectty isolates itself behind ports, what gaps it
must fill, the license situation, and the single non-trivial risk.

Related: [../decisions/0005-build-on-gentle-ai-stack.md](../decisions/0005-build-on-gentle-ai-stack.md) ·
[../research/gentle-ai-stack.md](../research/gentle-ai-stack.md) ·
[agent-protocol.md](agent-protocol.md) · [domain-model.md](domain-model.md)

---

## Per-component treatment

### engram — runtime dependency behind `PersistencePort`

engram is a persistent memory store: Go process, SQLite + FTS5 backend,
`topic_key` upsert semantics. It exposes two interfaces:

| Interface | Transport | Used by |
|---|---|---|
| stdio MCP server | JSON-RPC over stdin/stdout | Agent tooling (agents call `mem_save`, `mem_search`, etc.) |
| Local HTTP server on `:7437` | REST / SSE | Spectty backend (hooks, polling, direct persistence reads) |

Spectty treats engram as an **external runtime dependency** — the user installs
engram separately (or Spectty bundles it as a sidecar). The Rust backend never
imports engram code; it calls the HTTP API.

All engram access is routed through `PersistencePort`:

```rust
trait PersistencePort: Send + Sync {
    async fn upsert(&self, topic_key: &str, payload: &serde_json::Value) -> Result<()>;
    async fn get(&self, topic_key: &str) -> Result<Option<serde_json::Value>>;
    async fn search(&self, query: &str, project: &str) -> Result<Vec<SearchResult>>;
    async fn subscribe(&self, topic_key: &str, callback: Box<dyn Fn(serde_json::Value) + Send>) -> Result<SubscriptionHandle>;
}
```

The concrete implementation (`EngramAdapter`) calls engram's HTTP `:7437`
endpoints. If engram is not running, the adapter returns a structured error;
Spectty degrades gracefully (Spec pane shows cached or empty state; cost
metrics persist in-memory only for the session).

#### What Spectty stores via PersistencePort

| Data | Topic key pattern | Notes |
|---|---|---|
| Session metadata | `spectty/sessions/{session_id}` | Status, workspace path, title, timestamps |
| Spec (live plan) | `spectty/specs/{session_id}` | JSON from `spectty_spec` tool calls |
| CostMetrics | `spectty/cost/{session_id}` | Accumulated per session |
| Checkpoints | `spectty/checkpoints/{session_id}/{checkpoint_id}` | Git ref + label |
| Provisioner state | `spectty/provisioner/{agent}/{scope}` | Last SHA fingerprint, backup path |

All keys are namespaced under `spectty/` to avoid collisions with engram
records written by agents themselves (`sdd/`, `skill-registry`, etc.).

---

### gentle-ai — patterns copied, provisioner coexists

gentle-ai is a CLI that provisions agent environments (injects MCP tools, hook
configs, SKILL.md files). Spectty does **not** depend on the gentle-ai binary.
Instead, it copies the patterns (MIT license, zero obligation on Spectty's
code) and ships its own `Provisioner` implementation.

What Spectty copies:
- Three-layer injection structure (MCP tools + hook additionalContext + SKILL.md).
- Managed-section markers (`spectty:managed:start` / `spectty:managed:end`).
- Atomic write (.tmp → rename) + backup-before-write.
- SHA fingerprint cache for the refresh hook.
- Per-agent native-format adapters (file paths, JSON/TOML structure per agent).

What Spectty does NOT copy:
- The gentle-ai binary or its command surface.
- Any dependency on gentle-ai's internal packages.
- The `gentle-ai:managed` section namespace (Spectty uses its own).

The two provisioners coexist in the same agent config files using distinct
managed-section namespaces. A user running both gentle-ai and Spectty has two
managed blocks in their `CLAUDE.md`; each tool manages only its own.

---

### SDD — artifact model adopted and generalized

Spectty's Spec data model is generalized from SDD's artifact lifecycle
(proposal → spec → tasks → apply-progress → verify). The `spectty_spec` tool
schema (see [agent-protocol.md](agent-protocol.md)) expresses this progression
as a typed JSON structure.

Agents that run SDD phases will naturally map their artifact state to
`spectty_spec` calls. Agents that do not use SDD can still express their own
task lists with the same structure. The generalization is intentional: Spectty
is not an SDD runner; it is a cockpit that happens to speak a superset of SDD's
vocabulary.

---

## Gaps Spectty fills

engram is a **store**, not a pub/sub system. It has no real-time event stream.
This is the single most significant technical gap between what engram provides
and what Spectty needs for a live Spec pane.

### Gap 1 — No real-time event stream (the #1 technical problem)

**Problem:** When an agent calls `spectty_spec`, the payload is upserted into
engram. Nothing in engram notifies the Spectty backend that the record changed.

**Spectty's approach — polling with a subscribe abstraction:**

The `PersistencePort::subscribe` method hides the implementation detail. The
`EngramAdapter` implements it via polling:

```
EngramAdapter
  └── poll loop (default: 2 s interval)
        ├── GET /api/observations?topic_key=spectty/specs/{session_id}&since={last_updated_at}
        ├── if changed → emit internal Tokio broadcast
        └── subscribers (Spec pane handler) → emit Tauri `spec_updated` event
```

The polling interval is configurable per deployment. A tighter interval (500 ms)
is available for local development; a looser interval (5 s) is appropriate when
engram is running on a slower machine or the user has many sessions open.

**Future path — push/subscribe layer:**

If engram gains SSE or WebSocket push notification support, the `EngramAdapter`
can switch to a push-driven implementation without changing `PersistencePort` or
any code above it. The port is the firewall.

> ❓ OPEN: Evaluate engram's roadmap for native pub/sub. If it is planned,
> prototype the polling adapter first and defer the push implementation to M3+.
> If it is not planned, Spectty should consider contributing a minimal SSE
> endpoint upstream.

### Gap 2 — Progress only at task granularity after a batch

SDD records apply-progress after each batch, not after each individual task
within a batch. Spectty addresses this by having agents call `spectty_spec`
after each individual task transition (the SKILL.md instructs this), not only
at batch boundaries. This produces finer-grained live updates without changing
engram.

### Gap 3 — Narrative progress vs. structured JSON

engram stores LLM-generated prose summaries. Spectty requires structured JSON
(task IDs, statuses, done/in-progress/pending arrays) for the Spec pane to
render a live progress bar and task checklist. The `spectty_spec` tool enforces
the structured schema; the agent fills it via the SKILL.md guidance.

### Gap 4 — Zero cost/visibility UI

engram stores cost data in `CostMetrics` observations, but has no UI. Spectty's
Dashboard consumes cost data via `PersistencePort::get` on the
`spectty/cost/{session_id}` key and renders it with a per-session cost bar and a
workspace aggregate. The `cost_updated` Tauri event drives incremental updates.

---

## License

Both engram and gentle-ai are MIT-licensed. Consequences:

| Usage | Obligation |
|---|---|
| Using engram as a runtime dependency (calling its HTTP API) | Zero. No source disclosure required. Spectty's own code may be any license. |
| Copying engram source into Spectty | Preserve the MIT copyright notice in the copied files. |
| Copying gentle-ai patterns (not source) | None. Patterns and ideas are unrestricted. |
| Copying gentle-ai source files | Preserve the MIT copyright notice in the copied files. |

Spectty may ship as a proprietary product without conflict.

---

## Risk: fast-moving solo-maintainer project

gentle-ai is maintained by a single developer and evolves quickly. Breaking
changes to its provisioning format (file paths, config schema, managed-section
markers) would normally require Spectty to track upstream.

**Mitigation:** Spectty depends on engram's stable interfaces (HTTP `:7437` API,
topic_key semantics, MCP stdio protocol) — not on gentle-ai's binary or internal
structure. Changes to gentle-ai's CLI do not affect Spectty unless engram's HTTP
API changes, which is a much slower-moving surface.

For the copied patterns (provisioner logic), Spectty is already independent:
once copied, the patterns evolve at Spectty's pace. If gentle-ai improves its
provisioner, Spectty can selectively adopt improvements without being forced to.

The `PersistencePort` is the primary firewall. All engram-specific details
(HTTP paths, auth, response shapes) are encapsulated in `EngramAdapter`. If
engram's API changes, only the adapter needs updating.

> ❓ OPEN: Define a minimal integration test that runs against a real engram
> instance and validates the `EngramAdapter`'s contract. This test should run
> in CI against a pinned engram version and alert when engram publishes a new
> release so the adapter can be validated before adopting the new version.
