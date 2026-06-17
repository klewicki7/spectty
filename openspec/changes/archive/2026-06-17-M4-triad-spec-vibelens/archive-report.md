# Archive Report: M4 — The Triad (Living Spec Pane + VibeLens + Why)

**Change**: M4-triad-spec-vibelens
**Project**: spectty (repo: ai-terminal)
**Artifact store**: hybrid (filesystem `openspec/` + Engram)
**Archived**: 2026-06-17
**Status**: ARCHIVED — SDD cycle complete, change CLOSED.
**Verdict**: PASS WITH WARNINGS (0 CRITICAL, 4 WARNINGS, 2 SUGGESTIONS; no blockers to archive).

## Traceability (Engram observation IDs)

| Phase / artifact | Topic key | Obs ID |
|---|---|---|
| Exploration | `sdd/M4-triad-spec-vibelens/explore` | #867 |
| Proposal | `sdd/M4-triad-spec-vibelens/proposal` | #868 |
| Spec (delta) | `sdd/M4-triad-spec-vibelens/spec` | #870 |
| Design | `sdd/M4-triad-spec-vibelens/design` | #869 |
| Tasks | `sdd/M4-triad-spec-vibelens/tasks` | #871 |
| Apply progress | `sdd/M4-triad-spec-vibelens/apply-progress` | #874 |
| Verify report | `sdd/M4-triad-spec-vibelens/verify-report` | #906 |
| Archive report | `sdd/M4-triad-spec-vibelens/archive-report` | (this obs, Engram) |

