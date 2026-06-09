# M2 — Spawn Agent + Provisioner — Verify Report

> SDD verify phase. Consumes spec (obs #801 + `specs/*`), tasks (obs #803 +
> `tasks.md`), design (obs #802 + `design.md`), apply-progress (obs #805),
> acceptance (`acceptance.md`). Validates the merged-to-`main` M2 implementation
> (10 stacked PRs / 12 WUs). Artifact store: HYBRID (this file + engram
> `sdd/M2-spawn-agent-provisioner/verify-report`).
>
> **Verdict: PASS-WITH-WARNINGS. Ready to archive: YES** (no CRITICAL findings;
> WARNINGS are conscious documented deferrals, not blockers).

Date: 2026-06-08 · Branch `main` @ `74085c3` · working tree clean.

---

## 1. Gate results (re-run VERBATIM)

| Gate | Command | Result |
|------|---------|--------|
| Rust tests | `cargo test --workspace` | **PASS** — 150 tests, 0 failed (core 37, adapters 77, src-tauri lib 23, spectty-mcp 11 + 2 stdio handshake; doctests 0) |
| UI tests | `pnpm -C ui test` | **PASS** — 44 passed / 6 files (ipc 7, session-ipc 9, useSession 8, useTerminal 5, PaneHeader 13, SpawnDialog 2) |
| Format | `cargo fmt --all -- --check` | **PASS** — exit 0, clean |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** — Finished, no warnings |
| Core quarantine | `cargo deny --manifest-path crates/core/Cargo.toml check bans` | **PASS** — `bans ok`, exit 0 |
| Build (Rust) | `cargo build -p spectty` | **PASS** — Finished |
| Build (UI) | `pnpm -C ui build` | **PASS** — built, only the pre-existing xterm >500 kB chunk advisory (not an error) |

All seven gates GREEN. No CRITICAL gate failures.

---

## 2. Hexagonal quarantine (Core boundary)

- `cargo tree -p spectty-core -e normal` → runtime closure is **serde + thiserror ONLY**
  (+ their proc-macro deps). NO tokio, tauri, portable-pty, serde_json, time crate.
- `crates/core/Cargo.toml`: `[dependencies]` = serde + thiserror; `serde_json` is
  `[dev-dependencies]` ONLY (test wire format) — out of the `-e normal` graph, so
  cargo-deny stays green. Documented deviation (tasks 1.4), correctly scoped.
- Agent-name literals in `crates/core/src`: every hit is inside `#[cfg(test)] mod tests`
  or a doc-comment example (`agent_spec.rs:13` is a `///` example). NO production
  agent-name branch; `AgentKind` is an opaque serde String newtype (D12), Core never
  branches on `kind`.
- NO `serde_json` / `std::process` / `regex` / `portable_pty` / `tauri` in Core
  production code. `regex` appears ONLY in a `claude_code.rs` doc-comment ("keeps regex
  out of the dep graph"); patterns are `&'static [&'static str]` DATA (D11).

**Quarantine: INTACT.** No violation.

---

## 3. Requirement → implementation → test coverage matrix

| Capability / Requirement | Implementation | Proving test(s) | Verdict |
|---|---|---|---|
| **agent-runner**: AgentRunner Core port, M2 method subset, NO `provisioner()` | `crates/core/src/ports/agent_runner.rs` (`detect_status -> Option<Observed>` D8, launch_spec, descriptor, tier default, parse_cost/quick_actions skeletons) | trait compiles; skeleton tests `*_parse_cost_is_zero_skeleton`, `*_quick_actions_is_empty_skeleton` | COVERED |
| AgentSpec/AgentTier/AgentDescriptor serde value types | `crates/core/src/entities/agent_spec.rs` | `agent_spec_round_trips_through_serde`, descriptor round-trip | COVERED |
| ClaudeCodeRunner launch_spec | `crates/adapters/src/agent/claude_code.rs` | `claude_launch_spec_program_cwd_and_session_env`, `..._ignores_user_command` | COVERED |
| ClaudeCodeRunner detect_status (patterns as DATA, no regex) | `claude_code.rs` `awaiting_input`/`ready` `&'static [&'static str]` | `claude_detect_status_each_awaiting_input_pattern_is_needs_input`, `..._wins_over_active`, unrecognized→None | COVERED |
| GenericRunner Idle + idle-timeout Completed via injected time | `crates/adapters/src/agent/generic.rs` | `generic_detect_status_*` boundary table (idle_ms==/<>timeout) | COVERED |
| parse_cost/quick_actions honest skeletons | trait defaults | skeleton tests on both runners | COVERED |
| **agent-status-machine**: 6-variant enum | `agent_status.rs` | `agent_status_exposes_all_six_variants` | COVERED |
| pure total `transition()` enforcing legal table | `agent_status.rs::transition` | `transition_covers_the_full_legal_table` (30 cells) + named-scenario tests | COVERED |
| **output-signal**: Core serde OutputSignal, non-Instant time | `crates/core/src/entities/output_signal.rs` (Timestamp millis) | `output_signal_round_trips_and_carries_no_instant`, `..._constructible_without_pty` | COVERED |
| producer on 2nd consumer, bounded drop-oldest, ANSI strip | `crates/adapters/src/agent/output_signal.rs` (pure) + bounded `sync_channel` try_send in `src-tauri/src/session_runtime.rs` | producer ANSI/window/UTF-8 tests; `bounded_signal_channel_drops_oldest_never_blocks` | COVERED |
| **session-registry**: Session aggregate +agent +created_at | `crates/core/src/entities/session.rs` | `session_exposes_m2_fields_and_round_trips` | COVERED |
| SessionRegistry &self create/lookup/close, mints SessionId==PtyId | `crates/core/src/entities/session_registry.rs` + `src-tauri/src/pty_state.rs` (next_pty_id retired) | registry create/lookup/close/mint/apply_observed tests | COVERED |
| distinct from PtyRegistry (no OS handles in Core) | Core holds Session only | `registry_holds_no_os_handle` (serde round-trip proof) | COVERED |
| **provisioning-port**: ProvisioningPort SEPARATE Core trait | `crates/core/src/ports/provisioning.rs` | `inject_returns_handle_carrying_scope_then_retract_consumes_it` | COVERED |
| pure String→String JSON namespace editor, foreign keys untouched (R7) | `crates/adapters/src/provision/json_namespace.rs` (preserve_order) | `inject_then_retract_preserves_hand_formatted_foreign_values`, `non_object_mcp_servers_is_a_parse_error_not_data_loss`, `retract_removes_only_spectty_keys` | COVERED |
| atomic write (tmp+fsync+rename) + .spectty.bak seam | `crates/adapters/src/provision/file_io.rs` (RealConfigFile/FakeConfigFile) | `first_write_creates_spectty_bak...`, `second_write_does_not_overwrite_the_backup` | COVERED |
| scope GLOBAL default / PROJECT when git-tracked | `crates/adapters/src/provision/scope.rs` | `resolve_scope_table` (fake is_git_tracked) | COVERED |
| spectty-mcp stub advertises 5 tool schemas, no effects (R4) | `crates/spectty-mcp/src/main.rs` | `tools_list_advertises_exactly_five_schemas`, `tools_call_known_returns_ack_no_effect`, unknown→-32601, bad params→-32602; stdio handshake integration | COVERED |
| inject-on-create / retract-on-close wiring | `src-tauri/src/commands/session.rs` | `spawn_session_impl_mints_inserts_and_injects_only_when_required`, `close_session_impl_kills_pty_then_retracts_and_removes` | COVERED |
| **agent-session-ui**: bridge spawn/close + status_changed event (only on real change) | `src-tauri/src/commands/session.rs` + `session_runtime.rs::observe_and_diff` | `observe_and_diff_emits_only_on_change`, StatusChanged payload tests | COVERED |
| UI spawn flow + PaneHeader badge + useSession hook | `ui/src/hooks/useSession.ts`, `components/{SpawnDialog,PaneHeader}.tsx`, `session/ipc.ts` | useSession 8, PaneHeader 13, SpawnDialog 2, session-ipc 9 (vitest) | COVERED |
| **hexagonal-core**: M2 grows Core, ZERO new deps, no agent names | Core (above) | cargo-deny `bans ok`; cargo tree serde+thiserror; agent-name scan clean | COVERED |

**Every spec requirement maps to an implementation AND a passing test. No requirement
is unimplemented or untested.**

---

## 4. The five roadmap exit criteria

| # | Criterion | Coverage | Automated floor (verified present + passing) |
|---|-----------|----------|----------------------------------------------|
| 1 | Claude Code → `Idle` | **manual-dominant** (real CLI quiescence) | `transition_starting_ready_reaches_idle` + `observe_and_diff` + Generic real-PTY pipeline. Floor PASS. |
| 2 | Managed `spectty` section present (5 tools) | **manual** (real `~/.claude.json` shape, L1) — STRONG floor | `inject_then_retract_preserves_hand_formatted_foreign_values`, `claude_provisioner` inject global/project/backup, `spectty_mcp_stdio_handshake_advertises_five_tools`. Floor PASS. |
| 3 | `Running`→`AwaitingInput`→`Running` | **manual-dominant** (empirical R5 patterns, L4) | `transition_running_awaiting_running_round_trip` + `ClaudeCodeRunner::detect_status` pattern table. Floor PASS. |
| 4 | Close → PTY terminates + section removed | **manual** (real child observation) — STRONG floor | `inject_then_retract_removes_managed_entry`, `retract_absent_file_is_ok`, `close_session_impl_kills_pty_then_retracts_and_removes`. Floor PASS. |
| 5 | Generic `bash` → `Idle` → `Completed` | **manual** (wall-clock idle path) — STRONG floor | `#[cfg(unix)] real_pty_generic_reaches_running_then_completed` (clean-exit path) + `transition_idle_timeout_completes_from_idle`. Floor PASS. |

The five `[manual]` criteria require a live Claude Code install + real PTY and are
EXPECTED to be manual — NOT failed here. Each criterion's claimed automated floor was
verified to EXIST and PASS in this run. Criteria 2/4/5 have a STRONG floor; 1/3 are
manual-dominant by nature (real-CLI signals cannot be faithfully faked).

---

## 5. Known limitations L1–L5

| # | Limitation | Real? | Documented? | Deferral correct? | Assessment |
|---|-----------|-------|-------------|-------------------|------------|
| L1 | `~/.claude.json` shape tested vs synthetic fixtures only | YES | acceptance.md L1 + design open-q (b) | Manual acceptance step exists | OK — manual gate covers it. SUGGESTION below. |
| L2 | spectty-mcp sidecar NOT in `tauri.conf.json bundle.externalBin` | YES — confirmed absent | acceptance.md L2 | Dev path works; packaged build unverified | **WARNING** (see findings) |
| L3 | EOF exit-code=0 (`pty.rs:275` emits `code:None`, no `child.wait()`) → Error-on-nonzero not driveable | YES — confirmed | acceptance.md L3 | Carried to M3 | OK as conscious deferral; SUGGESTION to harden M3 |
| L4 | Claude AwaitingInput patterns are empirical `&'static str` | YES | acceptance.md L4 | One-line DATA edit + test, never Core | OK — correctly scoped, refine on real session |
| L5 | R8 orphan reconciliation deferred to M3 (no boot sweep) | YES | acceptance.md L5 + design D14 + tasks 12.8 | `.spectty.bak` + idempotent retract escape hatch shipped; M3 sweep tracked | OK — CONSCIOUS, documented, tracked deferral |

**L3 and L5 specifically**: both are CONSCIOUS, documented deferrals with a tracking
path to M3. L5/R8 ships a real escape hatch (atomic write tmp→fsync→rename, backup
before first write, idempotent `retract_absent_file_is_ok`); a leaked key is harmless
(points at the real stub binary). L3's `code` field exists (`Option<i32>`) but is always
None because termination is read-side EOF, not `child.wait()`. Neither is a silent gap.
Judgment: neither should block M2 archive. L3 is flagged as a SUGGESTION to upgrade in
M3 (it weakens the `Error` terminal path); L5 is correctly the M3 boundary.

---

## 6. Transition table (load-bearing, fixed twice during apply)

Verified `crates/core/src/entities/agent_status.rs::transition` matches the normative spec:
- `(Completed | Error, _) => current` — terminals absorbing.
- `(Running | Idle, Finished) => Completed` — **Completed reachable ONLY from Running|Idle**.
- `(Starting, Finished) => current` (Starting) — the NAMED "illegal jump rejected" scenario.
- `(AwaitingInput, Finished) => current` — also rejected (PR1b fix).
- Starting cannot jump to Completed. ✓

Regression tests present and passing: `transition_illegal_jump_starting_to_completed_is_rejected`,
`transition_awaiting_input_finish_is_rejected`, `transition_idle_timeout_completes_from_idle`,
`transition_running_to_completed_on_finish`, and the exhaustive 30-cell
`transition_covers_the_full_legal_table`. This is the PR1b spec-violation fix — CORRECT
on main (spec wins over the looser design §3.4 code block).

---

## 7. Bugs caught + fixed during apply (each on main with a regression test)

| Bug | Fix on main | Regression test | Verified |
|-----|-------------|-----------------|----------|
| PR1b transition-table spec violation (blanket `(_, Finished)=>Completed`) | `transition` restricted to `(Running\|Idle, Finished)` | `transition_illegal_jump_starting_to_completed_is_rejected`, `..._awaiting_input_finish_is_rejected`, full table | YES |
| PR2b UTF-8 corruption (byte-as-char Latin-1 window) | `Vec<u8>` window + `from_utf8_lossy` at snapshot, forward-scan truncation to UTF-8 lead | `producer_preserves_multibyte_utf8`, `..._split_across_ingests`, `..._truncate_window_on_multibyte_boundary` | YES |
| PR3 config-reformat honesty (false "byte-identical", tautological fixture) | `preserve_order` value+order contract; honest test | `inject_then_retract_preserves_hand_formatted_foreign_values`, `non_object_mcp_servers_is_a_parse_error_not_data_loss` | YES |
| PR5b spawn-failure leak (post-insert failure left orphan session + un-retracted key) | `cleanup_failed_spawn` removes session + best-effort retract on any post-insert error | `spawn_session_cleans_up_when_pty_spawn_fails`, `spawn_session_cleanup_removes_generic_session_without_retract` | YES |
| PR6 close affordance missing | PaneHeader Close button (presentational, onClose callback) | `renders a Close button and calls onClose when clicked`, `does not render a Close button when onClose is not provided` | YES |

All five fixes confirmed on `main` with a guarding regression test.

---

## 8. Tasks completeness

All 12 WUs are checked complete in `tasks.md`, each carrying `[REQ:...]` mappings.
Exceptions, all legitimate:
- 12.1–12.6 unchecked: `[manual]` exit criteria requiring a live Claude Code install +
  real PTY — run by hand, not by CI. Their automated floor is green (verified §4).
- The **WU-11 Gate checkbox** (`tasks.md:270`) is unchecked while both sub-tasks
  11.1/11.2 ARE checked and their tests pass in this run — a cosmetic checkbox omission,
  not a real gap (SUGGESTION below).

---

## 9. Findings

### CRITICAL — none

### WARNING
- **W1 (L2 — packaged provisioning unverified).** `spectty-mcp` is NOT registered in
  `src-tauri/tauri.conf.json` under `bundle.externalBin`. Dev provisioning works
  (`target/debug/spectty-mcp`), but a PACKAGED build would inject an `mcpServers` entry
  pointing at a binary that is not bundled — breaking Claude Code startup in a release
  build (the very Lock-4 failure mode the spec warns about). This is documented (L2) and
  does NOT block the M2 acceptance gate (which is dev/macOS-gated), so it is a WARNING,
  not a CRITICAL. **Recommend: track an explicit follow-up to add `externalBin` before any
  packaged M2 release.** Archive may proceed since M2's gate is the dev acceptance run.

### SUGGESTION
- **S1 (spec text vs D8 signature).** `specs/spec.md:46` still reads
  `detect_status(&OutputSignal) -> Option<AgentStatus>` whereas the shipped code and D8
  use `Option<Observed>` (the design explicitly records this refinement at design.md:370).
  Conscious reconciliation, not a defect — annotate spec.md with the D8 pointer at archive
  so the promoted `openspec/specs/` copy is self-consistent.
- **S2 (L3 hardening).** `pty.rs:275` always emits `code: None`; the `Error`-on-nonzero-exit
  path is not driveable today. Correctly deferred to M3, but flag it prominently so M3's
  reconciliation work also wires `child.wait()` into the terminal-status derivation.
- **S3 (WU-11 gate checkbox).** Tick `tasks.md:270` (the WU-11 Gate line) at archive — both
  sub-tasks are done and their tests pass; the unticked gate is cosmetic.

---

## 10. Ready to archive?

**YES.** All seven gates green; 150 Rust + 44 UI tests pass; Core quarantine intact; every
spec requirement is implemented and tested; all five exit-criteria automated floors exist
and pass; the load-bearing transition table is spec-correct with its bug-fix regressions;
all five apply-phase bugs are fixed on main with guarding tests; L1–L5 are conscious,
documented, tracked deferrals. The single WARNING (W1/L2 packaged sidecar) is outside the
M2 dev acceptance gate and documented — it does not block archive but MUST be tracked as a
pre-release follow-up. Suggestions S1–S3 are non-blocking polish for the archive step.

**next_recommended: sdd-archive.**
