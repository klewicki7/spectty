# Archive Report: M2 — Spawn Agent + Provisioner

**Change**: M2-spawn-agent-provisioner
**Project**: spectty (repo: ai-terminal)
**Artifact store**: hybrid (filesystem `openspec/` + Engram)
**Archived**: 2026-06-08
**Status**: ARCHIVED — SDD cycle complete, change CLOSED.
**Verdict**: PASS-WITH-WARNINGS (0 CRITICAL, 1 WARNING, 3 SUGGESTIONS — all SUGGESTIONS
applied or carried forward; the WARNING is a documented pre-release follow-up for M3).

## Traceability (Engram observation IDs)

| Phase / artifact | Topic key | Obs ID |
|---|---|---|
| Exploration | `sdd/M2-spawn-agent-provisioner/explore` | (see Engram) |
| Proposal | `sdd/M2-spawn-agent-provisioner/proposal` | (see Engram) |
| Spec (delta) | `sdd/M2-spawn-agent-provisioner/spec` | 801 |
| Design | `sdd/M2-spawn-agent-provisioner/design` | 802 |
| Tasks | `sdd/M2-spawn-agent-provisioner/tasks` | 803 |
| Apply progress | `sdd/M2-spawn-agent-provisioner/apply-progress` | 805 |
| Verify report | `sdd/M2-spawn-agent-provisioner/verify-report` | 820 |
| Archive report | `sdd/M2-spawn-agent-provisioner/archive-report` | (this) |

Implementation merged to `main` @ `74085c3`.

## What M2 delivered

