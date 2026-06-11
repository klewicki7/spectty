# Design: M4 — The Triad (Living Spec Pane + VibeLens + Why)

> Ratifies ADRs **D26–D38** (M2 ratified D7–D20, M3 ratified D21–D25; continue from D26).
> Reads proposal obs #868 + this codebase. Strict TDD; 5 chained PRs stacked-to-main, ≤400 lines.
> Hexagonal: Core stays serde+thiserror pure (R6 quarantine); all I/O in adapters/src-tauri.

## Technical Approach

Give the FROZEN 5 MCP tools real EFFECTS behind the unchanged schema, wire `EngramAdapter`
against engram's local HTTP, add three pure Core entities + three port traits, and build the
React triad. The MCP→app transport is **engram-as-bus** (proposal D1): `spectty-mcp` upserts to
engram and returns; the backend runs a per-session **poll loop** (a Tokio task, mirroring the
M3 `run_signal_loop` injected-emit discipline) that reads via the unchanged sync `PersistencePort`,
detects change, and emits Tauri events. `spectty_approval` is the one BLOCKING tool and gets a
dedicated resolver seam (D31). VibeLens is an MCP **client** (stdio subprocess — VERIFIED) behind
`DiffExplainerPort`. Maps 1:1 to the proposal's 5 slices.

## Architecture Decisions

### D26 — Engram HTTP client lives in a thin `EngramHttp` trait; `EngramAdapter` wraps it
**Choice**: Split the reqwest transport into a private `trait EngramHttp { async fn post_observation(..); async fn get_observation(topic_key, since) -> Option<Obs>; }` and have `EngramAdapter` (the `PersistencePort` impl) own an `Arc<dyn EngramHttp>` + a Tokio handle to bridge sync→async (`tokio::runtime::Handle::block_on` on a dedicated runtime, NOT the Tauri main runtime).
**Alternatives**: reqwest calls inline in `EngramAdapter` (no contract-test seam); rewrite the port async (rejected by D2/D27).
**Rationale**: The exact engram REST surface is UNVERIFIED (see Pre-Apply Gate G1). A trait lets us write contract tests against an in-memory `FakeEngramHttp` double NOW and swap the verified real shapes behind it without touching `PersistencePort` or the poll loop. block_on bridges the sync port to async reqwest legally inside the adapter.

### D27 — `PersistencePort` UNCHANGED; subscribe/poll is an adapter-side `SpecBus`, not a port method
**Choice**: Keep `upsert(&str, String)` / `get(&str)` exactly (verified at persistence.rs:27-36). The poll/change-detection lives in a NEW adapter struct `SpecBus` (in `src-tauri` or adapters) that holds `Arc<dyn PersistencePort>`, polls per topic_key on a cadence, and invokes an injected `emit: impl FnMut(Change)` closure — the SAME seam shape as `run_signal_loop`.
**Alternatives**: add `subscribe`/`search` to the port (the doc vision) — forces async + `serde_json::Value` across the Core boundary; violates R6 + cargo-deny.
**Rationale**: Preserves the load-bearing M0 contract and keeps `serde_json` adapter-side. Poll is pure-testable via a fake PersistencePort returning scripted payloads.

### D28 — Poll cadence + change detection: 2 s default, `updated_at`-monotonic, last-seen per topic
**Choice**: Per-session Tokio task, `tokio::time::interval(2s)` (configurable via env `SPECTTY_POLL_MS`). Change detection = compare the observation's `updated_at` (engram column, VERIFIED to exist) against a per-topic `last_updated_at: Option<i64>`; emit only on strictly-greater. If engram exposes a `?since=` filter use it; else fetch-and-compare (G1 decides). The poll loop owns NO Core mutation beyond reading — it deserializes the String payload (adapter-side) and emits the typed event.
**Alternatives**: content hash diff (works without `updated_at` but re-hashes every tick); fixed 200 ms (engram load).
**Rationale**: `updated_at` is the cheapest reliable signal; 2 s matches data-flow.md and satisfies all exit criteria. Carve-out A (D30) covers the latency risk.

### D29 — Tauri events: `spec_updated`, `diff_updated`; commands `get_spec`, `get_diff_explanation`
**Choice**: Add events `spec_updated { session_id, spec: SpecContract }` and `diff_updated { session_id, explanation: DiffExplanation }`, plus commands `get_spec(id) -> Option<SpecContract>` and `get_diff_explanation(id) -> Option<DiffExplanation>`, registered in `generate_handler!`. Approval surfaces via the EXISTING `status_changed` path (`AwaitingInput` + `quick_actions`) — no new approval event; resolution rides a new `approve_prompt(session_id, action_id, decision)` command.
**Alternatives**: a bespoke `approval_requested` event (duplicates the M2 status path the proposal says to reuse).
**Rationale**: Reuses the proven status pipeline for approval (D1B); minimal new IPC surface; mirrors existing `ipc.ts` listener pattern.