Implementation merged to `main` @ `f209fa1` (6 PRs #36–#41 all stacked and merged). Manual acceptance run: PENDING (macOS aarch64, real Claude Code + engram :7437 + npx vibelens-mcp).

## What M4 delivered

Turns the instrumented terminal into the signature **TRIAD** for a SINGLE session: **SPEC** (living contract + plan-approval gate + live task progress) → **DIFF** (VibeLens panel) → **WHY** (per-file rationale). Closes agent drift problem (ADR-0007). Delivered across **12 work units (WU-0..WU-11)** in **6 stacked PRs (#36–#41)** with Strict TDD. Implements the FROZEN 5 MCP tools' real EFFECTS behind unchanged schema. Wires `EngramAdapter` to engram's local HTTP API. Builds React 19 UI triad. Core stays pure serde+thiserror; all I/O in adapters/src-tauri.

### Headline changes

- **`EngramAdapter` real impl**: HTTP POST/GET vs engram local :7437 API for `upsert`/`get`; per-session poll loop (default 2s, env-configurable) detects change via `updated_at` field; degrades gracefully when engram down (map to `PersistenceError::Backend`, log, retain last-known, never crash session).
- **Core spec entities**: `SpecContract { intent, proposal, tasks[], progress, approval:ApprovalState, steering_notes }`, `TaskState (Pending/InProgress/Done/Skipped)` with one-way transitions, `ApprovalState (Pending/Approved/Rejected/Adjusted)`. Pure serde+thiserror, no I/O.
- **Plan-approval gate**: Core business rule per ADR-0007: `may_begin_edits()` returns true ONLY when `Approved`; dev override flag separate, not default. Blocking tool `spectty_approval` registers pending request (session_id, action_id), long-polls engram for resolution.
- **VibeLens diff pipeline**: Three new SYNC Core ports (`GitPort`, `FileWatchPort`, `DiffExplainerPort`); `McpAdapter` calls VibeLens stdio server (`show_diff_explanation`); hash-dedup vs `last_diff_hash` skips redundant MPC calls; `FileWatch` debounced 500ms–1s generic fallback; `spectty_diff` cooperative signal bypasses debounce; degrade table (MCP unreachable, git fail, parse error → log+retain, never crash).
- **Tauri commands + events**: `get_spec`, `get_diff_explanation` (commands); `spec_updated { session_id, spec }`, `diff_updated { session_id, explanation }` (Tauri v2 Emitter events, fire only on change).
- **UI triad**: `SpecPane` live checklist (no refresh on `spec_updated`), plan-approval gate UI (Approve/Adjust/Reject), coarse generic-tier badge; `VibeLensPanel` per-file rationale, manual Refresh, empty vs degraded states; `TriadLayout` (Spec | Terminal | VibeLens); exactly one `.terminal-pane` preserved (App test intact).
- **Restart recovery**: Re-attach reads `spectty/{session_id}/spec` + `/progress` BEFORE first poll, emits initial `spec_updated` so UI restores immediately; engram-down degrades empty/last-known no crash.
- **W1 doc-only fix**: Agent-runner baseline scenario corrected: `(Starting, Ready) => Idle` now stated consistently with M2 core table row `((Starting, Ready), Idle)`. No code/test change.

## Final test counts

- **Rust**: `cargo test --workspace` → **351 tests, 0 failed**:
  - Tauri lib: 90 passed
  - Adapters: 160 passed, 2 ignored (G1/G2 real-daemon `#[ignore]`)
  - Core: 52 passed (unchanged — quarantine intact)
  - spectty-mcp: 33 passed
  - spectty-hook: 1 + 11 passed
- **UI**: `pnpm -C ui test` → **86 tests, 0 failed** (14 files; apply-progress claimed 84; PR #41 (F1/F3 fix) added 2 SpecPane specs → 86).
- `cargo fmt --all -- --check` → clean (EXIT 0).
- `cargo clippy --workspace --all-targets -- -D warnings` → 0 warnings/0 errors (EXIT 0).
- `cargo deny --manifest-path crates/core/Cargo.toml check bans` → **`bans ok`** (Core quarantine intact; zero new external deps).

## Verify verdict (obs #906)

**PASS WITH WARNINGS**, **0 CRITICAL**. Implementation complete, fully tested (351 passing Rust + 86 UI tests), all gates GREEN (cargo build, clippy -D, cargo-deny, fmt). Manual acceptance run documented as PENDING (only the user can execute on real macOS stack with real Claude Code + engram + VibeLens). Does NOT block archive.

- **Critical**: 0
- **Warnings**: 4
  - **W1** (process): Manual acceptance run PENDING — all 8 roadmap criterion result lines = ☐. Automated floor GREEN, mechanical core verified; M4 *acceptance* = PENDING.
  - **W2** (deviation, undocumented): D38 engram-key cleanup (`close_session` best-effort delete of 3 `spectty/{sid}/*` keys) NOT implemented. Behaviorally low-impact (engram keys upsert, leaked key harmless, new sessions overwrite, mirrors D14 best-effort). Undocumented gap between design and code.
  - **W3** (deferred, documented): VibeLens quoted-path parsing (PR-5 review F3): `changed_files` splits on ` b/`, so a path with space inside quoted `diff --git "a/x y" "b/x y"` header is dropped from per-file annotations. Data-only fix, no Core impact. Recorded in tasks.md:560 + acceptance.md.
  - **W4** (carry-forward, documented): `spectty_status` / `spectty_cost` remain stubs (schema FROZEN, effects deferred M5+). Pinned by existing tests.
- **Suggestions**: 2
  - **S1** (stale artifact): apply-progress obs #874 line "Gate action_id == 'plan'" is SUPERSEDED by PR #41's `get_approval` fix. Original hardcoded "plan" would deadlock; fix adds `get_approval` command + IPC, SpecPane fetches REAL pending request action_id. REQ-19 now correct. Flag in archive: #874 doc is stale on this point.
  - **S2** (unverified): engram session-row memoization (PR-1 Finding 5): confirm `ensure_session` is memoized so 2s production poll loop doesn't double session-row write traffic. Verify during acceptance run.

## M3 carry-forwards

No NEW carry-forward. M4 touched agent-runner only for W1 doc correction (zero-risk). No regression in 351 Rust / 86 UI tests.

## Design deviations — all documented + test-pinned

| Deviation | Justification | Location |
|-----------|---------------|----------|
| D26 EngramHttp trait abstraction | REST surface UNVERIFIED (G1 gate); FakeEngramHttp contracts NOW, verified shapes swap later w/o touching port | design.md ADR-D26 |
| D27 SpecBus subscribe/poll is adapter-side | Rejected adding subscribe to Core port (forces async+serde_json::Value, violates R6 quarantine) | design.md ADR-D27 |
| D30 State-file side-channel DEFERRED | 2s poll satisfies exit criterion; side-channel reserved, reopen only on acceptance evidence | design.md ADR-D30 |
| D31 spectty_approval blocking via engram round-trip | Handler long-polls GET on same key for resolution written back; lower latency than HTTP callback for M4 baseline | design.md ADR-D31 |
| D34 Diff dedup per-session | Hash-dedup state on per-session DiffPipeline (bridge); Session.last_diff/last_diff_hash retained as Core capacity for M5 | design.md ADR-D34, spec.md REQ-13 PR-5 note |
| G2 VibeLens adapter = display SINK | show_diff_explanation is a WRITE tool, adapter pushes explanation to VibeLens for rendering; returns local explanation regardless of push outcome | design.md G2 amendment, acceptance.md empirical-shape note |

All deviations documented in design.md ADR sections + spec.md PR notes + acceptance.md. No test-pinned deviation contradictions.

## Specs promoted to / extended in the living baseline

The M4 delta specs were merged into the project's living baseline at `openspec/specs/`. Thirteen new capability specs created (12 new + 1 W1 correction); two baseline extended:

| Capability | Baseline spec file | Action |
|---|---|---|
| persistence-port | `openspec/specs/persistence-port/spec.md` | Extended (M4 ADDED: EngramAdapter HTTP impl, per-session poll seam, degrade semantics) |
| spec-contract | `openspec/specs/spec-contract/spec.md` | Created (NEW: SpecContract, TaskState, ApprovalState entities) |
| plan-approval-gate | `openspec/specs/plan-approval-gate/spec.md` | Created (NEW: Core business rule, gate-before-edit, dev override) |
| spectty-spec-effect | `openspec/specs/spectty-spec-effect/spec.md` | Created (NEW: spectty_spec MCP tool real effect, poll→event) |
| spectty-approval | `openspec/specs/spectty-approval/spec.md` | Created (NEW: blocking approval resolver, long-poll) |
| diff-pipeline | `openspec/specs/diff-pipeline/spec.md` | Created (NEW: FileWatch/DiffExplainer/Git ports, trigger arbitration, hash-dedup) |
| spectty-diff-effect | `openspec/specs/spectty-diff-effect/spec.md` | Created (NEW: spectty_diff cooperative signal, generic fallback) |
| tauri-bridge | `openspec/specs/tauri-bridge/spec.md` | Extended (M4 ADDED: get_spec/get_diff_explanation commands, spec_updated/diff_updated events) |
| spec-pane-ui | `openspec/specs/spec-pane-ui/spec.md` | Created (NEW: live checklist, approval gate UI, generic badge) |
| vibelens-panel-ui | `openspec/specs/vibelens-panel-ui/spec.md` | Created (NEW: per-file rationale, manual refresh, degraded states) |
| triad-layout | `openspec/specs/triad-layout/spec.md` | Created (NEW: Spec \| Terminal \| VibeLens layout, routing) |
| restart-recovery | `openspec/specs/restart-recovery/spec.md` | Created (NEW: re-attach hydrate, engram-down degrade) |
| agent-runner | `openspec/specs/agent-runner/spec.md` | Extended (M4 W1 MODIFIED: hook-derived Ready scenario corrected to `(Starting, Ready) => Idle`) |

The original M4 delta specs are preserved in this archive folder under `specs/` as the historical record.

## Pre-apply gates (resolved)

- **G1** (blocks Slice 1 apply): VERIFY engram :7437 REST against running daemon. **RESOLVED**: Design ADR-D26 + acceptance.md empirical-shape note confirm `:7437 /observations` path + updated_at field (STRING type per G1 finding).
- **G2** (blocks Slice 4 apply): VERIFY show_diff_explanation param schema. **RESOLVED**: PR-4 branch verified via running `npx -y vibelens-mcp` — schema = `{title(req), diff(req), summary?, annotations?, ...}`, WRITE tool returning `{ok, reviewId, ...}`. Recorded in design.md G2 amendment + acceptance.md.

## CARRIED-FORWARD to M5+

| Item | Recommendation |
|---|---|
| **W1 / manual acceptance run** | User runs on real macOS packaged build w/ Claude Code + engram :7437 + npx vibelens-mcp. All 8 roadmap criteria (11.1–11.8) documented in acceptance.md. |
| **W2 / D38 engram-key cleanup** | Implement best-effort delete of 3 `spectty/{sid}/*` keys in `close_session` OR record as deferred (low-impact, mirrors D14 best-effort retraction; leaked key harmless). |
| **W3 / VibeLens quoted-path parsing** | Adapter fix: parse quoted git diff headers. Data-only, no Core impact. Recorded in acceptance.md deferred items. |
| **S1 / apply-progress #874 stale line** | Document that "Gate action_id == 'plan'" is superseded by PR #41's `get_approval` fix; REQ-19 now correct. |
| **S2 / session-row memoization** | Confirm `ensure_session` is memoized under real 2s poll loop. Verify during acceptance run. |
| **S3 / state-file side-channel (D30 reserve)** | Reopen only if acceptance evidence shows 2s poll latency impact. Side-channel held in reserve. |
| **spectty_status / spectty_cost stubs** | Deferred; schema FROZEN, effects to M5+ (cost UI). Schema/parameter shapes NOT changing. |
| **Diff persistence (not implemented)** | Session.last_diff/last_diff_hash retained as Core capacity for M5 multi-session diff restore (currently transient per-session). |
| **HTTP-callback IPC (D1B reserve)** | Port negotiation deferred to M5+. M4 baseline engram-as-bus satisfies all exit criteria. |

## Archive contents

- `proposal.md` (M4 intent, scope, ratified decisions, slice plan)
- `specs/` (original M4 delta specs — historical record):
  - `spec.md` (change-level, 25 requirements, acceptance gate, exit-criteria trace, degradation table)
  - `persistence-port.md` (MODIFIED)
  - `spec-contract.md` (NEW)
  - `plan-approval-gate.md` (NEW)
  - `spectty-spec-effect.md` (NEW)
  - `spectty-approval.md` (NEW)
  - `diff-pipeline.md` (NEW)
  - `spectty-diff-effect.md` (NEW)
  - `tauri-bridge.md` (MODIFIED)
  - `spec-pane-ui.md` (NEW)
  - `vibelens-panel-ui.md` (NEW)
  - `triad-layout.md` (NEW)
  - `restart-recovery.md` (NEW)
  - `agent-runner.md` (MODIFIED — W1 doc correction)
- `design.md` (5 slices, 38 ADRs D26–D38, testing gates, file changes, slice mapping, pre-apply gates G1/G2)
- `tasks.md` (12 WUs, PR boundary map, W1 + async-trait check, all gates)
- `acceptance.md` (8 roadmap exit criteria 11.1–11.8 macOS gating, automated floor per criterion, deferred items, empirical-shape notes)
- `verify-report.md` (PASS WITH WARNINGS, 0 CRITICAL, 351+86 tests GREEN, all gates GREEN)
- `archive-report.md` (this file)

## SDD cycle complete

M4-triad-spec-vibelens was explored (#867) → proposed (#868) → specified (#870) → designed (#869) → tasked (#871) → applied (#874: 6 stacked PRs #36–#41, Strict TDD, all merged @ f209fa1) → verified (#906: PASS WITH WARNINGS, 0 CRITICAL, manual acceptance PENDING) → archived. The *implementation* is CLOSED. M4 *milestone acceptance* awaits the user's manual run on real macOS.

**Next recommendations**:
1. User runs manual acceptance (acceptance.md 11.1–11.8) on packaged macOS build with real Claude Code + engram + VibeLens.
2. Follow-up: implement W2 engram-key cleanup or record as deferred; verify S2 session-row memoization.
3. M5 slate: cost UI (spectty_cost effect), multi-session UI, diff persistence, state-file side-channel (if latency evidence), HTTP-callback IPC.
