# Proposal: M4 — The Triad (Living Spec Pane + VibeLens + Why)

> Change name: `M4-triad-spec-vibelens`. Scope source: roadmap.md section
> **"M3 — Living Spec Pane + VibeLens (The Triad)"**. Renamed M4 in our SDD cycle because
> the prior cycle shipped hook-based status detection instead. Roadmap-M4 (multi-session +
> worktree isolation) is explicitly OUT of scope here.

## 1. Intent

### Problem
Spectty today is an instrumented terminal: it spawns a Claude Code agent, provisions a
FROZEN 5-tool MCP contract (`spectty_spec`, `spectty_diff`, `spectty_approval`,
`spectty_status`, `spectty_cost`), and detects agent lifecycle status via a hook sidecar.
But the marquee experience — Spectty's **signature triad** — does not exist yet:

- The MCP tools are stubs: `tools/call` returns a benign ACK with **no side effect**
  (`spectty-mcp/src/main.rs`, M2 closing note: "M3 = implement the spectty_* tool EFFECTS").
- `EngramAdapter` is a `todo!()` — persistence to engram was never wired (the prior cycle
  pivoted to hooks). So "PersistencePort wired to engram, survives restart" is greenfield.
- There is no Spec pane, no plan-approval gate, no VibeLens panel, no diff pipeline.

The #1 pain this addresses is **agent drift** (ADR-0007): the agent misunderstands the ask,
makes plausible-but-wrong choices, and you discover the divergence late. The triad closes
that loop: **SPEC** (living contract + plan-approval gate + live task progress) → **DIFF**
(VibeLens panel) → **WHY** (per-file rationale), for a **single session**.

### Why now
M0–M2 shipped the load-bearing primitives this milestone composes: the Core hexagon, the
`PersistencePort` contract, the FROZEN MCP schema (the "M3-swap contract"), the
Provisioner, and the proven hook-sidecar local-IPC pattern. Everything is staged for the
tool EFFECTS to land behind the unchanged schema. This is the first user-demoable moment —
without it, Spectty is "just a terminal wrapper."

### Success looks like (exit criteria, mapped from roadmap)
1. Spawn a Claude Code (Cooperative) session; seed intent in the Spec pane.
2. Agent submits a plan via `spectty_spec`; the **plan-approval gate appears**; user approves.
3. Agent begins work; **task states update live** in the Spec pane (no manual refresh).
4. Agent edits 3 files; the **VibeLens panel updates within seconds** via `spectty_diff`.
5. **Per-file rationale** is accurate and human-readable.
6. **Restart Spectty mid-session**; spec + progress are restored from engram.
7. **Generic agent (no injection)**: PTY-scraping drives approximate Spec state; FileWatcher
   drives VibeLens — both degrade gracefully.
8. Triad layout: Spec pane + Terminal + VibeLens panel all visible per session.

## 2. Scope

### In scope
- Give the FROZEN 5 MCP tools real EFFECTS behind the unchanged advertised schema. This
  milestone fully wires `spectty_spec`, `spectty_diff`, `spectty_approval`; `spectty_status`
  and `spectty_cost` land as minimal effect stubs (cost UI is M5) sufficient for the triad.
- `EngramAdapter` real implementation against engram's local HTTP API on `:7437` (upsert +
  get), plus a per-session poll loop with change detection emitting Tauri events.
- Core entities: `SpecContract` + `TaskState` + `ApprovalState` (pure data + transitions),
  `DiffExplanation` entity, and port trait definitions `FileWatchPort`, `DiffExplainerPort`,
  `GitPort` (interfaces are Core-owned; impls are adapters).
- Plan-approval gate as a Core business rule (gate fires before first code edit; one-way
  `TaskState` transitions; dev override) per ADR-0007.
- VibeLens pipeline: FileWatch (notify, debounced) OR cooperative `spectty_diff` signal →
  `GitPort::diff_head` → hash dedup → `DiffExplainerPort::explain` (MCP client of the
  existing VibeLens server) → `Session::update_diff` → `diff_updated` event.
- Tauri bridge: `get_spec` / `get_diff_explanation` commands; `spec_updated` / `diff_updated`
  events; per-session pipeline wiring in `spawn_session` / `session_runtime.rs`.
- UI triad: Spec pane (live checklist + approval gate) + VibeLens panel + layout; `ipc.ts`
  contract additions.