### D30 — Spec hot-path latency: ship engram-as-bus only in M4; state-file side-channel DEFERRED
**Choice**: Ship the 2 s poll as the sole spec transport in M4. Hold the M3 state-file side-channel in reserve; add it only if acceptance shows 2 s is too sluggish.
**Alternatives**: ship both now (doubles the mechanisms + tests for an unproven need).
**Rationale**: Engram-as-bus meets every exit criterion on its own; YAGNI. Acceptance criterion 3 ("live, no refresh") is satisfied by 2 s polling. Re-open only on evidence.

### D31 — `spectty_approval` blocking transport: engram round-trip resolver (NO HTTP callback)
**Choice**: `spectty-mcp`'s `spectty_approval` handler upserts the request to `spectty/{session_id}/approval` and then **long-polls** `get` on that same topic_key (e.g. every 500 ms, bounded timeout) for a resolution field written back by the app. The app's poll loop sees the pending approval → emits `status_changed(AwaitingInput, quick_actions)`. The `approve_prompt` command writes the decision into `ApprovalState` and upserts the resolved payload to the same topic_key. The MCP tool's long-poll observes the resolution and returns to the agent.
**Alternatives**: localhost HTTP callback (reintroduces the port-negotiation/auth work M3 deferred); in-process pending-future (impossible — MCP server is a separate process, serde-only, D16).
**Rationale**: Single mechanism (engram), no new transport, restart-survivable, keeps `spectty-mcp` serde+HTTP-client-only. Cost: approval latency = long-poll interval; acceptable for a human-in-the-loop gate. Isolated to Slice 3.

### D32 — Core entities `SpecContract` / `TaskState` / `ApprovalState` (pure, transition fns)
**Choice**: New `crates/core/src/entities/spec.rs`. `SpecContract { intent: String, proposal: Option<String>, tasks: Vec<SpecTask>, progress: Vec<TaskProgress>, approval: ApprovalState, steering_notes: Vec<String> }`. `SpecTask { id, title, state: TaskState }`. `enum TaskState { Pending, InProgress, Done, Skipped }` with `fn transition(self, to: TaskState) -> Result<TaskState, SpecError>` enforcing one-way legal moves (Pending→InProgress→Done/Skipped; no backward). `enum ApprovalState { Pending, Approved, Rejected, Adjusted }`. All `serde + thiserror`, no I/O — mirrors `AgentStatus::transition`.
**Alternatives**: anemic engram blob (loses testable invariants; contradicts ADR-0007).
**Rationale**: Gate-before-edit and legal transitions become pure unit tests.

### D33 — Plan-approval gate is a Core business rule on `SpecContract`
**Choice**: `SpecContract::may_begin_edits(&self) -> bool` returns true only when `approval == Approved` (dev override = a constructor flag). `apply_progress(task_id, new_state)` returns `Err(SpecError::GateNotApproved)` if a task would move to `InProgress` while approval is `Pending`. The adapter/MCP layer reads this; it never re-implements the rule.
**Alternatives**: gate enforced in src-tauri (rule leaks out of Core).
**Rationale**: ADR-0007 frames the gate as domain logic; pure-testable.

