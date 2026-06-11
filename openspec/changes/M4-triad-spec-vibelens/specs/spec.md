# M4 — The Triad (Living Spec + VibeLens + Why) — Delta Spec

> SDD spec phase. Consumes `sdd/M4-triad-spec-vibelens/proposal` (obs #868) and
> `openspec/changes/M4-triad-spec-vibelens/proposal.md`, plus the M0–M3 baseline specs
> under `openspec/specs/` (persistence-port, agent-runner, session-registry, tauri-bridge,
> agent-session-ui, terminal-ui, provisioning-port, output-signal, spectty-hook-sidecar)
> and the FROZEN MCP schema in `crates/spectty-mcp/src/main.rs`.
> Drives `sdd-tasks` (alongside `sdd-design`). Artifact store: HYBRID
> (engram `sdd/M4-triad-spec-vibelens/spec` + this file + per-capability files in this dir).
>
> This is a DELTA spec: it states WHAT MUST be true after M4 is applied, on top of the
> M0–M3 baseline. It describes outcomes, NOT implementation. RFC 2119 keywords
> (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.
>
> Verification class per requirement:
> - **[unit]** — assertable under Strict TDD (`cargo test --workspace` / `pnpm -C ui test`)
>   without a real engram daemon, a real PTY, a real agent, the VibeLens server, or a
>   running app. These seed the Strict-TDD task list.
> - **[manual]** — real-app / real-Claude-Code / real-engram / real-VibeLens manual
>   acceptance check; the `sdd-verify` pass/fail gate, on top of the strict-TDD unit gate.
> - **[ci]** — enforced by existing CI gates (cargo build, clippy -D warnings, cargo-deny
>   Core quarantine).
>
> REQ numbering continues the project sequence; M4 uses the `M4-REQ-NN` namespace.

M0–M2 shipped the primitives this milestone composes: the Core hexagon, the SYNC `PersistencePort`
contract, the FROZEN 5-tool MCP schema (the "M3-swap contract"), the Provisioner, and (M3) the
hook-sidecar local-IPC pattern. M4 turns the instrumented terminal into the signature **triad** for
a SINGLE session: **SPEC** (living contract + plan-approval gate + live task progress) → **DIFF**
(VibeLens panel) → **WHY** (per-file rationale). It addresses agent **drift** (ADR-0007).

M4 gives the FROZEN MCP tools real EFFECTS behind the UNCHANGED advertised schema (the schema is
NOT modified — only effects change). It wires `EngramAdapter` to engram's local HTTP API and adds a
per-session change-detecting poll loop, introduces pure Core entities for the living contract and
the plan-approval gate, builds the VibeLens diff pipeline, and renders the triad UI. Generic-tier
(no-injection) fallbacks (PTY-scraped spec; FileWatcher-driven VibeLens) MUST degrade gracefully and
are explicit exit criteria.

This delta spec is organized by capability. The full text lives here; per-capability files in this
directory mirror these sections:
`persistence-port.md`, `spec-contract.md`, `spectty-spec-effect.md`, `plan-approval-gate.md`,
`spectty-approval.md`, `diff-pipeline.md`, `spectty-diff-effect.md`, `tauri-bridge.md`,
`spec-pane-ui.md`, `vibelens-panel-ui.md`, `triad-layout.md`, `restart-recovery.md`,
`agent-runner.md` (the W1 doc-only correction).

---

## Capability: persistence-port (engram adapter + poll/bus layer)

The shipped SYNC/String `PersistencePort` is the load-bearing M0 contract and MUST NOT change
(Decision 2). M4 implements the `EngramAdapter` `todo!()` against engram's local HTTP API on
`:7437` and adds a SEPARATE adapter-side subscribe/poll seam (NOT the persistence port) that drives
change detection and Tauri events. `serde_json` stays adapter-side. Canonical topic_key form is
`spectty/{session_id}/spec`, `spectty/{session_id}/progress`, `spectty/{session_id}/cost` (Decision 5).

### Requirement: PersistencePort signature is UNCHANGED by M4  [unit] [ci]  (M4-REQ-01)

The Core `PersistencePort` trait MUST keep its shipped shape EXACTLY: `&self`,
`upsert(&self, topic_key: &str, payload: String) -> Result<(), PersistenceError>` and
`get(&self, topic_key: &str) -> Result<Option<String>, PersistenceError>`, with `PersistenceError`
carrying its single `Backend(String)` variant and a missing key returning `Ok(None)`. M4 MUST NOT
add `async`, `subscribe`, `search`, `serde_json::Value`, or any new method to `ports/persistence.rs`.
`crates/core` MUST gain NO new external dependency (cargo-deny Core quarantine intact).

#### Scenario: Persistence port shape is unchanged after M4  [unit]
- **Given** the `PersistencePort` trait after M4
- **When** its method set and signatures are inspected
- **Then** they MUST match the M0 contract exactly (sync, `&self`, `String` payload, `Option<String>`
  read, single `Backend` error variant) with no method added or removed

#### Scenario: Core has no new external dependency  [ci]
- **Given** `crates/core` after M4
- **When** cargo-deny and the boundary test run
- **Then** `crates/core` MUST contain no engram/reqwest/notify/git/serde_json::Value/tauri dependency

### Requirement: EngramAdapter upserts and reads via engram HTTP and degrades gracefully  [unit]  (M4-REQ-02)

`EngramAdapter` MUST implement `PersistencePort` against engram's local HTTP API: `upsert` maps to a
POST that creates-or-updates an observation keyed by `topic_key`; `get` maps to a read keyed by
`topic_key` returning `Ok(Some(payload))` when present and `Ok(None)` when absent. All HTTP/reqwest/
async/serialization MUST live in the adapter. When engram is unreachable or returns an error, the
adapter MUST map it to `PersistenceError::Backend`, MUST log, MUST retain last-known state, and MUST
NOT panic or crash the session (Decision 6). The HTTP seam MUST be injectable so behavior is unit-
testable with a fake transport (no real daemon).

#### Scenario: upsert then get round-trips the payload (fake transport)  [unit]
- **Given** an `EngramAdapter` over a fake HTTP transport
- **When** `upsert("spectty/42/spec", payload)` then `get("spectty/42/spec")` run
- **Then** the read MUST return `Ok(Some(payload))` unchanged

#### Scenario: get of an absent topic_key returns Ok(None)  [unit]
- **Given** an `EngramAdapter` over a fake transport with no observation for the key
- **When** `get("spectty/99/spec")` runs
- **Then** it MUST return `Ok(None)` (absence is not an error)

#### Scenario: engram unreachable degrades without crashing  [unit]
- **Given** an `EngramAdapter` whose fake transport returns a connection error
- **When** `upsert` or `get` runs
- **Then** it MUST return `Err(PersistenceError::Backend(_))`, MUST NOT panic, AND the calling session
  MUST remain alive (degrade, do not crash)

#### Scenario: engram endpoint shapes are verified against a running daemon  [manual]
- **Given** a running engram on `:7437`
- **When** the adapter performs a real upsert + get for `spectty/{session_id}/spec`
- **Then** the payload MUST round-trip AND the `updated_at`/`since` change-detection field MUST be
  observed to change on update (design pins exact endpoint/field; verify, do not assume)

### Requirement: A per-session subscribe/poll seam detects change and emits without touching the port  [unit]  (M4-REQ-03)

M4 MUST provide an adapter/`src-tauri`-side subscribe/poll seam (NOT the Core `PersistencePort`) that,
per session, reads `spectty/{session_id}/spec` (and `/progress`) on a configurable interval (default
2 s) and detects change via the engram change-detection field (`updated_at`/`since`). On a detected
change it MUST read the new payload and invoke a registered callback EXACTLY ONCE per change (no
re-emit for an unchanged payload). The seam MUST be async/Tokio-side and injectable so change
detection is unit-testable with a fake reader.

#### Scenario: A changed observation triggers exactly one callback  [unit]
- **Given** the poll seam with a fake reader returning `updated_at=t1` then `updated_at=t2 (> t1)`
- **When** two poll ticks fire
- **Then** the change callback MUST be invoked EXACTLY ONCE (on the second tick) with the new payload

#### Scenario: An unchanged observation does not re-emit  [unit]
- **Given** the poll seam after consuming `updated_at=t2`
- **When** the next tick reads the same `updated_at=t2`
- **Then** the callback MUST NOT be invoked again

#### Scenario: A poll error is tolerated and the loop continues  [unit]
- **Given** the poll seam whose fake reader returns an error on one tick then a valid change next
- **When** the ticks fire
- **Then** the errored tick MUST NOT panic or stop the loop AND the subsequent change MUST still emit

---

## Capability: spec-contract (Core SpecContract / TaskState / ApprovalState)

`SpecContract`, `TaskState`, and `ApprovalState` are PURE Core entities (Decision 3, ADR-0007):
serde + thiserror only, no I/O, no time access, no agent name. They carry the living-contract data
and the legal transition rules; serialization to engram is the adapter's job.

### Requirement: SpecContract is a pure Core aggregate with the living-contract fields  [unit]  (M4-REQ-04)

`spectty-core` MUST define `SpecContract` carrying at minimum `intent` (dev-seeded), `proposal`
(agent-generated), `tasks: Vec<…>` (each with `id`, `title`, `status: TaskState`, optional `notes`),
`progress`, `approval: ApprovalState`, and `steering_notes`. It MUST be serde-round-trippable and
MUST contain no I/O, no `reqwest`, no `tauri`, no time access.

#### Scenario: SpecContract round-trips through serde  [unit]
- **Given** a populated `SpecContract` value
- **When** it is serialized then deserialized
- **Then** the round-tripped value MUST equal the original

#### Scenario: SpecContract is pure  [unit]
- **Given** the `SpecContract` definition
- **When** its module is inspected
- **Then** it MUST reference no filesystem/network/time API and no hard-coded agent name

### Requirement: TaskState enforces one-directional legal transitions  [unit]  (M4-REQ-05)

`TaskState` MUST define `pending`, `in_progress`, `done`, `skipped`. A pure transition rule MUST
permit only forward progress: `pending → {in_progress, skipped}`, `in_progress → {done, skipped}`;
`done` is terminal; an illegal transition MUST leave the state unchanged (mirroring `AgentStatus::
transition`). No backward transition (e.g. `done → in_progress`) is permitted.

#### Scenario: pending advances to in_progress  [unit]
- **Given** a task in `pending`
- **When** an `in_progress` transition is applied
- **Then** the task MUST become `in_progress`

#### Scenario: done is terminal and rejects backward transition  [unit]
- **Given** a task in `done`
- **When** an `in_progress` transition is applied
- **Then** the task MUST remain `done` (illegal backward transition is ignored, not an error)

#### Scenario: pending may be skipped  [unit]
- **Given** a task in `pending`
- **When** a `skipped` transition is applied
- **Then** the task MUST become `skipped`

### Requirement: ApprovalState models the plan-approval lifecycle  [unit]  (M4-REQ-06)

`ApprovalState` MUST define `Pending`, `Approved`, `Rejected`, `Adjusted`. The initial state of a
freshly submitted plan MUST be `Pending`. Resolution transitions (`Pending → Approved/Rejected/
Adjusted`) MUST be representable as pure values; `Approved` is the only state that satisfies the
gate (see plan-approval-gate).

#### Scenario: A submitted plan starts Pending  [unit]
- **Given** a `SpecContract` constructed from a freshly submitted plan
- **When** its `approval` is inspected
- **Then** it MUST be `ApprovalState::Pending`

#### Scenario: Approval resolves to Approved  [unit]
- **Given** an `ApprovalState::Pending`
- **When** an approve resolution is applied
- **Then** it MUST become `ApprovalState::Approved`

---

## Capability: plan-approval-gate (Core business rule)

The plan-approval gate is a PURE Core business rule (ADR-0007): the agent MUST NOT begin code edits
until the human approves the plan. The gate predicate is enforceable in unit tests with no I/O.

### Requirement: The gate blocks code edits until the plan is Approved  [unit]  (M4-REQ-07)

`spectty-core` MUST expose a pure predicate (e.g. `SpecContract::may_edit()` or equivalent) that
returns true ONLY when `approval == Approved`. While `approval` is `Pending`, `Rejected`, or
`Adjusted`, the predicate MUST return false. A dev override MUST be representable (an explicit
override flag/transition that the human controls) so the gate can be bypassed deliberately; the
override MUST NOT be the default and MUST be distinguishable from a normal `Approved`.

#### Scenario: Pending plan does not permit edits  [unit]
- **Given** a `SpecContract` with `approval = Pending`
- **When** the gate predicate is evaluated
- **Then** it MUST return false (edits gated)

#### Scenario: Approved plan permits edits  [unit]
- **Given** a `SpecContract` with `approval = Approved`
- **When** the gate predicate is evaluated
- **Then** it MUST return true

#### Scenario: Rejected plan does not permit edits  [unit]
- **Given** a `SpecContract` with `approval = Rejected`
- **When** the gate predicate is evaluated
- **Then** it MUST return false

#### Scenario: Dev override permits edits without normal approval  [unit]
- **Given** a `SpecContract` with `approval = Pending` and the dev override engaged
- **When** the gate predicate is evaluated
- **Then** it MUST return true AND the override MUST be distinguishable from a normal `Approved`

---

## Capability: spectty-spec-effect (real effect behind FROZEN schema)

`spectty_spec` gains a real EFFECT behind the UNCHANGED advertised schema (`{session_id, spec:{proposal,
tasks[]:{id,title,status,notes}}}`). The MCP binary upserts to `spectty/{session_id}/spec` via its thin
engram HTTP client (serde only — never core/tauri, D16) and returns immediately; the poll loop emits
`spec_updated`.

### Requirement: The MCP tool schema is FROZEN; only effects change  [unit] [ci]  (M4-REQ-08)

`crates/spectty-mcp/src/main.rs` MUST keep the advertised `tools/list` schema for all five tools
(`spectty_spec`, `spectty_diff`, `spectty_approval`, `spectty_status`, `spectty_cost`) BYTE-FOR-BYTE
as shipped (canonical order, parameter shapes). M4 changes ONLY the `tools/call` effect bodies. The
binary MUST depend on `serde`/`serde_json` and MAY gain a thin engram HTTP client, but MUST NOT
depend on `crates/core` or `tauri`.

#### Scenario: tools/list schema is unchanged  [unit]
- **Given** the `tools/list` response after M4
- **When** it is compared against the frozen baseline schema
- **Then** the advertised tool names, order, and parameter schemas MUST be identical

#### Scenario: spectty-mcp depends on serde only (no core/tauri)  [ci]
- **Given** `crates/spectty-mcp` dependencies after M4
- **When** they are inspected
- **Then** they MUST NOT include `spectty-core` or `tauri`

### Requirement: spectty_spec upserts the contract to engram and surfaces live  [unit]  (M4-REQ-09)

A `tools/call` for `spectty_spec` MUST parse the `{session_id, spec}` payload and upsert it to
`spectty/{session_id}/spec`, then return immediately (fire-and-forget). The backend poll loop MUST,
on detecting the change, read the payload and emit a Tauri `spec_updated { session_id, spec }` event.
A malformed payload MUST be rejected without crashing the MCP binary.

#### Scenario: spectty_spec upserts under the canonical key  [unit]
- **Given** a `spectty_spec` call for `session_id = 42` over a fake engram client
- **When** the effect runs
- **Then** an upsert MUST target `spectty/42/spec` with the serialized contract AND the call MUST
  return promptly

#### Scenario: A spec change emits spec_updated once  [unit]
- **Given** the poll loop detecting a new `spectty/42/spec` payload
- **When** the change is observed
- **Then** EXACTLY ONE `spec_updated { session_id: 42, spec }` event MUST be emitted

#### Scenario: Malformed spectty_spec payload is rejected without crash  [unit]
- **Given** a `spectty_spec` call with a payload missing required `spec` fields
- **When** `handle_message` dispatches it
- **Then** it MUST return an error/benign result without panicking and MUST NOT upsert a partial blob

---

## Capability: spectty-approval (blocking pending-future / resolver seam)

`spectty_approval` is the ONLY genuinely BLOCKING tool: the agent waits for the human's
approve/deny. M4 implements the pending-future / resolver seam (Decision 1, carve-out B): a
`tools/call` registers a pending request keyed `(session_id, action_id)`, surfaces it as
`AwaitingInput` + `quick_actions` (reusing the M2 status path), and resolves when the UI sends
`approve_prompt`. Exact plumbing (engram round-trip vs localhost HTTP callback) is pinned in design.

### Requirement: spectty_approval registers a pending request and surfaces it as AwaitingInput  [unit]  (M4-REQ-10)

A `tools/call` for `spectty_approval` (advertised schema `{session_id, action_id, description,
risk_level, options[]}` — UNCHANGED) MUST register a pending request keyed by `(session_id,
action_id)` and surface it to the UI as `AwaitingInput` carrying `quick_actions` derived from
`options[]`. The request MUST remain pending until resolved; duplicate `(session_id, action_id)`
registrations MUST be idempotent (one pending entry).

#### Scenario: An approval call registers one pending request  [unit]
- **Given** the resolver seam with no pending requests
- **When** a `spectty_approval { session_id, action_id, options }` is registered
- **Then** EXACTLY ONE pending request keyed `(session_id, action_id)` MUST exist AND it MUST surface
  as `AwaitingInput` with `quick_actions` from `options`

#### Scenario: Duplicate action_id registration is idempotent  [unit]
- **Given** a pending request for `(42, "edit-1")`
- **When** the same `(42, "edit-1")` is registered again
- **Then** there MUST still be exactly one pending request for that key

### Requirement: approve_prompt resolves the pending request and unblocks the agent  [unit]  (M4-REQ-11)

When the UI sends `approve_prompt` for `(session_id, action_id)` with a decision, the resolver MUST
resolve the matching pending request with that decision, remove it from pending, and the resolution
MUST become observable to the blocked `spectty_approval` caller. An `approve_prompt` for an unknown
`(session_id, action_id)` MUST be a no-op (no crash).

#### Scenario: approve_prompt resolves a pending approval  [unit]
- **Given** a pending request for `(42, "edit-1")`
- **When** `approve_prompt(42, "edit-1", Approved)` is received
- **Then** the request MUST resolve as `Approved`, MUST be removed from pending, AND the resolution
  MUST be retrievable by the blocked caller

#### Scenario: approve_prompt for an unknown key is a no-op  [unit]
- **Given** no pending request for `(42, "ghost")`
- **When** `approve_prompt(42, "ghost", Approved)` is received
- **Then** it MUST be ignored without error and MUST NOT create a pending entry

---

## Capability: diff-pipeline (FileWatchPort / DiffExplainerPort / GitPort / VibeLens client)

A new pipeline produces per-file rationale: a trigger (FileWatch debounced 500 ms–1 s OR cooperative
`spectty_diff`) → `GitPort::diff_head` → hash-dedup vs `last_diff_hash` → `DiffExplainerPort::explain`
(MCP client of the existing VibeLens server, tool `show_diff_explanation`) → `Session::update_diff` →
`diff_updated` event (Decision 4). Ports are Core-owned interfaces; impls are adapters. Empty-repo and
all failure modes degrade without crashing.

### Requirement: FileWatchPort / DiffExplainerPort / GitPort are Core-owned interfaces with adapter impls  [unit] [ci]  (M4-REQ-12)

`spectty-core` MUST define `FileWatchPort`, `DiffExplainerPort` (e.g. `explain(diff: &str, workspace:
&Path) -> Result<DiffExplanation>`), and `GitPort` (with `diff_head`) as pure trait interfaces with
no I/O dependency in Core. `DiffExplanation` MUST be a pure serde Core entity (with an empty/`empty()`
form). All implementations (notify watcher, VibeLens MCP client, git) MUST live in adapters/`src-tauri`;
`crates/core` MUST gain no new external dependency.

#### Scenario: The three ports are Core traits with no I/O dep  [unit] [ci]
- **Given** `crates/core` after M4
- **When** the port modules are inspected and cargo-deny runs
- **Then** `FileWatchPort`, `DiffExplainerPort`, `GitPort`, and `DiffExplanation` MUST be present and
  pure AND Core MUST carry no `notify`/`git`/MCP/`reqwest` dependency

#### Scenario: DiffExplanation round-trips and has an empty form  [unit]
- **Given** a `DiffExplanation` value and `DiffExplanation::empty()`
- **When** each is serialized then deserialized
- **Then** both MUST round-trip unchanged AND `empty()` MUST represent "no diff to explain"

### Requirement: The diff pipeline hash-dedups and skips redundant explanations  [unit]  (M4-REQ-13)

The pipeline MUST compute a hash of the `git diff HEAD` output and compare it against the Session's
`last_diff_hash`. If the hash is unchanged, it MUST skip the `DiffExplainerPort::explain` call (no
redundant MCP call). On a changed hash, it MUST call `explain`, update `Session` with the new
`DiffExplanation` and `last_diff_hash`, and emit `diff_updated`. The empty-repo case MUST diff against
the empty tree; a truly empty diff MUST yield `DiffExplanation::empty()` with NO MCP call.

#### Scenario: An unchanged diff hash skips the explainer  [unit]
- **Given** a Session whose `last_diff_hash` equals the current diff hash
- **When** the pipeline runs
- **Then** `DiffExplainerPort::explain` MUST NOT be called AND no `diff_updated` MUST be emitted

#### Scenario: A changed diff hash explains and emits once  [unit]
- **Given** a Session whose `last_diff_hash` differs from the current diff hash
- **When** the pipeline runs over a fake `DiffExplainerPort`
- **Then** `explain` MUST be called once, the Session MUST store the new explanation + hash, AND
  EXACTLY ONE `diff_updated { session_id, explanation }` MUST be emitted

#### Scenario: A truly empty diff yields empty() with no MCP call  [unit]
- **Given** an empty repository (no commits, no changes) diffed against the empty tree
- **When** the pipeline runs
- **Then** the result MUST be `DiffExplanation::empty()` AND `DiffExplainerPort::explain` MUST NOT be called

### Requirement: The diff pipeline degrades gracefully on every failure mode  [unit]  (M4-REQ-14)

When the VibeLens MCP client is unreachable, times out, returns an error, or returns an unparseable
response, OR when `GitPort::diff_head` fails, the pipeline MUST log, retain the previous
`DiffExplanation`, surface an "unavailable"/"parse error" state to the panel, and MUST NOT crash the
session. No failure mode may panic or terminate the PTY.

#### Scenario: VibeLens unreachable retains previous explanation  [unit]
- **Given** a Session with a prior `DiffExplanation` and a fake `DiffExplainerPort` returning a
  connection error
- **When** the pipeline runs
- **Then** the prior explanation MUST be retained, an "unavailable" state MUST be surfaced, AND the
  session MUST remain alive

#### Scenario: A git failure does not crash the session  [unit]
- **Given** a fake `GitPort` whose `diff_head` returns an error
- **When** the pipeline runs
- **Then** it MUST log and surface a degraded state without panicking or terminating the session

---

## Capability: spectty-diff-effect (cooperative trigger behind FROZEN schema)

`spectty_diff` (advertised schema `{session_id, hint?}` — UNCHANGED) gains a real EFFECT: a cooperative
signal that triggers the diff pipeline immediately, bypassing the FileWatch debounce for lower latency.
The `emits_diff_signals` capability (already modeled, false for claude-code today) governs whether the
cooperative path is active; otherwise the FileWatch fallback drives the pipeline.

### Requirement: spectty_diff cooperatively triggers the pipeline, bypassing debounce  [unit]  (M4-REQ-15)

A `tools/call` for `spectty_diff` MUST trigger the diff pipeline for `session_id` WITHOUT waiting for
the FileWatch debounce window, then return promptly. When no cooperative signal arrives (generic tier),
the FileWatch debounced trigger MUST drive the pipeline instead. Both paths MUST converge on the SAME
pipeline (diff → dedup → explain → emit).

#### Scenario: A cooperative spectty_diff bypasses the debounce  [unit]
- **Given** a session with the diff pipeline wired and a fake clock
- **When** a `spectty_diff { session_id }` signal arrives
- **Then** the pipeline MUST run immediately without waiting for the debounce window

#### Scenario: Generic tier falls back to the debounced FileWatch trigger  [unit]
- **Given** a session whose agent has `emits_diff_signals = false` and a debounced `FileWatchPort`
- **When** files change with no cooperative signal
- **Then** the pipeline MUST run via the debounced FileWatch trigger (degrades gracefully) — same
  pipeline, no cooperative signal required

---

## Capability: tauri-bridge (spec/diff commands + events)

The `src-tauri` bridge gains `get_spec` / `get_diff_explanation` commands and `spec_updated` /
`diff_updated` events (Tauri v2 `Emitter`), plus per-session pipeline wiring in `spawn_session` /
`session_runtime.rs`. This extends the M0/M2 bridge without removing existing commands/events.

### Requirement: get_spec and get_diff_explanation commands are registered  [unit]  (M4-REQ-16)

`src-tauri` MUST register `get_spec(session_id)` returning the current `SpecContract` (or absent) and
`get_diff_explanation(session_id)` returning the current `DiffExplanation` (or absent), each returning
`Result<_, _>` over owned types. The existing M0/M2 commands MUST remain registered.

#### Scenario: Both new commands are registered alongside existing ones  [unit]
- **Given** the `generate_handler!` registration after M4
- **When** the command set is inspected
- **Then** `get_spec` AND `get_diff_explanation` MUST be present AND the existing `spawn_session` /
  `close_session` / `list_sessions` / `get_session` commands MUST still be present

### Requirement: spec_updated and diff_updated are emitted via the Tauri v2 Emitter  [unit]  (M4-REQ-17)

`src-tauri` MUST emit `spec_updated { session_id, spec }` when the poll loop detects a spec change and
`diff_updated { session_id, explanation }` when the diff pipeline produces a new explanation, both via
the Tauri v2 `Emitter` API (NOT v1 signatures). Each MUST fire only on an ACTUAL change (no emit for an
unchanged spec/diff).

#### Scenario: spec_updated fires only on an actual spec change  [unit]
- **Given** the bridge wired to the poll seam
- **When** the seam reports no change
- **Then** NO `spec_updated` MUST be emitted; it MUST fire only when the payload actually changes

#### Scenario: diff_updated carries session_id and explanation  [unit]
- **Given** the diff pipeline producing a new explanation for a session
- **When** `diff_updated` is emitted
- **Then** its payload MUST carry `session_id` and the `explanation`, via the v2 `Emitter`

---

## Capability: spec-pane-ui (live checklist + approval gate)

The UI gains a Spec pane: it seeds intent, renders the living checklist reacting to `spec_updated`
(no manual refresh), and presents the plan-approval gate (Approve / Edit / Reject) that calls
`approve_prompt`. It follows the M1/M2 hook pattern (`useSession`-style, React 19 named imports, no
manual `useMemo`/`useCallback`) with `invoke`/listeners mocked in vitest. Generic tier shows a coarse
scraped badge when only PTY-scraping is available.

### Requirement: The Spec pane renders the live checklist from spec_updated  [unit]  (M4-REQ-18)

The Spec pane MUST render the `SpecContract` tasks as a checklist and update task states LIVE on each
`spec_updated` event WITHOUT a manual refresh. Each task MUST show its `TaskState`
(pending/in_progress/done/skipped).

#### Scenario: A spec_updated event updates the checklist without refresh  [unit]
- **Given** the mounted Spec pane with the event listener mocked
- **When** a `spec_updated` event delivers a contract with a task moved to `done`
- **Then** the checklist MUST reflect that task as `done` with no manual refresh

#### Scenario: Generic tier shows a coarse scraped badge  [unit]
- **Given** a session whose progress source is PTY-scraping only (no structured spec)
- **When** the Spec pane renders
- **Then** it MUST show a coarse status badge (approximate state) rather than a precise checklist,
  degrading gracefully

### Requirement: The plan-approval gate presents Approve/Edit/Reject and calls approve_prompt  [unit]  (M4-REQ-19)

When `approval == Pending`, the Spec pane MUST present the plan-approval gate with Approve, Edit, and
Reject actions. Selecting one MUST invoke `approve_prompt` with `(session_id, action_id, decision)`.
The gate MUST NOT render once `approval` is resolved (Approved/Rejected/Adjusted).

#### Scenario: Approving the plan invokes approve_prompt  [unit]
- **Given** the Spec pane with a Pending approval and `invoke` mocked
- **When** the user clicks Approve
- **Then** `approve_prompt` MUST be invoked with the session id, action id, and an `Approved` decision

#### Scenario: The gate hides once approval is resolved  [unit]
- **Given** the Spec pane after a `spec_updated` event with `approval = Approved`
- **When** the pane re-renders
- **Then** the plan-approval gate MUST NOT be shown

---

## Capability: vibelens-panel-ui (per-file rationale)

The UI gains a VibeLens panel that renders the `DiffExplanation` reacting to `diff_updated`, showing
per-file rationale, and a manual refresh control. It surfaces the degraded "unavailable"/"parse error"
state from the pipeline. Same hook/test conventions as the Spec pane.

### Requirement: The VibeLens panel renders per-file rationale from diff_updated  [unit]  (M4-REQ-20)

The VibeLens panel MUST render the `DiffExplanation` per-file rationale and update on each
`diff_updated` event without a manual refresh. An empty explanation MUST render a clear "no changes"
state, and a degraded state MUST render "unavailable"/"parse error" (not a crash or blank panel).

#### Scenario: A diff_updated event renders per-file rationale  [unit]
- **Given** the mounted VibeLens panel with the listener mocked
- **When** a `diff_updated` event delivers a multi-file explanation
- **Then** the panel MUST render each file's rationale

#### Scenario: A degraded explanation renders an unavailable state  [unit]
- **Given** the panel receiving a degraded/unavailable explanation state
- **When** it renders
- **Then** it MUST show an "unavailable"/"parse error" indicator and MUST NOT blank or crash

### Requirement: A manual refresh control re-runs the diff explanation  [unit]  (M4-REQ-21)

The VibeLens panel MUST provide a manual refresh control that triggers a fresh diff explanation for
the session (invoking the pipeline / a refresh command), independent of the automatic FileWatch/
cooperative trigger.

#### Scenario: Manual refresh triggers a fresh explanation  [unit]
- **Given** the VibeLens panel with `invoke` mocked
- **When** the user clicks manual refresh
- **Then** a refresh invocation for the session MUST be issued (forcing a fresh diff explanation)

---

## Capability: triad-layout

The session view MUST present the triad — Spec pane + Terminal + VibeLens panel — all visible for a
single session (exit criterion 8 / roadmap).

### Requirement: The triad layout shows Spec pane, Terminal, and VibeLens per session  [unit] [manual]  (M4-REQ-22)

The session view MUST lay out three regions simultaneously for one session: the Spec pane, the
existing Terminal pane, and the VibeLens panel. All three MUST be visible without navigating away
from the session.

#### Scenario: All three triad regions render for a session  [unit]
- **Given** the mounted session view for one active session
- **When** it renders
- **Then** the Spec pane, the Terminal, AND the VibeLens panel MUST all be present in the layout

---

## Capability: restart-recovery

Restarting Spectty mid-session MUST restore the spec + progress from engram (exit criterion 6),
because engram IS the store (Decision 1).

### Requirement: Spec and progress are restored from engram on restart  [unit] [manual]  (M4-REQ-23)

On session re-attach after a restart, Spectty MUST read `spectty/{session_id}/spec` (and
`spectty/{session_id}/progress`) from engram and reconstruct the `SpecContract` so the Spec pane shows
the prior intent, plan, task states, and approval state. If engram is unreachable at restart, the pane
MUST degrade gracefully (empty/last-known state) without crashing.

#### Scenario: Restored spec reconstructs the contract  [unit]
- **Given** an engram (fake reader) holding a `spectty/42/spec` payload from a prior run
- **When** the session is re-attached after restart
- **Then** the reconstructed `SpecContract` MUST equal the persisted one (intent, tasks, states,
  approval)

#### Scenario: Restart with engram down degrades gracefully  [unit]
- **Given** a restart where the engram reader returns a connection error
- **When** re-attach runs
- **Then** the Spec pane MUST show an empty/last-known state without crashing

#### Scenario: Manual restart mid-session restores spec + progress  [manual]
- **Given** an active session with an approved plan and partial task progress, then Spectty is
  restarted
- **When** the session is re-opened
- **Then** the Spec pane MUST show the prior intent, plan, task states, and approval restored from engram

---

## Capability: agent-runner (W1 doc-only correction)

W1 is a ZERO-RISK documentation correction folded into M4 (Slice 1). The implementation was always
correct; the spec prose lagged. M4 MUST lock the corrected scenario in the baseline `agent-runner`
spec so the pipeline-augmentation `transition` scenario matches the M2 core table row
`((Starting, Ready), Idle)`.

### Requirement: The agent-runner pipeline-augmentation Ready scenario matches the core table  [ci]  (M4-REQ-24)

`openspec/specs/agent-runner/spec.md` pipeline-augmentation scenario for a hook-derived `Ready`
observation MUST assert `(Starting, Ready) => Idle` (NOT `Starting` unchanged), consistent with the
M2 baseline transition table and the existing core test row `((Starting, Ready), Idle)`. This is a
doc-only change — NO code change, NO new test.

#### Scenario: The Ready scenario reads (Starting, Ready) => Idle  [ci]
- **Given** the `agent-runner` spec pipeline-augmentation section after M4
- **When** the hook-derived `Ready` scenario is read
- **Then** it MUST state that `transition(Starting, Ready)` returns `Idle` (matching the core table),
  with no contradictory "Starting unchanged" wording remaining

---

## Acceptance gate (M4 exit criteria → scenarios)  [manual]

These are the `sdd-verify` pass/fail gate, on top of the strict-TDD unit gate. All require a real
Claude Code session, a real engram, and the real VibeLens server. macOS is the gating platform.

| Roadmap exit criterion | Traces to |
|---|---|
| 1. Seed intent; agent submits plan via `spectty_spec` | M4-REQ-09, M4-REQ-18 |
| 2. Plan-approval gate appears; user approves | M4-REQ-07, M4-REQ-19, M4-REQ-10/11 |
| 3. Task states update live (no refresh) | M4-REQ-03, M4-REQ-09, M4-REQ-17, M4-REQ-18 |
| 4. VibeLens updates within seconds via `spectty_diff` | M4-REQ-13, M4-REQ-15, M4-REQ-20 |
| 5. Per-file rationale accurate/readable | M4-REQ-12, M4-REQ-20 |
| 6. Restart mid-session restores spec + progress | M4-REQ-02, M4-REQ-23 |
| 7. Generic agent degrades gracefully (PTY-scrape + FileWatcher) | M4-REQ-15, M4-REQ-18, M4-REQ-14 |
| 8. Triad layout visible per session | M4-REQ-22 |

### Requirement: M4 satisfies all eight roadmap exit criteria  [manual]  (M4-REQ-25)

#### Scenario: End-to-end triad happy path on macOS
- **Given** a real Claude Code (Cooperative) session, a running engram, and the VibeLens server
- **When** the user seeds intent, the agent submits a plan via `spectty_spec`, the gate appears and the
  user approves, the agent edits 3 files, and `spectty_diff` fires
- **Then** the gate MUST have appeared before edits, task states MUST update live with no manual
  refresh, the VibeLens panel MUST update within seconds with accurate per-file rationale, AND the
  triad layout MUST be visible throughout

#### Scenario: Generic agent end-to-end degradation
- **Given** a Generic-tier agent (no `spectty_*` injection) in a session
- **When** the agent runs and edits files
- **Then** PTY-scraping MUST drive approximate Spec state AND the FileWatcher MUST drive VibeLens —
  both degrading gracefully without crashing the session