- Carry-forward **W1 doc-only fix** (zero-risk): the pipeline-augmentation scenario
  contradiction in `openspec/specs/agent-runner/spec.md` (~line 173+) — the `(Starting, Ready)`
  scenario vs the M2 baseline core transition table.

### Out of scope
- `GitPort`-driven worktrees / multi-session UI / session switcher (roadmap-M4).
- Dashboard, OS notifications (`NotifierPort`), checkpoints (M5 / post-MVP).
- Cost UI beyond the minimal `spectty_cost` effect stub the triad needs (M5).
- HTTP-callback IPC port negotiation as a primary transport (kept as a reserve option;
  see Decision 1).
- Changing the advertised MCP tool schema (FROZEN — D15/R4). Only EFFECTS change.
- Adding any external dependency to `crates/core` (cargo-deny quarantine intact).

## 3. Approach and ratified decisions

The exploration (obs #867) surfaced six decisions requiring a position. Each is ratified
below with rationale and tradeoffs.

### Decision 1 — Transport (MCP tool → app): engram-as-bus, with two carve-outs
**Ratified:** Engram is the primary bus for `spectty_spec` and `spectty_cost`. The
`spectty-mcp` binary gains a thin engram HTTP client (serde only — never core/tauri, D16),
upserts to engram `:7437` and returns immediately. The Spectty backend runs a per-session
poll loop (default 2 s, configurable) inside the `EngramAdapter` seam, detects change, reads,
and emits the Tauri event. This is the documented design (data-flow.md sequence (d)),
satisfies "survives restart" **for free** (engram IS the store), and keeps one mechanism for
spec + cost.

**Carve-out A — the live-checklist hot path:** the 2 s poll latency is flagged by both the
roadmap exit criterion ("no manual refresh") and spec-pane.md as a UX risk. We do NOT block
on it. The proven **state-file side-channel** (the M3 hook-sidecar pattern: atomic write,
monotonic `ts`, consume-once, `run_signal_loop` 200 ms tick) is held in **reserve** as a
low-latency path for the spec hot path IF 2 s proves sluggish in acceptance. Design phase
decides whether to ship it in M4 or defer; engram-as-bus is the baseline that satisfies all
exit criteria on its own.

**Carve-out B — `spectty_approval` is a BLOCKING round-trip.** The agent waits for the
user's approve/deny before continuing. Fire-and-forget polling cannot serve this cleanly.
**Ratified mechanism: a pending-future / resolver seam** — the `tools/call` for
`spectty_approval` registers a pending request keyed by `(session_id, action_id)`, surfaces
it as `AwaitingInput` + `quick_actions` (reusing the M2 status path), and resolves when the
UI sends `approve_prompt` (data-flow.md sequence (e)). This requires the MCP server and the
app to share a resolution channel. Two viable plumbings — (i) the app holds the future and
the MCP tool polls/long-polls engram for the resolution it writes back, or (ii) a localhost
HTTP callback (M3 archive deferred port negotiation to "M4+"). **Pin the exact plumbing in
design.** This is the single most novel transport question and is isolated to one slice.

> Tradeoff: engram-as-bus trades latency (2 s) for restart-survival and a single mechanism.
> HTTP-callback would be lower-latency and bidirectional but reintroduces port
> negotiation/auth work the prior cycle deliberately deferred. We accept the latency for
> spec/cost and isolate the only genuinely-blocking tool (approval) as its own decision.

### Decision 2 — PersistencePort reconciliation: KEEP the shipped sync/String port; add a SEPARATE subscribe seam in the adapter
**Ratified:** Do NOT rewrite the Core port to async + `serde_json::Value` + `subscribe` +
`search` (the doc vision). The shipped `PersistencePort` is the **load-bearing M0 contract**,
guarded by cargo-deny and boundary tests, and is deliberately sync + opaque-String so
`serde_json` stays adapter-side. We **keep `upsert(&str, String)` / `get(&str)` exactly as
shipped.** The polling/subscribe behavior lives **inside the `EngramAdapter`** (or a thin
adapter-side `SubscribePort`-style trait that is NOT the persistence port), where async,
reqwest, broadcast channels, and change detection are legal. The Tauri layer drives the poll
loop on a Tokio task and bridges change → event.

> Tradeoff: the doc's `PersistencePort::subscribe` abstraction is convenient but would force
> async into the Core boundary and pull `serde_json::Value` across it — violating the
> quarantine. Keeping persistence pure and pushing subscribe into the adapter preserves the
> M0 invariant at the cost of a slightly less elegant single-port story. Design must define
> the adapter-side subscribe surface explicitly (callback registration + change detection)
> WITHOUT touching `ports/persistence.rs`.

### Decision 3 — Core entities: SpecContract + TaskState + ApprovalState are pure Core entities
**Ratified:** The living contract and plan-approval gate are **business rules** (ADR-0007:
gate fires before any code edit; one-directional `TaskState` transitions; dev override), so
they belong in the Core as a rich domain model — mirroring how `AgentStatus::transition`
already lives in Core. Core gains `SpecContract { intent, proposal, tasks[], progress[],
approval, steering_notes }`, `TaskState` (pending/in_progress/done/skipped with legal
transitions), and `ApprovalState` (Pending/Approved/Rejected/Adjusted). All serde+thiserror
pure — no I/O. Serialization to engram is the adapter's job (PersistencePort takes a String).

> Tradeoff: an adapter-only "spec is just an engram blob" would be faster but anemic — it
> loses the testable invariants (gate-before-edit, legal transitions) and contradicts
> ADR-0007's `SpecArtifact` being "Spectty's own domain type." We pay the modeling cost for
> testability and to keep the gate enforceable in pure unit tests.

### Decision 4 — VibeLens: Spectty as MCP CLIENT of the existing VibeLens server behind `DiffExplainerPort`
**Ratified:** A new `DiffExplainerPort` (Core-owned interface) is implemented by an
`McpAdapter` that calls the existing VibeLens MCP tool `show_diff_explanation` with
`git diff HEAD` + per-file analysis — the SAME tool the project's `CLAUDE.md` already wires.
`FileWatchPort` (notify crate, debounced 500 ms–1 s per session) is the generic fallback
trigger; the cooperative `spectty_diff` signal bypasses the debounce for lower latency
(`emits_diff_signals` capability already modeled). `GitPort::diff_head` handles the empty-repo
case (diff against the empty tree). Hash-dedup against `last_diff_hash` on the Session skips
redundant MCP calls.

> Tradeoff: VibeLens-as-linked-library is rejected (no Rust crate; it is an external MCP
> tool). The MCP-client boundary keeps the Core ignorant of MCP/HTTP at the cost of pinning
> the VibeLens transport (stdio subprocess vs HTTP) and the exact `show_diff_explanation`
> param schema — both flagged OPEN in vibelens-integration.md. **Design must pin both.**

### Decision 5 — topic_key canonical form: PIN the per-session form
**Ratified:** Use the spec-pane.md form: **`spectty/{session_id}/spec`** and
**`spectty/{session_id}/progress`** (session-prefixed), and by extension
`spectty/{session_id}/cost`. Reject the data-flow.md `spectty/specs/{id}` /
`spectty/cost/{id}` form. Rationale: grouping all artifacts under a single
`spectty/{session_id}/*` namespace makes per-session restore and cleanup a single prefix
operation and reads naturally as "this session's spec/progress/cost." Update data-flow.md's
write-trigger table to match in the spec phase (doc reconciliation, not a code decision).

> Tradeoff: data-flow.md currently uses the type-first form in two tables; we accept a small
> doc edit to converge on one canonical form rather than carry two inconsistent conventions
> into code.

### Decision 6 — EngramAdapter: define the minimal engram HTTP surface; biggest risk → its own slice
**Ratified:** Implement the `todo!()` against engram's documented local HTTP API on `:7437`:
`POST /api/observations` (upsert by topic_key) and `GET /api/observations?topic_key=...&since=...`
(read + change detection). The adapter owns reqwest, async, error mapping to
`PersistenceError::Backend`, and **graceful degradation when engram is down** (log + retain
last-known; never crash the session). Because this is the single largest greenfield risk, it
is the FIRST slice — everything else depends on persistence working. Design must confirm the
exact engram endpoint shapes and the `since`/`updated_at` change-detection field against the
running engram (verify, do not assume).

## 4. Slice plan (chained PRs, stacked-to-main, ≤400 lines each)

The exploration's five seams are ratified, with the W1 doc fix folded in as a zero-risk
task. Slices are dependency-ordered; each is an independently reviewable, mergeable PR.

1. **Slice 1 — EngramAdapter + poll loop (foundation).** Implement `EngramAdapter`
   upsert/get against `:7437`; adapter-side subscribe/poll seam + change detection; degrade
   when engram down. No schema change to `PersistencePort`. *Highest risk; everything depends
   on it.* Include the W1 doc-only fix here (or as a trailing zero-risk task) since it is
   independent of code.
2. **Slice 2 — Core SpecContract + `spectty_spec` effect.** Core `SpecContract` / `TaskState`
   / `ApprovalState` entities + transitions (pure, unit-tested); `spectty_spec` real effect
   (MCP server upserts to `spectty/{session_id}/spec`); poll loop emits `spec_updated`;
   `get_spec` command.
3. **Slice 3 — Plan-approval gate + `spectty_approval`.** Gate-before-first-edit business
   rule; `spectty_approval` blocking pending-future/resolver seam (the carve-out B plumbing);
   `approve_prompt` resolution path; `AwaitingInput` + `quick_actions` reuse.
4. **Slice 4 — VibeLens ports + diff pipeline + `spectty_diff`.** `FileWatchPort`,
   `DiffExplainerPort`, `GitPort` traits; notify-based debounced watcher; `McpAdapter` for
   `show_diff_explanation`; `git diff HEAD` + empty-repo handling; hash dedup;
   `spectty_diff` cooperative trigger; `diff_updated` event; `get_diff_explanation` command.
5. **Slice 5 — UI triad.** Spec pane (live checklist + approval gate UI) + VibeLens panel
   + triad layout; `ipc.ts` additions for `spec_updated` / `diff_updated` / `get_spec` /
   `get_diff_explanation`. Minimal `spectty_status` / `spectty_cost` effect stubs land where
   needed to satisfy the triad without M5 cost UI.

> Slicing is ratified at the intent level. `sdd-tasks` may re-balance line budgets within
> these seams (e.g. split Slice 1's adapter from the poll loop if it exceeds 400 lines), but
> the dependency order (persistence → spec → approval → diff → UI) is fixed.

## 5. Risks and open questions (for design to resolve)

- **R1 (Slice 1, highest):** `EngramAdapter` is greenfield — reqwest, async-on-a-sync-port
  bridging, change-detection field (`updated_at`/`since`), degrade-when-down. Verify the
  actual engram `:7437` endpoint shapes before coding.
- **R2 (Decision 1B):** `spectty_approval` blocking transport plumbing — pending-future via
  engram round-trip vs localhost HTTP callback. Pin in design; it is the only genuinely
  bidirectional/blocking tool.
- **R3 (Decision 1A):** 2 s poll latency vs the live-checklist UX. State-file side-channel
  held in reserve; design decides ship-in-M4 vs defer based on whether 2 s is acceptable.
- **R4 (Decision 4):** VibeLens MCP transport (stdio subprocess vs HTTP) and the exact
  `show_diff_explanation` parameter schema are both OPEN — pin against the VibeLens server.
- **R5:** Generic-tier graceful degradation (PTY-scraping for spec; FileWatcher for VibeLens)
  is an explicit exit criterion — must be preserved in every slice that adds a cooperative
  path.
- **R6:** Core quarantine — no new external deps in `crates/core`; all I/O (reqwest, notify,
  git, MCP client, poll loop) stays in adapters/src-tauri. cargo-deny + boundary tests guard
  this; verify after every Core touch.
- **R7 (doc):** W1 carry-forward — `(Starting, Ready) => Idle` scenario contradiction in
  `openspec/specs/agent-runner/spec.md` (~line 173+). Zero-risk doc fix; not M4 code.

## 6. Exit criteria → slice mapping

| Exit criterion (roadmap) | Slice(s) |
|---|---|
| Seed intent; agent submits plan via `spectty_spec` | 2 |
| Plan-approval gate appears; user approves | 3 |
| Task states update live (no refresh) | 1 (poll), 2 (effect+event) |
| VibeLens updates within seconds via `spectty_diff` | 4 |
| Per-file rationale accurate/readable | 4 |
| Restart mid-session; spec + progress restored from engram | 1 (persistence), 2 (spec restore) |
| Generic agent degrades gracefully (PTY-scrape + FileWatcher) | 2, 4, 5 |
| Triad layout visible per session | 5 |