The agent runner, status machine, output-signal pipeline, session registry, provisioning
port, and the spawn/status UI — all built on the M0 hexagonal skeleton + M1 PTY layer, with
the Core quarantine kept INTACT (`serde` + `thiserror` only; no agent name, config format,
ANSI/regex, `tokio`, `tauri`, or `portable-pty` in Core production code). Shipped across
**12 work units (WU-1..WU-12)** delivered as **10 stacked PRs (#7–#16)**, Strict TDD
throughout.

- **`AgentRunner` Core port** with the M2 method subset (`launch_spec`, `detect_status`,
  `descriptor`, `tier` full; `parse_cost`, `quick_actions` honest skeletons). No
  `provisioner()` method — provisioning is the separate `ProvisioningPort` (Lock 1, supersedes
  ADR-0004 method shape for M2). Two adapters: `ClaudeCodeRunner` (Cooperative, empirical
  pattern table as DATA) + `GenericRunner` (Generic, injected-clock idle-timeout).
- **Pure Core `transition` state machine** — `AgentStatus` { Starting, Idle, Running,
  AwaitingInput, Completed, Error }, total `transition(current, observed) -> AgentStatus`,
  illegal observations leave `current` unchanged. 30-cell legal-table test passes.
- **`OutputSignal`** Core serde value type (serde-friendly time field, NEVER `Instant`),
  produced by an impure adapter on a SECOND, independent PTY-read consumer with a bounded
  drop-oldest buffer that can never throttle the M1 render path.
- **`SessionRegistry`** in Core (`&self` interior mutability, `SessionId == PtyId`), distinct
  from the `src-tauri` `PtyRegistry` that holds OS handles.
- **`ProvisioningPort` + `ProvisionerAdapter`** — pure `String -> String` JSON managed-namespace
  editor owning only `spectty_*` keys (foreign keys round-trip untouched), atomic write with
  `.spectty.bak` backup, GLOBAL/PROJECT scope via injected `is_git_tracked` predicate.
- **`spectty-mcp` binary** — registered-but-stubbed, starts over stdio, advertises the five
  tool schemas (`spectty_spec`, `spectty_diff`, `spectty_approval`, `spectty_status`,
  `spectty_cost`); calls return benign acknowledgements (effects deferred to M3).
- **Spawn/close UI + bridge** — `spawn_session` / `close_session` commands, `status_changed`
  event emitted only on an actual status change, `SpawnDialog` + `PaneHeader` (status badge
  flows DOWN from `useSession`; UI never computes status locally — backend authoritative).

## Final test counts

- **Rust**: `cargo test --workspace` → **150 tests, 0 failed** (core 37, adapters 77,
  src-tauri lib 23, spectty-mcp 11 + 2 stdio handshake).
- **UI**: `pnpm -C ui test` → **44 passed / 6 files**.
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets -- -D warnings`
  no warnings; `cargo deny ... check bans` → `bans ok`; `cargo build -p spectty` +
  `pnpm -C ui build` succeed.

## 5 apply-phase bugs caught-and-fixed by the fresh-review gate

The Strict-TDD + adversarial fresh-review gate caught five real defects during apply; each is
fixed on `main` with a regression test:

1. **PR1b — transition-table spec violation**: the design §3.4 code block contradicted the
   spec; the spec wins. `agent_status.rs::transition` corrected so `(Completed|Error,_)`
   is absorbing, `(Running|Idle,Finished)=>Completed`, and illegal jumps from
   `Starting`/`AwaitingInput` are rejected. Regression: `transition_covers_the_full_legal_table`.
2. **PR2b — UTF-8 corruption**: the OutputSignal producer split multi-byte UTF-8 across chunk
   boundaries. Fixed with `Vec<u8>` accumulation + `from_utf8_lossy`. Regression:
   `producer_preserves_multibyte_utf8*`.
3. **PR3 — config-reformat honesty**: the JSON editor reordered/reformatted foreign keys.
   Fixed with `preserve_order`; a non-object `mcpServers` now surfaces as a parse error, not
   silent data loss. Regression: `non_object_mcp_servers_is_a_parse_error_not_data_loss`.
4. **PR5b — spawn-failure leak**: a failed PTY spawn left a half-provisioned session.
   Fixed with `cleanup_failed_spawn`. Regression:
   `spawn_session_cleans_up_when_pty_spawn_fails`.
5. **PR6 — close affordance**: no way to close a session from the UI. Added a `PaneHeader`
   Close button + 2 tests.

## Verify verdict (obs 820)

PASS-WITH-WARNINGS, **0 CRITICAL**. Hexagonal quarantine INTACT
(`cargo tree -p spectty-core -e normal` = serde + thiserror only; `serde_json` is a test-only
dev-dep, out of the `-e normal` ban graph). Every spec requirement across all 7 capabilities
(agent-runner, agent-status-machine, output-signal, session-registry, provisioning-port,
agent-session-ui, hexagonal-core) maps to an implementation AND a passing test. The 5 roadmap
exit criteria are `[manual]` (live Claude Code + real PTY) as EXPECTED; each claimed automated
floor verified present + passing. Ready to archive: YES.

### SUGGESTIONS applied at archive
- **S1 (applied)**: the stale `detect_status -> Option<AgentStatus>` (change `spec.md:46`,
  lagging the D8 design refinement to `Option<Observed>`, `design.md:370`) was corrected to
  `detect_status(&OutputSignal) -> Option<Observed>` in the promoted baseline
  `openspec/specs/agent-runner/spec.md`. The stale signature was NOT carried into the baseline.
- **S3 (applied)**: the cosmetic WU-11 Gate checkbox in `tasks.md:270` was ticked (both
  sub-tasks done and passing) before archiving.

## CARRIED-FORWARD to M3

| Item | Source | What M3 must do |
|---|---|---|
| **W1 / L2** | verify WARNING | Add `spectty-mcp` to `tauri.conf.json` `bundle.externalBin` before any packaged release — a packaged build would otherwise point at an unbundled binary (Lock-4 failure mode). Outside M2's dev acceptance gate, so it did not block archive. |
| **L3 / S2** | verify SUGGESTION | Wire `child.wait()` (EOF currently arrives as `code: None` at `pty.rs:275`, so Error-on-nonzero-exit is not driveable). Harden L3 in M3. |
| **L5 / R8** | design deferral | `Provisioner` boot-time orphan reconciliation. M2 ships the `.spectty.bak` + idempotent `retract` escape hatch; full reconciliation is M3. |
| **L1** | design deferral | Validate the real `~/.claude.json` shape against the managed-namespace editor. |
| **L4** | design deferral | Refine the Claude scrape patterns against a real Claude Code session (empirical DATA edit in `ClaudeCodeRunner` + a unit test, never a Core change). |

## Specs promoted to / extended in the living baseline

The M2 delta specs were merged into the project's living baseline at `openspec/specs/`. Five
new capability specs created, one baseline extended:

| Capability | Baseline spec file | Action |
|---|---|---|
| agent-runner | `openspec/specs/agent-runner/spec.md` | Created (incl. the agent-status-machine `transition` requirement; S1 `detect_status -> Option<Observed>` correction applied) |
| output-signal | `openspec/specs/output-signal/spec.md` | Created (2 requirements) |
| session-registry | `openspec/specs/session-registry/spec.md` | Created (3 requirements) |
| provisioning-port | `openspec/specs/provisioning-port/spec.md` | Created (6 requirements) |
| agent-session-ui | `openspec/specs/agent-session-ui/spec.md` | Created (2 requirements) |
| hexagonal-core | `openspec/specs/hexagonal-core/spec.md` | Extended (M2 MODIFIED: Core grows the agent domain incl. `OutputSignal` with ZERO new deps; supersedes the M1 OutputSignal-ban clause; dependency-set + agent-agnostic invariants retained) |

Note: `agent-status-machine` is NOT a distinct baseline capability — its `transition` /
`AgentStatus` requirement lives inside the `agent-runner` capability (the M2 delta carried no
standalone `agent-status-machine.md` file). The original M2 delta specs are preserved in this
archive folder under `specs/` as the historical record.

## Archive contents

- `explore.md`
- `proposal.md`
- `specs/` (original M2 delta specs — historical record)
- `design.md`
- `tasks.md` (12 work units, all gates ticked)
- `acceptance.md`
- `verify-report.md`
- `archive-report.md` (this file)

## SDD cycle complete

M2-spawn-agent-provisioner was explored → proposed → specified → designed → tasked → applied
(10 stacked PRs #7–#16, Strict TDD, 5 apply-phase bugs caught + fixed) → verified
(PASS-WITH-WARNINGS, 0 CRITICAL) → archived. The change is CLOSED. Next: **M3** (carry-forward
items above: spectty-mcp bundling, child.wait wiring, orphan reconciliation, real-config
validation, Claude pattern refinement).