### D34 — `DiffExplanation` + `last_diff_hash` on the Session aggregate
**Choice**: New `crates/core/src/entities/diff.rs`: `DiffExplanation { files: Vec<FileExplanation>, summary: String }` + `DiffExplanation::empty()`. Extend `Session` with `last_diff: Option<DiffExplanation>` and `last_diff_hash: Option<u64>`, plus `Session::update_diff(expl, hash)`. Hash = `DefaultHasher` over the diff string (std only).
**Alternatives**: store diff only in the adapter (UI can't restore on reconnect; dedup state leaks out of the aggregate).
**Rationale**: Matches vibelens-integration.md; keeps dedup state with the aggregate; std-only hash respects R6.

### D35 — Three new Core port traits: `GitPort`, `FileWatchPort`, `DiffExplainerPort`
**Choice**: Define in `crates/core/src/ports/`. `GitPort::diff_head(&self, workspace) -> Result<String, GitError>` (handles empty-repo → diff vs empty tree). `FileWatchPort` exposes a subscription that yields debounced `FileChanged` batches. `DiffExplainerPort::explain(&self, diff, workspace) -> Result<DiffExplanation, ExplainError>`. Trait DEFINITIONS only in Core (interfaces are Core-owned); zero new Core deps.
**Alternatives**: concrete adapter types referenced from src-tauri without ports (couples the pipeline to git2/notify).
**Rationale**: Ports keep Core ignorant of git2/notify/MCP; enables fakes in pipeline tests.

### D36 — VibeLens transport: stdio subprocess (VERIFIED), `McpAdapter` owns the child
**Choice**: `DiffExplainerPort` impl `VibeLensMcpAdapter` spawns `npx -y vibelens-mcp` as a stdio child (newline-delimited JSON-RPC 2.0 — same framing as `spectty-mcp`), calls `show_diff_explanation { diff, file_analysis }`, parses the response into `DiffExplanation`. Manages subprocess lifecycle (spawn lazily on first explain, reuse, restart on crash).
**Alternatives**: HTTP client (VibeLens has no HTTP server — `.mcp.json` proves stdio).
**Rationale**: VERIFIED against `.mcp.json`. Param field names (`diff`, `file_analysis`) are illustrative — confirm via `tools/list` (Pre-Apply Gate G2).

### D37 — Diff pipeline trigger arbitration: cooperative `spectty_diff` bypasses FileWatch debounce
**Choice**: Pipeline = `(FileWatch debounced 500 ms–1 s) OR (spectty_diff signal)` → `GitPort::diff_head` → hash == `last_diff_hash`? skip : `DiffExplainerPort::explain` → `Session::update_diff` → emit `diff_updated`. The cooperative `spectty_diff` path fires immediately (no debounce wait); FileWatch is the generic fallback (`emits_diff_signals == false`). A shared "explain in flight" guard prevents the two triggers double-firing.
**Alternatives**: FileWatch only (slower for cooperative agents); both unconditionally (redundant MCP calls).
**Rationale**: Preserves R5 generic degradation while giving cooperative agents low latency; hash-dedup makes a double-trigger harmless.

### D38 — Restart recovery: poll loop hydrates from engram on session re-attach
**Choice**: On `spawn_session` (or re-attach), before starting the poll interval, do ONE `get(spectty/{session_id}/spec)` + `get(.../progress)` and emit an initial `spec_updated` so the UI restores immediately. Diff is NOT persisted to engram (transient enrichment) — VibeLens panel shows "no diff yet" until the next trigger. Cleanup: `close_session` adds best-effort deletes of the three `spectty/{session_id}/*` keys (single-prefix op per D5) alongside the existing state-file cleanup.
**Alternatives**: wait for the first poll tick (2 s blank UI on restart).
**Rationale**: Satisfies exit criterion 6 (restart restores spec+progress) with no extra latency.

## Data Flow

    Agent ──spectty_spec(stdio)──▶ spectty-mcp ──POST upsert──▶ engram :7437
                                                                    │ spectty/{sid}/spec
    SpecBus poll task (2s) ──GET (updated_at)──▶ engram ───────────┘
       │ change? deserialize String → SpecContract
       ▼
    emit("spec_updated") ──▶ ipc.ts listenSpecUpdated ──▶ SpecPane

    Approval (D31):  spectty_approval upsert .../approval + long-poll get
       app poll sees pending ─▶ status_changed(AwaitingInput, quick_actions)
       user ─▶ approve_prompt cmd ─▶ ApprovalState=Approved ─▶ upsert resolution
       spectty_approval long-poll reads resolution ─▶ returns to agent

    Diff (D37):  FileWatch(debounce) | spectty_diff ─▶ GitPort::diff_head
       ─▶ hash==last? skip : DiffExplainerPort::explain (vibelens stdio child)
       ─▶ Session::update_diff ─▶ emit("diff_updated") ─▶ VibeLensPanel

## File Changes

| File | Action | Description |
|------|--------|-------------|
| `crates/core/src/entities/spec.rs` | Create | `SpecContract`, `SpecTask`, `TaskState`, `ApprovalState`, transitions, gate rule (D32/D33) |
| `crates/core/src/entities/diff.rs` | Create | `DiffExplanation`, `FileExplanation`, `::empty()` (D34) |
| `crates/core/src/entities/session.rs` | Modify | Add `last_diff`, `last_diff_hash`, `update_diff` (D34) |
| `crates/core/src/ports/git.rs` | Create | `GitPort` trait (D35) |
| `crates/core/src/ports/file_watch.rs` | Create | `FileWatchPort` trait (D35) |
| `crates/core/src/ports/diff_explainer.rs` | Create | `DiffExplainerPort` trait (D35) |
| `crates/adapters/src/persistence/engram.rs` | Modify | Implement `todo!()` via `EngramHttp` trait + reqwest; degrade-when-down (D26) |
| `crates/adapters/src/git/mod.rs` | Create | `Git2Adapter` (or shell-git) `GitPort` impl, empty-repo handling (D35) |
| `crates/adapters/src/file_watch/mod.rs` | Create | `NotifyFileWatcher` debounced (D35) |
| `crates/adapters/src/diff/vibelens.rs` | Create | `VibeLensMcpAdapter` stdio client (D36) |
| `crates/spectty-mcp/src/main.rs` | Modify | Real effects: spec/diff/approval/status/cost upserts + approval long-poll; gain engram HTTP client (serde+http only, D16) |
| `src-tauri/src/session_runtime.rs` | Modify | Add `SpecBus` poll loop + diff pipeline alongside `run_signal_loop` (same emit-closure discipline) |
| `src-tauri/src/commands/session.rs` | Modify | Wire poll task + file watcher per session; restart hydrate; cleanup `spectty/{sid}/*` (D38) |
| `src-tauri/src/commands/spec.rs` | Create | `get_spec`, `get_diff_explanation`, `approve_prompt` commands (D29) |
| `src-tauri/src/lib.rs` | Modify | Register new commands + manage engram adapter; wire pipeline |
| `ui/src/session/ipc.ts` | Modify | Add `listenSpecUpdated`, `listenDiffUpdated`, `getSpec`, `getDiffExplanation`, `approvePrompt` |
| `ui/src/components/SpecPane.tsx` | Create | Live checklist + approval gate UI |
| `ui/src/components/VibeLensPanel.tsx` | Create | Per-file rationale panel |
| `ui/src/components/TriadLayout.tsx` | Modify/Create | Spec \| Terminal \| VibeLens layout |
| `openspec/specs/agent-runner/spec.md` | Modify | W1 doc-only fix `(Starting, Ready) => Idle` (zero-risk, Slice 1) |

## Interfaces / Contracts

```rust
// crates/core/src/entities/spec.rs (pure)
pub enum TaskState { Pending, InProgress, Done, Skipped }
impl TaskState { pub fn transition(self, to: TaskState) -> Result<TaskState, SpecError>; }
pub enum ApprovalState { Pending, Approved, Rejected, Adjusted }
pub struct SpecContract { /* intent, proposal, tasks, progress, approval, steering_notes */ }
impl SpecContract {
    pub fn may_begin_edits(&self) -> bool;                 // gate (D33)
    pub fn apply_progress(&mut self, task_id: &str, to: TaskState) -> Result<(), SpecError>;
}

// crates/core/src/ports/*.rs (trait defs only)
#[async_trait] pub trait GitPort: Send + Sync { async fn diff_head(&self, ws: &Path) -> Result<String, GitError>; }
pub trait FileWatchPort: Send + Sync { /* subscribe → debounced FileChanged batches */ }
#[async_trait] pub trait DiffExplainerPort: Send + Sync {
    async fn explain(&self, diff: &str, ws: &Path) -> Result<DiffExplanation, ExplainError>;
}
```
> NOTE: `async_trait` is needed on the Git/DiffExplainer ports. If `async-trait` is not already a Core
> dep, prefer a SYNC trait signature with the async bridged inside the adapter (block_on) to avoid a NEW
> Core dep (R6). **Tasks phase: confirm whether `async-trait` is already vendored in Core; if not, use sync
> port signatures.** Per data-flow.md the doc draws these async, but R6 forbids a new Core dep.

## Testing Strategy (Strict TDD — seams named)

| Layer | What | Approach |
|-------|------|----------|
| Unit (pure) | `TaskState::transition`, `SpecContract::may_begin_edits`/`apply_progress` gate, `ApprovalState`, `DiffExplanation::empty`, hash dedup | Core unit tests, no I/O (RED first) |
| Unit (pure) | `SpecBus` change detection (`updated_at` monotonic, emit-only-on-change) | Fake `PersistencePort` returning scripted payloads; inject `emit` closure (mirrors `observe_and_diff`) |
| Contract | `EngramAdapter` upsert/get + degrade-when-down | `FakeEngramHttp` in-memory double impl of `EngramHttp` trait; one ignored `#[test]` hitting real `:7437` gated behind G1 |
| Contract | `VibeLensMcpAdapter` request/parse | Fake stdio child (scripted JSON-RPC); one ignored real-`npx` test gated behind G2 |
| Unit | `GitPort` empty-repo path | temp git repo fixtures |
| Integration | poll loop → `spec_updated`; diff pipeline → `diff_updated`; approval long-poll resolves | `run_signal_loop`-style test with collected emits, no `AppHandle` |
| UI | SpecPane checklist render + approval gate; VibeLensPanel render; ipc listeners | vitest (`pnpm -C ui test`) |

## Migration / Rollout

No data migration. Doc reconciliation only: update data-flow.md write-trigger table to the
`spectty/{session_id}/spec|progress|cost` form (D5, done in spec phase) and the W1 `(Starting, Ready)`
fix (Slice 1). Feature lands behind the existing per-session pipeline — no flag needed.

## Slice → design mapping (each independently green)

1. **EngramAdapter + poll loop** — D26, D27, D28, D38 (hydrate), W1 doc fix. Green: `FakeEngramHttp` contract tests + `SpecBus` unit tests pass; G1 documented.
2. **Core SpecContract + spectty_spec effect** — D32, D33, D29 (`spec_updated`/`get_spec`). Green: pure entity tests + poll→event integration.
3. **Plan-approval gate + spectty_approval** — D31, D33 gate, `approve_prompt` cmd. Green: gate unit tests + approval long-poll integration.
4. **VibeLens ports + diff pipeline + spectty_diff** — D34, D35, D36, D37, D29 (`diff_updated`/`get_diff_explanation`). Green: GitPort/dedup unit + fake-stdio contract + pipeline integration; G2 documented.
5. **UI triad** — SpecPane + VibeLensPanel + layout + ipc.ts; minimal spectty_status/spectty_cost effect stubs. Green: vitest.

## Open Questions / Pre-Apply Gates

- [x] **G1 (blocks Slice 1 apply, R1) — VERIFIED 2026-06-11 against the running daemon on `:7437`:**
  - **Read path**: `GET /observations` → `200`, JSON array of objects with fields
    `{id, sync_id, session_id, type, title, content, project, scope, topic_key, revision_count,
    duplicate_count, last_seen_at, created_at, updated_at}`. The legacy `/api/observations` path
    from the old `todo!()` comment **404s** — the correct base is `/observations`.
  - **Change-detection field**: `updated_at`, a string like `"2026-06-11 03:07:16"` (space-separated,
    lexicographically monotonic → safe for `>` string comparison in the poll loop).
  - **`?topic_key=` query does NOT narrow server-side** (returns the full list); **`?since=` is NOT
    supported** (ignored). Therefore the adapter/poll loop **filters by `topic_key` client-side** and
    **change-detects by comparing `updated_at` strings** (D28 fetch-and-compare fallback — confirmed).
  - **topic_key is stored LOWERCASED** server-side (`sdd/m4-triad-spec-vibelens/...`). The adapter
    matches `topic_key` **case-insensitively** when scanning the returned list.
  - **Write path**: `POST /observations` with body
    `{session_id, topic_key, project, scope("project"|"personal"), content, type, title}`
    → `201 {id, status:"saved"}`. **Requires the `session_id` to already exist**: register it once via
    `POST /sessions {id, project}` (→ `201`, idempotent INSERT-OR-IGNORE; missing session → `400/404
    "session not found"`). Missing `session_id`/`title`/`content` → `400`.
  - **`EngramHttp` trait shape PINNED**: `Obs { content: String, updated_at: String }` (string ts,
    NOT `i64`); `get_observation(topic_key, _since: Option<&str>)` keeps the `since` param for
    forward-compat but the impl ignores it and compares `updated_at` strings. `ReqwestEngramHttp`
    ensures the session via `POST /sessions` before each `POST /observations`.
  - Build stays green behind `FakeEngramHttp` regardless; the one real-`:7437` contract test is kept
    `#[ignore]` (daemon-dependent, not run in CI) per WU-1.8.
- [ ] **G2 (blocks Slice 4 apply, R4)**: VERIFY `show_diff_explanation` param schema via `tools/list` against `npx -y vibelens-mcp` (transport already VERIFIED = stdio). Confirm field names (`diff`, `file_analysis`?) and response shape before un-ignoring the real-`npx` contract test. (DEFERRED to PR-4; not in PR-1 scope.)
- [x] **Tasks-phase check (RESOLVED)**: `async-trait` is NOT a Core dependency (`crates/core/Cargo.toml` runtime = `serde` + `thiserror` only). The new Git/DiffExplainer ports (PR-4) MUST use sync signatures (async bridged in adapters) to preserve R6 (no new Core dep). Confirmed in tasks.md.
