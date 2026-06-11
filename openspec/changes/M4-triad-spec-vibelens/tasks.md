# M4 — The Triad (Living Spec Pane + VibeLens + Why) — Task Checklist

> **STATUS: NOT STARTED.** sdd-tasks output; consumes `sdd/M4-triad-spec-vibelens/spec`
> (obs #870) + `openspec/changes/M4-triad-spec-vibelens/specs/*` (change-level `spec.md`
> + 13 per-capability deltas, 25 REQs M4-REQ-01..25) and `sdd/M4-triad-spec-vibelens/design`
> (obs #869) + `openspec/changes/M4-triad-spec-vibelens/design.md` (ADRs D26–D38).
> Artifact store: HYBRID (engram `sdd/M4-triad-spec-vibelens/tasks` + this file).
> Format follows the archived M3 tasks.md (work units → REQ tags + ADRs, RED→GREEN→REFACTOR,
> PR-slice mapping, per-WU gates).
>
> **Design is authoritative over spec where they differ.** Spec describes WHAT (the 25
> normative REQs); design fixes HOW (ADRs D26–D38). Tasks follow the DESIGN throughout.
>
> **Strict TDD is ACTIVE.** Test runners: `cargo test --workspace` (Rust) +
> `pnpm -C ui test` (= `vitest run`, TS). Every code work unit pairs its RED test with
> its GREEN implementation in the SAME unit: write the failing test first, make it pass,
> then refactor. Do NOT batch tests at the end.
>
> **Per-WU gate commands** (the M4 set — same as M3):
> - `cargo fmt --all -- --check`
> - `cargo clippy --workspace --all-targets -- -D warnings`
> - `cargo test --workspace`
> - `cargo deny --manifest-path crates/core/Cargo.toml check bans` — Core quarantine MUST
>   stay green at EVERY WU. The HARD invariant for M4: `crates/core` gains ONLY pure
>   `serde + thiserror` entities/ports. NO `serde_json`, `tokio`, `reqwest`, `notify`,
>   `git2`, `async-trait`, `tauri` in Core runtime deps. (See **Tasks-phase check** below.)
> - `pnpm -C ui test` (= `vitest run`) and `pnpm -C ui build` (UI WUs only).
> - VibeLens: after edits in a WU, call `show_diff_explanation` with that WU's
>   `git diff HEAD` (per project CLAUDE.md) — apply-phase obligation, not a commit.
>
> **Spec traceability tag** per task: `[M4-REQ-NN]` maps to a spec Requirement.
> Verification class carried from spec: `[unit]` / `[manual]` / `[ci]`.
> **Design tag** `[D#]` references the ADR that fixes the decision.
>
> **delivery_strategy: auto-chain. chain_strategy: stacked-to-main.** Each PR slice ≤400
> changed lines, independently green (tests + all gates pass at every PR HEAD). Dependency
> order is FIXED by the design: persistence → spec → approval → diff → UI.

---

## Tasks-phase check (RESOLVED) — `async-trait` is NOT a Core dependency

> Verified against `crates/core/Cargo.toml`: runtime `[dependencies]` = `serde` + `thiserror`
> ONLY (`serde_json` is dev-dep, test-only). `async-trait` is ABSENT.
>
> **DECISION (binds D35/D36):** the three new Core port traits (`GitPort`, `FileWatchPort`,
> `DiffExplainerPort`) MUST use **SYNC** signatures — `fn diff_head(&self, ws: &Path) ->
> Result<String, GitError>`, `fn explain(&self, diff: &str, ws: &Path) -> Result<DiffExplanation,
> ExplainError>`. The async (reqwest/stdio/notify) is bridged INSIDE the adapters via the
> dedicated-runtime `block_on` pattern already chosen for `EngramAdapter` (D26). Adding
> `async-trait` to Core would be a NEW Core dep and break `cargo deny check bans` (R6
> quarantine). The design's `#[async_trait]` sketch in §Interfaces is explicitly overridden
> by its own NOTE (design.md:148-151). This is non-negotiable for M4.

---

## W1 doc-only correction (RESOLVED in baseline) — verify-only, rides any PR

> **M4-REQ-24** requires `openspec/specs/agent-runner/spec.md`'s pipeline-augmentation Ready
> scenario to read `(Starting, Ready) => Idle` with no contradictory "Starting unchanged".
> **FINDING (verified this phase):** the baseline ALREADY satisfies this — `spec.md:235`
> reads "it MUST return `Idle` (the transition rule `(Starting, Ready) => Idle`)" and the
> M3 amendment note (`spec.md:228-230`) is consistent. The correction landed in M3 verify
> (commit `c707683 docs(m3): verify report + W1 spec scenario correction`).
>
> **M4 W1 task = CONFIRM-and-reconcile only** (see WU-1 task 1.5): re-assert the baseline
> matches REQ-24, and ensure the M4 per-capability `agent-runner.md` delta does not
> reintroduce a contradictory rule. Zero-risk, doc-only; rides Slice 1 (PR-1).

---

## Pre-Apply Gates (BLOCKING — see G1/G2 as explicit WUs below)

- **G1** (blocks Slice 1 apply, R1) — verify engram `:7437` REST surface against the RUNNING
  daemon. Captured as **WU-0** (must complete before WU-1 GREEN un-ignores the real test).
- **G2** (blocks Slice 4 apply, R4) — verify `show_diff_explanation` param schema via
  `tools/list` against the running VibeLens stdio server. Captured as **WU-9** (must complete
  before WU-10/11 un-ignore the real-`npx` test).

> Both slices are FULLY BUILDABLE behind fakes (`FakeEngramHttp`, fake stdio child) regardless
> of gate outcome — the gate only un-blocks the one `#[ignore]` real-endpoint contract test
> and pins the wire shapes. Slices stay green even if a gate is deferred.

---

## Dependency graph

```
WU-0 (G1: verify engram :7437) ──┐  (gate, no code)
                                 ▼
WU-1 (EngramHttp trait + EngramAdapter impl + FakeEngramHttp contract + W1 doc) ─┐  Slice 1 / PR-1
     │                                                                           │
     └── WU-2 (SpecBus poll loop: change-detect + injected emit) ───────────────┤  Slice 1 / PR-1
              │
              ▼
WU-3 (Core SpecContract/SpecTask/TaskState/ApprovalState + transition + gate) ──┐  Slice 2 / PR-2
     │   (PURE; depends only on Core; can dev in parallel with WU-1/2)          │
     └── WU-4 (spectty_spec effect + spec_updated event + get_spec + hydrate) ──┤  Slice 2 / PR-2
              │
              ▼
WU-5 (spectty_approval blocking long-poll resolver + approve_prompt cmd) ───────┐  Slice 3 / PR-3
     │   (needs WU-3 ApprovalState + WU-4 poll→status path)                     │
     └── (status_changed reuse — NO new event)                                  │
              │
              ▼
WU-6 (Core DiffExplanation + Session.last_diff/hash; 3 SYNC ports) ─────────────┐  Slice 4 / PR-4
     │   (PURE Core; can dev in parallel after WU-3 lands)                      │
WU-7 (GitPort + FileWatchPort adapters: git2/shell + notify debounced) ─────────┤  Slice 4 / PR-4
WU-8 (VibeLensMcpAdapter stdio child + diff pipeline arbitration + spectty_diff)─┤  Slice 4 / PR-4
WU-9 (G2: verify show_diff_explanation schema) ─────────────────────────────────┘  (gate, before WU-8 real test)
              │
              ▼
WU-10 (UI triad: ipc.ts + SpecPane + VibeLensPanel + TriadLayout + vitest) ──────┐  UI triad slice / PR-6
              │
              ▼
WU-11 (manual acceptance — 8 roadmap exit criteria + ADR D26-D38 note) ──────────┘  UI triad slice / PR-6 (verify/doc)
```

---

## WU-0 — Pre-Apply Gate G1: verify engram :7437 REST surface  [manual][ci]
**Commit**: `docs(m4): record verified engram :7437 REST surface (G1)`
**Depends on**: nothing (runs against the locally-running engram daemon).
**Blocks**: WU-1 GREEN (un-ignoring the real-`:7437` contract test); does NOT block WU-1
fake-backed build (`FakeEngramHttp`).
**Rollback**: n/a (verification + doc artifact only).
**PR slice**: PR-1 (rides Slice 1; ~0 code lines, findings recorded in design.md/this file).

> R1 OPEN. The design (D26/D28) and `engram.rs` disagree on the REST path
> (`/observations` per research doc vs `/api/observations` per the `todo!()` comment).
> `?since=` support and the change-detection field (`updated_at`) are UNCONFIRMED against a
> live daemon. The `EngramHttp` trait isolates this — pin the shapes here BEFORE the real test.

- [x] 0.1 Hit the running engram daemon on `:7437`. Confirm the EXACT upsert path
  (`POST /observations` vs `/api/observations`) and the exact GET/read path.
  `[M4-REQ-02]` `[manual]`
- [x] 0.2 Capture the upsert request JSON shape — confirm it carries `topic_key`, `project`,
  `scope` (`project|personal`), `content`; record the response envelope. `[M4-REQ-02]` `[manual]`
- [x] 0.3 Confirm whether GET supports a `?since=<ts>` / `?topic_key=...` filter, and which
  response field carries the change-detection timestamp (`updated_at`). If `?since=` is
  unsupported, the poll loop falls back to fetch-and-compare on `updated_at` (D28).
  `[M4-REQ-02][M4-REQ-03]` `[manual]`
- [x] 0.4 Record findings in `design.md` §Pre-Apply Gates G1 (check the box) and adjust the
  `EngramHttp` trait method signatures in WU-1 if the verified shapes require it (e.g.
  `since: Option<i64>` param, response struct field names). `[ci]`
- [x] **Gate (WU-0)**: G1 findings documented; `EngramHttp` trait shape pinned. No build/test
  gate (doc-only). If the daemon is unreachable, mark G1 deferred and keep WU-1's real test
  `#[ignore]` — the slice still ships green on `FakeEngramHttp`.

---

## WU-1 — `EngramHttp` trait + `EngramAdapter` impl + `FakeEngramHttp` contract + W1 doc  [unit][ci]
**Commit**: `feat(adapters): implement EngramAdapter over EngramHttp trait with degrade-when-down (D26)`
**Depends on**: WU-0 (G1 shapes pinned; can build on the provisional shape if G1 deferred).
**Blocks**: WU-2 (SpecBus needs a working `PersistencePort` to poll).
**Strict TDD**: RED the `FakeEngramHttp` contract tests FIRST (upsert→get round-trip; absent
key → `Ok(None)`; transport error → `PersistenceError::Backend`, never panic).
**Rollback**: revert → `engram.rs` back to `todo!()`; M3 still builds.
**PR slice**: PR-1 (Slice 1).

> D26: split reqwest behind a private `trait EngramHttp { fn post_observation(..); fn
> get_observation(topic_key, since) -> Option<Obs>; }`. `EngramAdapter` (the `PersistencePort`
> impl) owns `Arc<dyn EngramHttp>` + a DEDICATED Tokio runtime handle and `block_on`-bridges
> the sync port → async reqwest (NOT the Tauri main runtime). Contract-test against an
> in-memory `FakeEngramHttp` now; swap verified shapes later without touching the port.

- [x] 1.1 RED: `engram_adapter_upsert_then_get_round_trips` — `EngramAdapter` over an in-memory
  `FakeEngramHttp`; `upsert("spectty/s/spec", payload)` then `get("spectty/s/spec")` returns
  `Ok(Some(payload))`. RED proven before the impl exists. `[M4-REQ-02]` `[unit][D26]`
- [x] 1.2 RED: `engram_adapter_get_absent_key_returns_ok_none` — `get` on an unknown topic_key
  → `Ok(None)` (missing key is NOT an error). `[M4-REQ-02]` `[unit][D26]`
- [x] 1.3 RED: `engram_adapter_degrades_when_backend_down` — `FakeEngramHttp` scripted to return
  a transport error → both `upsert` and `get` return `Err(PersistenceError::Backend(_))`,
  NEVER panic / unwrap. `[M4-REQ-02]` `[unit][D26]`
- [x] 1.4 RED: `engram_adapter_implements_persistence_port_unchanged` — compile-time:
  `fn takes_port(_: &dyn PersistencePort) {}; takes_port(&adapter)`. Pins M4-REQ-01: port
  signature UNCHANGED (sync, `&self`, `String`/`Option<String>`, no async/subscribe/serde_json).
  `[M4-REQ-01]` `[unit][ci][D27]`
- [x] 1.5 W1 verify-only: re-read `openspec/specs/agent-runner/spec.md` — confirm the
  pipeline-augmentation Ready scenario reads `(Starting, Ready) => Idle` with no contradictory
  "Starting unchanged"; confirm the M4 per-capability `agent-runner.md` delta is consistent.
  Baseline already correct (M3 verify); reconcile the delta if it drifted. Doc-only, zero code.
  `[M4-REQ-24]` `[ci]`
- [x] 1.6 GREEN: create `crates/adapters/src/persistence/engram_http.rs` — private
  `pub(crate) trait EngramHttp: Send + Sync { fn post_observation(&self, topic_key: &str,
  payload: &str) -> Result<(), EngramHttpError>; fn get_observation(&self, topic_key: &str,
  since: Option<&str>) -> Result<Option<Obs>, EngramHttpError>; }` + `struct Obs { content:
  String, updated_at: String }` + the in-memory `FakeEngramHttp` (test module). Field names AND
  types per G1 (WU-0): engram returns `updated_at` as a `"YYYY-MM-DD HH:MM:SS"` STRING (not an
  `i64`), so the change-detect feed is a `String` and `since` is `Option<&str>`. `[M4-REQ-02]` `[unit][D26]`
- [x] 1.7 GREEN: rewrite `crates/adapters/src/persistence/engram.rs` — `EngramAdapter { http:
  Arc<dyn EngramHttp>, rt: tokio::runtime::Handle }`; `upsert`/`get` `block_on` the async
  reqwest impl behind `EngramHttp`; map transport failure → `PersistenceError::Backend`. Add
  `ReqwestEngramHttp` concrete impl (reqwest, `:7437`, path from G1). reqwest/tokio live in
  adapters ONLY — Core untouched. `[M4-REQ-02]` `[unit][D26]`
- [x] 1.8 GREEN: add ONE `#[ignore]` `engram_adapter_real_7437_contract` test hitting the live
  daemon (un-ignore only after G1/WU-0 pins the shapes). `[M4-REQ-02]` `[manual][D26]`
- [x] **Gate (WU-1)**: `cargo test --workspace` green (4 fake-backed unit tests; real test
  `#[ignore]`d); fmt clean; clippy `-D warnings`; `cargo deny --manifest-path crates/core/Cargo.toml
  check bans` → `bans ok` (Core gained NOTHING — M4-REQ-01).

---

## WU-2 — `SpecBus` poll loop: change-detect (updated_at monotonic) + injected emit  [unit]
**Commit**: `feat(tauri): add SpecBus poll loop with updated_at change detection (D27/D28)`
**Depends on**: WU-1 (`PersistencePort`/`EngramAdapter` to poll).
**Blocks**: WU-4 (spec_updated emission rides this loop), WU-5 (approval pending seen via poll).
**Strict TDD**: RED the change-detection contract FIRST — Fake `PersistencePort` returning
scripted payloads + injected `emit` closure (mirrors `observe_and_diff` / M3 `run_signal_loop`).
**Rollback**: revert → no poll loop; M3 signal loop unchanged.
**PR slice**: PR-1 (Slice 1).

> D27/D28: poll/subscribe is an ADAPTER-side `SpecBus` struct (NOT a port method). Holds
> `Arc<dyn PersistencePort>`, polls per topic_key on `tokio::time::interval(2s)` (env
> `SPECTTY_POLL_MS`), compares observation `updated_at` vs per-topic `last_updated_at`, emits
> via injected `FnMut` closure ONLY on strictly-greater. SAME seam shape as `run_signal_loop`.
> `serde_json` deserialization stays adapter-side. The loop owns no `AppHandle` (testable).

- [x] 2.1 RED: `spec_bus_emits_once_on_first_change` — Fake `PersistencePort` returns a payload
  with `updated_at=1`; `last_updated_at=None` → poll invokes `emit(Change)` EXACTLY ONCE,
  advances `last_updated_at` to 1. `[M4-REQ-03]` `[unit][D28]`
- [x] 2.2 RED: `spec_bus_does_not_re_emit_same_updated_at` — poll twice with `updated_at=1`;
  second tick → NO emit. `[M4-REQ-03]` `[unit][D28]`
- [x] 2.3 RED: `spec_bus_re_emits_on_newer_updated_at` — after consuming `updated_at=1`, next
  payload has `updated_at=2` → emit once more, advance to 2. `[M4-REQ-03]` `[unit][D28]`
- [x] 2.4 RED: `spec_bus_tolerates_poll_error` — Fake port returns `Err(Backend)` on a tick →
  loop logs + continues, does NOT emit, does NOT panic; next good tick resumes. `[M4-REQ-03]`
  `[unit][D28]`
- [x] 2.5 RED: `spec_bus_tolerates_absent_observation` — port returns `Ok(None)` (key not yet
  written) → no emit, no error. `[M4-REQ-03]` `[unit]`
- [x] 2.6 GREEN: create `src-tauri/src/spec_bus.rs`: `pub struct SpecBus { reader: Arc<dyn
  PollReader>, topic_key: String, last_updated_at: Option<String> }` (per G1 `updated_at` is a
  STRING, not `i64`) with `pub fn poll(&mut self, emit: &mut dyn FnMut(Change))` (pure-testable;
  injected emit) AND `pub async fn run_poll_loop(bus, interval, shutdown, emit)` — the async/Tokio
  seam (M4-REQ-03): `tokio::time::interval(poll_interval())`, a `watch`-based shutdown signal
  (graceful session-close / app-shutdown, mirrors `run_signal_loop`), and `spawn_blocking` around
  the sync poll step so the blocking `ReqwestEngramHttp` reader never runs on a runtime worker.
  Port-only fallback (`PortPollReader`) change-detects by EQUALITY (a content hash is NOT
  monotonic — see Finding-1 fix), emitting a synthetic monotonic counter so `decide()`'s strict
  `>` compare stays valid. Deserialize `String → SpecContract` adapter-side via `serde_json`
  (WU-4). `[M4-REQ-03]` `[unit][D27][D28]`
- [x] **Gate (WU-2)**: `cargo test --workspace` green (5 SpecBus unit tests); fmt/clippy clean;
  `cargo deny ... check bans` → `bans ok`.

> **Slice 1 COMPLETE after WU-2.** PR-1 = WU-0 (gate doc) + WU-1 + WU-2 + W1 doc fix.
> Green floor: `FakeEngramHttp` contract + `SpecBus` change-detection unit tests; G1 documented.

---

## WU-3 — Core entities: `SpecContract` / `SpecTask` / `TaskState` / `ApprovalState` + transitions + gate  [unit][ci]
**Commit**: `feat(core): add SpecContract entities with one-way TaskState transitions and approval gate (D32/D33)`
**Depends on**: nothing in Core (PURE; can dev in parallel with WU-1/2). Logically opens Slice 2.
**Blocks**: WU-4 (spec effect (de)serializes these), WU-5 (`ApprovalState`).
**Strict TDD**: RED `TaskState::transition` legal/illegal table + `may_begin_edits` gate +
`apply_progress` gate-error FIRST. Pure unit tests, no I/O. Mirrors `AgentStatus::transition`.
**Rollback**: revert → no spec entities; nothing imports them.
**PR slice**: PR-2 (Slice 2).

> D32/D33, ADR-0007. NEW `crates/core/src/entities/spec.rs`. `serde + thiserror` ONLY — no I/O,
> no `serde_json` runtime dep (round-trip tests use the dev-dep). This is the testable
> invariant surface: legal transitions + gate-before-edit become pure unit tests.

- [x] 3.1 RED: `task_state_transition_legal_table` — `Pending→InProgress→Done`,
  `Pending→InProgress→Skipped` succeed; `Done` is TERMINAL (any move out → `Err`); backward
  (`InProgress→Pending`, `Done→InProgress`) → `Err(SpecError)`; illegal jump
  (`Pending→Done`) → `Err`. RED proven by swapping a legal pair. `[M4-REQ-05]` `[unit][D32]`
- [x] 3.2 RED: `approval_state_default_is_pending` — a freshly submitted plan starts
  `ApprovalState::Pending`; variants `Pending/Approved/Rejected/Adjusted` exist + serde round-trip.
  `[M4-REQ-06]` `[unit][D32]`
- [x] 3.3 RED: `spec_contract_serde_round_trips` — `SpecContract { intent, proposal, tasks,
  progress, approval, steering_notes }` survives serialize → deserialize byte-stable (pure).
  `[M4-REQ-04]` `[unit][D32]`
- [x] 3.4 RED: `may_begin_edits_true_only_when_approved` — `may_begin_edits()` returns `true`
  ONLY when `approval == Approved`; `Pending/Rejected/Adjusted` → `false`. Dev-override
  constructor flag is representable, NOT the default, and distinguishable from a real approval.
  `[M4-REQ-07]` `[unit][D33]`
- [x] 3.5 RED: `apply_progress_blocks_in_progress_while_pending` — `apply_progress(task_id,
  InProgress)` while `approval == Pending` → `Err(SpecError::GateNotApproved)`; same call after
  `Approved` → `Ok`. `[M4-REQ-07]` `[unit][D33]`
- [x] 3.6 GREEN: create `crates/core/src/entities/spec.rs` — `enum TaskState { Pending,
  InProgress, Done, Skipped }` + `fn transition(self, to: TaskState) -> Result<TaskState,
  SpecError>` (one-way); `enum ApprovalState { Pending, Approved, Rejected, Adjusted }`;
  `struct SpecTask { id, title, state: TaskState }`; `struct SpecContract { intent: String,
  proposal: Option<String>, tasks: Vec<SpecTask>, progress: Vec<TaskProgress>, approval:
  ApprovalState, steering_notes: Vec<String>, dev_override: bool }`; `fn may_begin_edits(&self)
  -> bool`; `fn apply_progress(&mut self, task_id: &str, to: TaskState) -> Result<(), SpecError>`;
  `enum SpecError` (thiserror, incl. `GateNotApproved`). `serde + thiserror` ONLY.
  `[M4-REQ-04][M4-REQ-05][M4-REQ-06][M4-REQ-07]` `[unit][ci][D32][D33]`
- [x] 3.7 GREEN: export from `crates/core/src/entities/mod.rs` + re-export at Core `lib.rs`. `[ci]`
- [x] **Gate (WU-3)**: `cargo test --workspace` green (5 pure entity tests); fmt/clippy clean;
  `cargo deny --manifest-path crates/core/Cargo.toml check bans` → `bans ok` (Core gained ONLY
  `serde + thiserror` types — M4-REQ-04/ci). **This is the load-bearing Core-quarantine WU.**

---

## WU-4 — `spectty_spec` effect + `spec_updated` event + `get_spec` cmd + restart hydrate  [unit]
**Commit**: `feat(mcp,tauri): give spectty_spec a real engram-upsert effect, emit spec_updated, hydrate on re-attach (D29/D38)`
**Depends on**: WU-2 (SpecBus poll) + WU-3 (`SpecContract`).
**Blocks**: WU-5 (approval surfaces through the same poll→status path).
**Strict TDD**: RED the FROZEN-schema assertion + poll→`spec_updated` integration (collected
emits, no `AppHandle`) + restart-hydrate FIRST.
**Rollback**: revert → `spectty_spec` returns a stub; SpecBus loop runs but emits nothing.
**PR slice**: PR-2 (Slice 2).

> D16/D29/D38. `spectty-mcp` upserts to `spectty/{session_id}/spec` and returns IMMEDIATELY
> (gains an engram HTTP client — serde+http only, NO core/tauri). The app's SpecBus poll sees
> the change → emits `spec_updated { session_id, spec: SpecContract }`. `get_spec(id)` reads
> on demand. On spawn/re-attach: ONE `get(spectty/{sid}/spec)` + `get(.../progress)` BEFORE the
> poll interval → emit initial `spec_updated` (UI restores instantly, exit criterion 6).
>
> PERF NOTE (carry-in from PR-1 review, Finding 5): `EngramHttp::ensure_session` currently
> POSTs `/sessions` on EVERY upsert (correct + idempotent, INSERT-OR-IGNORE). Before the 2s
> production poll/effect loop goes live, MEMOIZE the already-ensured session ids (a
> `OnceCell`/seen-set keyed by `session_id`) so each session row is created at most once per
> process and the loop does not double write traffic. Also reconcile the D5 fallback: once real
> session ids are wired, the `"spectty"` stopgap in `engram_session_id` should be unreachable for
> canonical `spectty/{sid}/{spec|progress|cost}` keys (a `debug_assert` + test already pin this).

- [x] 4.1 RED: `spectty_mcp_tools_list_schema_is_byte_frozen` — assert `tools/list` output for
  the 5 tools is byte-for-byte identical to the M3-frozen schema fixture; only `tools/call`
  effects change. RED proven by mutating a description. `[M4-REQ-08]` `[unit][ci][D16]`
- [x] 4.2 RED: `spectty_spec_upserts_canonical_key_and_returns_immediately` — fake engram HTTP
  client; `spectty_spec` payload → upsert to `spectty/{session_id}/spec`, returns without
  blocking. Malformed payload → rejected, no crash. `[M4-REQ-09]` `[unit][D5]`
- [x] 4.3 RED: `poll_change_emits_spec_updated_once` — integration: SpecBus over Fake port
  scripted with one spec change → collected emits contain EXACTLY ONE `spec_updated` with the
  deserialized `SpecContract`. `[M4-REQ-09][M4-REQ-17]` `[unit][D29]`
- [x] 4.4 RED: `restart_hydrate_emits_initial_spec_updated` — re-attach path does ONE
  `get(spectty/{sid}/spec)` (+ `.../progress`), reconstructs `SpecContract`, emits initial
  `spec_updated`; engram-down → degrades to empty/last-known, NO crash. `[M4-REQ-23]`
  `[unit][D38]`
- [x] 4.5 GREEN: extend `crates/spectty-mcp/src/main.rs` — `spectty_spec` `tools/call` handler
  builds the `SpecContract` JSON and POST-upserts via a new serde+http engram client to
  `spectty/{session_id}/spec`; returns immediately. Schema (`tools/list`) UNTOUCHED.
  `[M4-REQ-08][M4-REQ-09]` `[unit][D16]`
- [x] 4.6 GREEN: create `src-tauri/src/commands/spec.rs` — `get_spec(id) -> Option<SpecContract>`
  command; register in `generate_handler!`. Add `spec_updated` event emission inside the SpecBus
  injected-emit closure (v2 `Emitter`), emit ONLY on actual change. `[M4-REQ-16][M4-REQ-17]`
  `[unit][D29]`
- [x] 4.7 GREEN: wire restart hydrate in `src-tauri/src/commands/session.rs` — on spawn/re-attach,
  before starting the poll interval, ONE `get` per spec/progress key → emit initial
  `spec_updated`. `[M4-REQ-23]` `[unit][D38]`
- [x] **Gate (WU-4)**: `cargo test --workspace` green (frozen-schema + 3 integration/effect tests);
  fmt/clippy clean; `cargo deny ... check bans` → `bans ok` (spectty-mcp stays serde+http only).

> **Slice 2 COMPLETE after WU-4.** PR-2 = WU-3 + WU-4. Green: pure entity unit tests +
> poll→`spec_updated` integration + restart hydrate. Exit criteria 1 (partial: seed/plan), 3, 6.

---

## WU-5 — `spectty_approval` blocking resolver (engram long-poll) + `approve_prompt` cmd  [unit]
**Commit**: `feat(mcp,tauri): add spectty_approval engram round-trip resolver and approve_prompt command (D31/D33)`
**Depends on**: WU-3 (`ApprovalState`) + WU-4 (poll→status path).
**Blocks**: nothing downstream in Rust; UI consumes it in WU-10.
**Strict TDD**: RED the pending-registration + idempotency + resolution-observable + unknown-key
no-op FIRST (fake engram round-trip; bounded long-poll).
**Rollback**: revert → `spectty_approval` returns a stub; no approval gate.
**PR slice**: PR-3 (Slice 3 — isolated by design).

> D31/D33/D29. ONE blocking tool. `spectty_approval` upserts the request to
> `spectty/{session_id}/approval` then LONG-POLLS `get` on the same key (~500ms, bounded) for a
> resolution written back. App poll sees pending → emits the EXISTING `status_changed(AwaitingInput,
> quick_actions)` (NO new approval event — reuse M2 path). `approve_prompt(session_id, action_id,
> decision)` writes the decision into `ApprovalState` + upserts the resolved payload; the MCP
> long-poll reads it → returns to the agent. Restart-survivable; spectty-mcp stays serde+http.

- [x] 5.1 RED: `spectty_approval_registers_pending_and_builds_quick_actions` — handler upserts a
  pending request keyed `(session_id, action_id)`; the app poll path maps it to
  `AwaitingInput + quick_actions` derived from `options[]`. `[M4-REQ-10]` `[unit][D31]`
  > Implemented as `spectty_approval_registers_pending_with_options` (MCP, asserts the upserted
  > pending doc carries `action_id`/`options`/null `resolution`). The `options→quick_actions`
  > status-path mapping is the UI/poll concern landing in PR-6/WU-10; PR-3 pins the persisted
  > shape `quick_actions` derive from.
- [x] 5.2 RED: `spectty_approval_duplicate_request_is_idempotent` — same `(session_id, action_id)`
  upserted twice → single pending entry, no duplicate. `[M4-REQ-10]` `[unit][D31]`
- [x] 5.3 RED: `approve_prompt_resolves_and_unblocks_caller` — fake round-trip: `approve_prompt`
  writes `ApprovalState::Approved` + resolved payload; the blocked long-poll observes the
  resolution and returns; pending entry removed. `[M4-REQ-11]` `[unit][D31]`
  > Tauri `approve_prompt_resolves_and_unblocks_caller` (resolution observable on same key) +
  > MCP `spectty_approval_long_poll_returns_resolution` (blocked long-poll reads it, returns
  > decision to agent).
- [x] 5.4 RED: `approve_prompt_unknown_key_is_no_op` — resolving an unknown `(session_id,
  action_id)` → no-op, no error/panic. `[M4-REQ-11]` `[unit][D31]`
  > Plus `approve_prompt_already_resolved_is_no_op` (a stale/duplicate decision cannot clobber
  > a resolved request).
- [x] 5.5 RED: `approve_prompt_writes_approval_state_via_core_gate` — decision flows through
  `SpecContract`/`ApprovalState` (Core rule, never reimplemented). `[M4-REQ-07]` `[unit][D33]`
- [x] 5.6 GREEN: extend `crates/spectty-mcp/src/main.rs` — `spectty_approval` handler upserts to
  `spectty/{session_id}/approval` then bounded long-polls `get` (~500ms interval) for the
  resolution; returns the decision to the agent. serde+http only. `[M4-REQ-10][M4-REQ-11]`
  `[unit][D31]`
  > `EngramClient` gained a `get`; `poll_for_resolution` (bounded by `SPECTTY_APPROVAL_MAX_POLLS`,
  > interval `SPECTTY_APPROVAL_POLL_MS`) returns a `pending`/timeout result rather than hanging.
  > Malformed payload → `-32602`; engram-down → benign `isError` degrade. `ReqwestEngramClient::get`
  > mirrors the G1 client-side topic_key filter.
- [x] 5.7 GREEN: add `approve_prompt(session_id, action_id, decision)` command to
  `src-tauri/src/commands/spec.rs` — writes `ApprovalState` via the Core gate, upserts the
  resolved payload to the same key; register in `generate_handler!`. App poll maps pending →
  EXISTING `status_changed(AwaitingInput, quick_actions)` (no new event). `[M4-REQ-10][M4-REQ-11]`
  `[unit][D29][D31]`
  > `ApprovalRequest` document + `resolve_approval_impl` (decision via Core `ApprovalState`,
  > unknown/resolved key = no-op) + `ApprovalDecision` UI enum mapped onto `ApprovalState`.
  > Registered in `lib.rs` `generate_handler!`. The pending→`status_changed` wiring is a UI/poll
  > concern deferred to PR-6/WU-10 (the persisted pending shape it consumes is pinned here).
- [x] **Gate (WU-5)**: `cargo test --workspace` green (5 approval tests incl. long-poll resolve
  integration); fmt/clippy clean; `cargo deny ... check bans` → `bans ok`.
  > Tauri spec module +5 tests (76 lib total); MCP +7 tests (24 total). core 47 unchanged
  > (Core quarantine intact — reused existing `ApprovalState`, no new Core dep). fmt clean,
  > clippy -D warnings exit 0, deny bans ok, pnpm -C ui test 64 pass.

> **Slice 3 COMPLETE after WU-5.** PR-3 = WU-5 (isolated). Green: gate unit tests + approval
> long-poll resolve integration. Exit criterion 2 (plan-approval gate).

---

## WU-6 — Core `DiffExplanation` + `Session.last_diff/last_diff_hash` + 3 SYNC port traits  [unit][ci]
**Commit**: `feat(core): add DiffExplanation, Session diff dedup state, and Git/FileWatch/DiffExplainer SYNC ports (D34/D35)`
**Depends on**: WU-3 landed (Core entities module exists). PURE; opens Slice 4.
**Blocks**: WU-7 (GitPort/FileWatchPort adapters) + WU-8 (DiffExplainerPort adapter + pipeline).
**Strict TDD**: RED `DiffExplanation::empty()` + hash-dedup invariant FIRST. Pure, std-only hash.
**Rollback**: revert → no diff entities/ports; nothing imports them.
**PR slice**: PR-4 (Slice 4).

> D34/D35. NEW `crates/core/src/entities/diff.rs` + 3 NEW `crates/core/src/ports/*.rs`.
> **Per the Tasks-phase check, the ports are SYNC** (no `async-trait` in Core): `GitPort::diff_head
> (&self, ws: &Path) -> Result<String, GitError>`; `DiffExplainerPort::explain(&self, diff: &str,
> ws: &Path) -> Result<DiffExplanation, ExplainError>`; `FileWatchPort` subscribe → debounced
> `FileChanged` batches. Zero new Core deps. Hash = `std::collections::hash_map::DefaultHasher`
> over the diff string (std only).

- [x] 6.1 RED: `diff_explanation_empty_is_well_formed` — `DiffExplanation::empty()` → empty
  `files`, empty `summary`; serde round-trips. `[M4-REQ-12]` `[unit][D34]`
- [x] 6.2 RED: `session_update_diff_stores_hash` — `Session::update_diff(expl, hash)` sets
  `last_diff` + `last_diff_hash`; same-hash detection is observable on the aggregate. `[M4-REQ-13]`
  `[unit][D34]`
- [x] 6.3 RED: `core_ports_are_object_safe_and_sync` — compile-time: `fn _g(_: &dyn GitPort){}`,
  `fn _e(_: &dyn DiffExplainerPort){}`, `fn _w(_: &dyn FileWatchPort){}` — confirms SYNC,
  `Send + Sync`, object-safe, no `async-trait`. `[M4-REQ-12]` `[unit][ci][D35]`
- [x] 6.4 GREEN: create `crates/core/src/entities/diff.rs` — `struct DiffExplanation { files:
  Vec<FileExplanation>, summary: String }` + `struct FileExplanation { path: String, rationale:
  String }` + `fn empty() -> Self`. `serde + thiserror`. `[M4-REQ-12]` `[unit][D34]`
- [x] 6.5 GREEN: modify `crates/core/src/entities/session.rs` — add `last_diff:
  Option<DiffExplanation>`, `last_diff_hash: Option<u64>`, `fn update_diff(&mut self, expl:
  DiffExplanation, hash: u64)`. `[M4-REQ-13]` `[unit][D34]`
- [x] 6.6 GREEN: create `crates/core/src/ports/git.rs` (`GitPort::diff_head` + `enum GitError`),
  `crates/core/src/ports/file_watch.rs` (`FileWatchPort`), `crates/core/src/ports/diff_explainer.rs`
  (`DiffExplainerPort::explain` + `enum ExplainError`). ALL SYNC. Export from
  `crates/core/src/ports/mod.rs`. `[M4-REQ-12]` `[unit][ci][D35]`
- [x] **Gate (WU-6)**: `cargo test --workspace` green (3 pure tests); fmt/clippy clean;
  `cargo deny --manifest-path crates/core/Cargo.toml check bans` → `bans ok` (Core gained ONLY
  serde+thiserror types/traits, NO `async-trait`/`notify`/`git2` — M4-REQ-12/ci). **Load-bearing
  Core-quarantine WU.**

---

## WU-7 — `Git2Adapter` (GitPort) + `NotifyFileWatcher` (FileWatchPort) adapters  [unit]
**Commit**: `feat(adapters): add Git2Adapter (empty-repo aware) and NotifyFileWatcher debounced (D35)`
**Depends on**: WU-6 (port traits). **Can dev in parallel with WU-8 (DiffExplainer adapter).**
**Blocks**: WU-8 (pipeline wires both).
**Strict TDD**: RED the empty-repo `diff_head` (diff vs empty tree) using temp git fixtures FIRST.
**Rollback**: revert → no git/file-watch adapters; ports stand alone.
**PR slice**: PR-4 (Slice 4).

> D35. `git2` (or shell-git) + `notify` live in ADAPTERS only. `GitPort::diff_head` handles
> empty-repo by diffing against the empty tree. `NotifyFileWatcher` yields debounced (500ms–1s)
> `FileChanged` batches. Async bridged adapter-side (sync port signature per Tasks-phase check).

- [ ] 7.1 RED: `git_adapter_diff_head_on_populated_repo` — temp git repo with a staged change →
  `diff_head` returns the unified diff. `[M4-REQ-12]` `[unit][D35]`
- [ ] 7.2 RED: `git_adapter_diff_head_empty_repo_uses_empty_tree` — temp repo with NO commits →
  `diff_head` diffs against the empty tree (no error, returns the add-all diff). `[M4-REQ-13]`
  `[unit][D35]`
- [ ] 7.3 RED: `git_adapter_truly_empty_workspace_returns_empty_string` — empty working tree, no
  changes → empty diff string (pipeline maps this to `DiffExplanation::empty()` in WU-8).
  `[M4-REQ-13]` `[unit][D35]`
- [ ] 7.4 RED: `notify_file_watcher_debounces_burst_into_one_batch` — fake/synthetic event burst
  → ONE debounced `FileChanged` batch within the window. `[M4-REQ-15]` `[unit][D35]`
- [ ] 7.5 GREEN: create `crates/adapters/src/git/mod.rs` — `Git2Adapter` impl `GitPort`
  (git2 or `std::process::Command` git), empty-repo handling. Add `git2` (or none if shell) to
  ADAPTERS Cargo.toml only. `[M4-REQ-12]` `[unit][D35]`
- [ ] 7.6 GREEN: create `crates/adapters/src/file_watch/mod.rs` — `NotifyFileWatcher` impl
  `FileWatchPort` (notify, debounced). Add `notify` to ADAPTERS Cargo.toml only. `[M4-REQ-15]`
  `[unit][D35]`
- [ ] **Gate (WU-7)**: `cargo test --workspace` green (4 adapter tests); fmt/clippy clean;
  `cargo deny --manifest-path crates/core/Cargo.toml check bans` → `bans ok` (git2/notify in
  adapters, NOT Core).

---

## WU-8 — `VibeLensMcpAdapter` stdio + diff pipeline arbitration + `spectty_diff` + `diff_updated`  [unit]
**Commit**: `feat(adapters,tauri): add VibeLensMcpAdapter stdio client and diff pipeline with cooperative/generic arbitration (D36/D37)`
**Depends on**: WU-6 (ports) + WU-7 (Git/FileWatch adapters) + WU-9 (G2 schema, for the real test).
**Blocks**: WU-10 (UI consumes `diff_updated`/`get_diff_explanation`).
**Strict TDD**: RED hash-dedup pipeline + degrade-on-failure + fake-stdio JSON-RPC contract FIRST.
**Rollback**: revert → no VibeLens/pipeline; ports + git/watch adapters stand alone.
**PR slice**: PR-4 (Slice 4).

> D36/D37/D29. `VibeLensMcpAdapter` spawns `npx -y vibelens-mcp` as a stdio child (newline-delimited
> JSON-RPC 2.0, same framing as spectty-mcp), calls `show_diff_explanation { diff, file_analysis }`
> (field names per G2/WU-9), parses → `DiffExplanation`; manages subprocess lifecycle (lazy spawn,
> reuse, restart on crash). Pipeline: `(FileWatch debounced) OR (spectty_diff signal)` →
> `GitPort::diff_head` → hash==`last_diff_hash`? skip : `explain` → `Session::update_diff` →
> emit `diff_updated`. Cooperative `spectty_diff` bypasses debounce; FileWatch is the generic
> fallback (`emits_diff_signals==false`). Shared in-flight guard; hash-dedup makes double-fire safe.

- [ ] 8.1 RED: `vibelens_adapter_parses_show_diff_explanation_response` — fake stdio child scripted
  with a JSON-RPC `show_diff_explanation` response → parsed into `DiffExplanation { files, summary }`.
  `[M4-REQ-12]` `[unit][D36]`
- [ ] 8.2 RED: `vibelens_adapter_degrades_on_unreachable_or_parse_fail` — child unreachable /
  timeout / error / unparseable response → log + return a degraded marker ("unavailable" /
  "parse error"), retain previous, NEVER crash. `[M4-REQ-14]` `[unit][D36]`
- [ ] 8.3 RED: `pipeline_skips_explain_when_hash_unchanged` — same diff (same hash as
  `last_diff_hash`) → NO `explain` call, NO `diff_updated` emit. `[M4-REQ-13]` `[unit][D37]`
- [ ] 8.4 RED: `pipeline_explains_and_emits_once_on_change` — changed diff → `explain` called once,
  `Session::update_diff`, EXACTLY ONE `diff_updated { session_id, explanation }`. `[M4-REQ-13][M4-REQ-17]`
  `[unit][D37]`
- [ ] 8.5 RED: `pipeline_truly_empty_diff_is_empty_no_mcp_call` — empty diff string →
  `DiffExplanation::empty()`, NO MCP call. `[M4-REQ-13]` `[unit][D37]`
- [ ] 8.6 RED: `pipeline_degrades_on_git_failure` — `GitPort::diff_head` errors → log + retain
  previous, no crash. `[M4-REQ-14]` `[unit][D37]`
- [ ] 8.7 RED: `spectty_diff_cooperative_bypasses_debounce_generic_falls_back` — cooperative
  `spectty_diff` signal fires the pipeline immediately (no debounce wait); generic tier
  (`emits_diff_signals==false`) uses debounced FileWatch; SAME downstream pipeline; in-flight
  guard prevents double-fire. `[M4-REQ-15]` `[unit][D37]`
- [ ] 8.8 GREEN: create `crates/adapters/src/diff/vibelens.rs` — `VibeLensMcpAdapter` impl
  `DiffExplainerPort` (stdio child, JSON-RPC, lifecycle, degrade). `[M4-REQ-12][M4-REQ-14]`
  `[unit][D36]`
- [ ] 8.9 GREEN: add the diff pipeline to `src-tauri/src/session_runtime.rs` (alongside SpecBus /
  `run_signal_loop`, same injected-emit discipline) + wire per-session FileWatch + `spectty_diff`
  trigger in `commands/session.rs`; add `get_diff_explanation(id) -> Option<DiffExplanation>` to
  `commands/spec.rs`; emit `diff_updated` via v2 `Emitter` only on actual change. `[M4-REQ-15][M4-REQ-16][M4-REQ-17]`
  `[unit][D37][D29]`
- [ ] 8.10 GREEN: extend `crates/spectty-mcp/src/main.rs` — `spectty_diff` `tools/call` effect
  fires the cooperative trigger (signal upsert/notify). Schema (`tools/list`) UNTOUCHED.
  `[M4-REQ-08][M4-REQ-15]` `[unit][D16]`
- [ ] 8.11 GREEN: add ONE `#[ignore]` `vibelens_real_npx_show_diff_explanation` test (un-ignore
  only after G2/WU-9 pins the schema). `[M4-REQ-12]` `[manual][D36]`
- [ ] **Gate (WU-8)**: `cargo test --workspace` green (7 fake-backed unit/integration tests; real
  test `#[ignore]`d); fmt/clippy clean; `cargo deny ... check bans` → `bans ok`.

> **Slice 4 COMPLETE after WU-8.** PR-4 = WU-6 + WU-7 + WU-8 (+ WU-9 gate doc). Green:
> GitPort/dedup unit + fake-stdio contract + pipeline integration; G2 documented. Exit criteria
> 4 (VibeLens < seconds), 5 (per-file rationale), 7 (generic degrade).

---

## WU-9 — Pre-Apply Gate G2: verify `show_diff_explanation` param schema  [manual][ci]
**Commit**: `docs(m4): record verified show_diff_explanation tools/list schema (G2)`
**Depends on**: nothing (runs against the running VibeLens stdio server).
**Blocks**: WU-8 GREEN un-ignoring the real-`npx` test (8.11); does NOT block WU-8 fake-backed build.
**Rollback**: n/a (verification + doc artifact).
**PR slice**: PR-4 (rides Slice 4; ~0 code lines).

> R4 OPEN. Transport already VERIFIED = stdio (`.mcp.json`: `npx -y vibelens-mcp`). Only the
> `show_diff_explanation` param schema is unverified (CLAUDE.md prose only: `git diff HEAD` +
> per-file analysis). The `DiffExplainerPort` + `VibeLensMcpAdapter` isolate this — pin the
> field names + response shape here BEFORE un-ignoring the real test.

- [ ] 9.1 Run `tools/list` against `npx -y vibelens-mcp` (stdio JSON-RPC). Confirm the
  `show_diff_explanation` input schema — exact param field names (`diff`, `file_analysis`?) and
  types. `[M4-REQ-12]` `[manual]`
- [ ] 9.2 Confirm the response shape (how `files[]` + `summary` map back to `DiffExplanation`).
  `[M4-REQ-12]` `[manual]`
- [ ] 9.3 Record findings in `design.md` §Pre-Apply Gates G2 (check the box); adjust the
  `VibeLensMcpAdapter` request/parse code (WU-8.8) + un-ignore 8.11 if shapes confirmed. `[ci]`
- [ ] **Gate (WU-9)**: G2 findings documented; adapter request/parse pinned. No build/test gate.
  If VibeLens is unreachable, mark G2 deferred and keep WU-8.11 `#[ignore]` — slice ships green
  on the fake stdio child.

---

## WU-10 — UI triad: ipc.ts + SpecPane + VibeLensPanel + TriadLayout  [unit]
**Commit**: `feat(ui): add SpecPane, VibeLensPanel, and TriadLayout wired to spec/diff IPC (D29)`
**Depends on**: WU-4 (`spec_updated`/`get_spec`) + WU-5 (`approve_prompt`) + WU-8 (`diff_updated`/
`get_diff_explanation`).
**Blocks**: WU-11 (manual acceptance).
**Strict TDD**: RED vitest specs for each component/listener FIRST (mocked IPC). Test runner
`pnpm -C ui test`. React 19 named imports; NO manual `useMemo`/`useCallback`; vitest mocks.
**Rollback**: revert → backend events emit but no triad UI; existing panes unaffected.
**PR slice**: PR-6 (UI triad slice). Slice 4 was sub-split into PR-4 (WU-6/7/8/9) + PR-5 to
stay within the 400-line review budget, so the UI triad lands as PR-6 (WU-10 + WU-11).

> D29. Mirror the existing `ipc.ts` listener pattern (`listenStatusChanged/Created/Closed`).
> Add `listenSpecUpdated`, `listenDiffUpdated`, `getSpec`, `getDiffExplanation`, `approvePrompt`.
> SpecPane = live checklist from `spec_updated` (no refresh) + per-task `TaskState` + generic-tier
> coarse scraped badge + plan-approval gate (Approve/Edit/Reject → `approve_prompt`). VibeLensPanel
> = per-file rationale from `diff_updated` + manual refresh control + degraded/empty states.
> TriadLayout = Spec | Terminal | VibeLens, all visible per session. Slice 5 also adds minimal
> `spectty_status`/`spectty_cost` effect stubs (per design slice map).

- [ ] 10.1 RED: `ipc listeners — listenSpecUpdated/listenDiffUpdated/approvePrompt` (vitest,
  mocked `@tauri-apps/api`) — each registers/invokes the correct event/command name + payload
  shape. `[M4-REQ-16][M4-REQ-17][M4-REQ-19]` `[unit]`
- [ ] 10.2 RED: `SpecPane renders live checklist from spec_updated without refresh` — emits a
  `spec_updated` → checklist updates, each task shows its `TaskState`. `[M4-REQ-18]` `[unit]`
- [ ] 10.3 RED: `SpecPane shows generic-tier coarse scraped badge` — generic-tier session → coarse
  badge rendered (not per-task detail). `[M4-REQ-18]` `[unit]`
- [ ] 10.4 RED: `SpecPane approval gate calls approve_prompt and hides once resolved` —
  Approve/Edit/Reject → `approvePrompt(session_id, action_id, decision)`; gate hides on resolution.
  `[M4-REQ-19]` `[unit]`
- [ ] 10.5 RED: `VibeLensPanel renders per-file rationale from diff_updated` — emits `diff_updated`
  → per-file rationale; empty → "no changes"; degraded → "unavailable"/"parse error" (no blank/
  crash). `[M4-REQ-20]` `[unit]`
- [ ] 10.6 RED: `VibeLensPanel manual refresh forces fresh explanation` — refresh control triggers
  a fresh explanation independent of the auto trigger. `[M4-REQ-21]` `[unit]`
- [ ] 10.7 RED: `TriadLayout shows spec + terminal + vibelens per session` — all three visible.
  `[M4-REQ-22]` `[unit]`
- [ ] 10.8 GREEN: extend `ui/src/session/ipc.ts` — add the 5 listeners/commands. `[M4-REQ-16][M4-REQ-17]`
  `[unit][D29]`
- [ ] 10.9 GREEN: create `ui/src/components/SpecPane.tsx` (checklist + approval gate),
  `ui/src/components/VibeLensPanel.tsx` (rationale + refresh + degraded states), and
  `ui/src/components/TriadLayout.tsx` (Spec | Terminal | VibeLens). React 19 named imports.
  `[M4-REQ-18][M4-REQ-19][M4-REQ-20][M4-REQ-21][M4-REQ-22]` `[unit]`
- [ ] 10.10 GREEN: add minimal `spectty_status`/`spectty_cost` effect stubs in
  `crates/spectty-mcp/src/main.rs` (per design slice map; schema UNTOUCHED). `[M4-REQ-08]` `[unit][D16]`
- [ ] 10.11 Poll loop watches `spectty/{sid}/approval` → emits `status_changed(AwaitingInput)` +
  `quick_actions` from `options[]` (REQ-10 surfacing half; the resolver half shipped in PR-3/WU-5).
  Reuses the EXISTING M2 status path — NO new approval event (D29). Consumes the pending-doc shape
  PR-3 pinned at `spectty/{session_id}/approval`. `[M4-REQ-10]` `[unit][D29][D31]`
- [ ] **Gate (WU-10)**: `pnpm -C ui test` green (all vitest specs) + `pnpm -C ui build` succeeds;
  `cargo test --workspace` green; fmt/clippy clean; `cargo deny ... check bans` → `bans ok`.

---

## WU-11 — Manual acceptance (M4 exit gate) + ADR D26-D38 note  [manual]
**Commit**: `docs(m4): record M4 manual acceptance (8 exit criteria) + append ADR D26-D38 notes`
**Depends on**: ALL prior WUs landed (full triad running). This is the `sdd-verify` pass/fail gate.
**Rollback**: n/a (verification + doc artifact).
**PR slice**: PR-6 (verify/doc, ~0 code lines; folds into the UI triad PR or stands alone).

> Maps verbatim to the spec acceptance gate (M4-REQ-25) and the 8 roadmap exit criteria.
> CANNOT be unit-tested. Run the real app on macOS (gating); generic-agent degradation path
> exercised explicitly. The `show_diff_explanation` field names (G2) and engram REST shapes (G1)
> are EMPIRICAL — refining them is a data/adapter change, never a Core change.

- [ ] 11.1 **Exit 1 — seed/plan**: a cooperative agent calls `spectty_spec` → SpecPane shows the
  seeded intent + task checklist, no manual refresh. `[M4-REQ-09][M4-REQ-18][M4-REQ-25]` `[manual]`
- [ ] 11.2 **Exit 2 — approval gate**: `spectty_approval` → status `AwaitingInput` + quick_actions;
  Approve via the SpecPane gate → agent unblocks; tasks may move to `InProgress` only after
  Approved. `[M4-REQ-07][M4-REQ-10][M4-REQ-11][M4-REQ-19][M4-REQ-25]` `[manual]`
- [ ] 11.3 **Exit 3 — live progress**: agent updates progress → SpecPane checklist reflects
  `TaskState` changes live (within one poll tick), no refresh. `[M4-REQ-03][M4-REQ-09][M4-REQ-17][M4-REQ-18][M4-REQ-25]`
  `[manual]`
- [ ] 11.4 **Exit 4 — VibeLens < seconds**: agent edits files (cooperative `spectty_diff`) →
  VibeLensPanel shows the explanation within seconds. `[M4-REQ-13][M4-REQ-15][M4-REQ-20][M4-REQ-25]`
  `[manual]`
- [ ] 11.5 **Exit 5 — per-file rationale**: VibeLensPanel shows per-file rationale (not just a
  summary). `[M4-REQ-12][M4-REQ-20][M4-REQ-25]` `[manual]`
- [ ] 11.6 **Exit 6 — restart restore**: close + re-attach the session → SpecPane restores
  spec+progress from engram immediately (no 2s blank); engram-down degrades gracefully.
  `[M4-REQ-02][M4-REQ-23][M4-REQ-25]` `[manual]`
- [ ] 11.7 **Exit 7 — generic degrade**: a GENERIC (non-cooperative) agent → spec via PTY-scrape
  coarse badge + VibeLens via debounced FileWatcher fallback; VibeLens/git failure shows
  "unavailable"/"parse error", never crashes. `[M4-REQ-14][M4-REQ-15][M4-REQ-18][M4-REQ-25]` `[manual]`
- [ ] 11.8 **Exit 8 — triad layout**: Spec pane + Terminal + VibeLens all visible per session on
  macOS. `[M4-REQ-22][M4-REQ-25]` `[manual]`
- [ ] 11.9 DOC: append D26-D38 ADR notes to `docs/decisions/0004-agent-agnostic-core.md`
  (§Amendment M4) — one note per decision, referencing the implementing files; record the
  Tasks-phase resolution (async-trait absent → sync ports) and G1/G2 verified shapes. `[manual]`
- [ ] 11.10 Record results in `openspec/changes/M4-triad-spec-vibelens/acceptance.md` for
  `sdd-verify`. macOS criteria (11.1–11.8) gating; Windows best-effort, MUST NOT block. `[manual]`
- [ ] **Gate (WU-11)**: all 8 macOS exit criteria PASS → M4 acceptance PASS; results recorded in
  acceptance.md. Windows informational only.

---

## Cross-cutting gates (apply to every code WU)
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo deny --manifest-path crates/core/Cargo.toml check bans` — Core quarantine stays green:
  Core gains ONLY pure `serde + thiserror` entities/ports; NO `serde_json`/`tokio`/`reqwest`/
  `notify`/`git2`/`async-trait`/`tauri` runtime dep; `PersistencePort` UNCHANGED (M4-REQ-01);
  `tools/list` schema byte-frozen (M4-REQ-08).
- UI WUs: `pnpm -C ui test` + `pnpm -C ui build`.
- VibeLens: after edits in a WU, call `show_diff_explanation` with that WU's `git diff HEAD`.

---

## Parallelism map

| Can run in parallel | Sequential dependency |
|---|---|
| WU-1 + WU-3 + WU-6 (pure Core entities WU-3/6 have no I/O dep on WU-1) | WU-0 (G1) precedes WU-1 real test |
| WU-7 + WU-8 adapter dev (both need WU-6 ports) | WU-1 → WU-2 (poll needs the port) |
| (G1/WU-0 and G2/WU-9 gates can run anytime against live daemons) | WU-2 + WU-3 → WU-4 (effect needs poll + entities) |
| — | WU-3 + WU-4 → WU-5 (approval needs ApprovalState + poll path) |
| — | WU-6 → WU-7 + WU-8 (adapters need ports); WU-9 (G2) → WU-8 real test |
| — | WU-4 + WU-5 + WU-8 → WU-10 (UI consumes all events) |
| — | WU-10 → WU-11 (acceptance needs full triad) |

**Dependency order FIXED (design): persistence → spec → approval → diff → UI.**

---

## Review Workload Forecast

**Estimated changed lines per WU** (additions + deletions, approximate, excluding lockfile churn):

| WU | Description | Slice / PR | Est. lines |
|---|---|---|---|
| WU-0 | G1 engram REST verification (doc) | PR-1 | ~10 |
| WU-1 | EngramHttp trait + EngramAdapter + FakeEngramHttp + W1 doc + 4 tests | PR-1 | ~200 |
| WU-2 | SpecBus poll loop + 5 tests | PR-1 | ~150 |
| WU-3 | Core spec.rs entities + transitions + gate + 5 tests | PR-2 | ~210 |
| WU-4 | spectty_spec effect + spec_updated + get_spec + hydrate + 4 tests | PR-2 | ~190 |
| WU-5 | spectty_approval long-poll + approve_prompt + 5 tests | PR-3 | ~220 |
| WU-6 | Core diff.rs + Session diff fields + 3 SYNC ports + 3 tests | PR-4 | ~150 |
| WU-7 | Git2Adapter + NotifyFileWatcher + 4 tests | PR-4 | ~170 |
| WU-8 | VibeLensMcpAdapter + diff pipeline + spectty_diff + diff_updated + 7 tests | PR-4 | ~280 |
| WU-9 | G2 show_diff_explanation schema verification (doc) | PR-4 | ~10 |
| WU-10 | UI ipc.ts + SpecPane + VibeLensPanel + TriadLayout + status/cost stubs + 7 vitest | PR-5 | ~360 |
| WU-11 | manual acceptance (8 criteria) + ADR D26-D38 notes | PR-5 | ~70 |
| **Total** | | | **~2020 lines** |

`Estimated total changed lines: ~2020`
`Chained PRs recommended: Yes`
`400-line budget risk: High` (total ~2020 lines; PR-4 at ~610 lines exceeds budget and MUST sub-split)
`Decision needed before apply: Yes` — PR-4 (Slice 4, ~610 lines) exceeds the 400-line budget and
carries a pre-planned sub-split. Recommended: **6-PR stacked-to-main chain**, all ≤400 lines.

**Proposed PR boundary map (stacked-to-main, all ≤400 lines):**

```
PR-1 [Slice 1: WU-0 + WU-1 + WU-2]  EngramAdapter + SpecBus poll + W1 doc  →  ~360 lines
      Green floor: FakeEngramHttp contract + SpecBus change-detection unit tests; G1 documented.

PR-2 [Slice 2: WU-3 + WU-4]  Core SpecContract + spectty_spec effect  →  ~400 lines
      Green floor: pure entity tests + poll→spec_updated integration + restart hydrate.

PR-3 [Slice 3: WU-5]  plan-approval gate + spectty_approval long-poll  →  ~220 lines
      Green floor: gate unit tests + approval long-poll resolve integration.

PR-4 [Slice 4: WU-6 + WU-7 + WU-9]  Core diff ports + Git/FileWatch adapters + G2 doc  →  ~330 lines
      Green floor: DiffExplanation/dedup unit + GitPort empty-repo + FileWatch debounce; G2 documented.

PR-5 [Slice 4b: WU-8]  VibeLensMcpAdapter + diff pipeline + spectty_diff + diff_updated  →  ~280 lines
      Green floor: fake-stdio contract + pipeline integration + degrade tests.
      (Slice 4 split into PR-4/PR-5 to keep each ≤400; both independently green.)

PR-6 [Slice 5: WU-10 + WU-11]  UI triad + manual acceptance  →  ~430 lines
      Green floor: vitest (SpecPane/VibeLensPanel/TriadLayout/ipc) + pnpm build.
      NOTE: ~430 incl. WU-11 doc; code-only (WU-10) ≈ ~360. If strict, WU-11 (~70 doc lines)
      can ride PR-6 without re-review (docs only) OR split as PR-7. Recommended: keep WU-11
      doc with PR-6; the ~430 is ~360 code + ~70 docs (reviewer load ≈ ≤60 min).
```

**Auto-chain note (delivery_strategy resolved):** chain_strategy = stacked-to-main; each PR
merges to main in order. Each PR is one deliverable work-unit group with a clear start/finish,
tests+gates green at its HEAD, and a rollback that does not remove unrelated work. PR-4/PR-5
split Slice 4 to respect the 400-line budget; dependency order is fixed (PR-1→PR-2→PR-3→PR-4→
PR-5→PR-6). No `size:exception` required if PR-6's doc tail rides with the UI code (docs add
no review-comprehension load). If the reviewer prefers code-only PRs ≤400, promote WU-11 to a
trailing PR-7 (~70 doc lines).
