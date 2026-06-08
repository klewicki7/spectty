# M2 — Spawn Agent + Provisioner — Task Checklist

> SDD tasks phase. Consumes `sdd/M2-spawn-agent-provisioner/spec` (obs #801) +
> `openspec/changes/M2-spawn-agent-provisioner/specs/*` and
> `sdd/M2-spawn-agent-provisioner/design` (obs #802) +
> `openspec/changes/M2-spawn-agent-provisioner/design.md` (ADRs D7–D20, the
> module/type layout, and the §8 Strict-TDD seam list). Artifact store: HYBRID
> (engram `sdd/M2-spawn-agent-provisioner/tasks` + this file). Format follows the
> archived M1 tasks.md.
>
> **Strict TDD is ACTIVE.** Test runners: `cargo test --workspace` (Rust),
> `pnpm -C ui test` (= `vitest run`, TS). Every code work unit pairs its RED test
> with its GREEN implementation in the SAME unit: write the failing test first,
> then make it pass, then refactor. Do NOT batch tests at the end.
>
> **Per-WU gate commands** (the M2 set):
> - `cargo fmt --all -- --check`
> - `cargo clippy --workspace --all-targets -- -D warnings`
> - `cargo test --workspace`
> - `cargo deny --manifest-path crates/core/Cargo.toml check bans` — the Core
>   quarantine gate; MUST stay green at EVERY Core-touching WU (the hard invariant:
>   `crates/core/Cargo.toml` gains NOTHING — serde + thiserror only).
> - `pnpm -C ui test` (= `vitest run`) and `pnpm -C ui build` (TS strict) for UI WUs.
> - VibeLens: after edits in a WU, call `show_diff_explanation` with that WU's
>   `git diff HEAD` (per project CLAUDE.md) — apply-phase obligation, not a commit.
>
> **Spec traceability tag** per task: `[REQ:<capability>/<short>]` maps to a spec
> Requirement. Verification class carried from spec: `[unit]` / `[manual]` / `[ci]`.
> **Design tag** `[D#]` references the ADR that fixes the decision.
>
> **Dependency rule (the spine)**: Core contracts → adapters → spectty-mcp stub →
> src-tauri wiring → UI. Within Core, the pure types/ports land before the registry
> that consumes them. Adapters depend on the Core ports. src-tauri depends on BOTH
> Core and adapters. The stub `spectty-mcp` is independent (serde-only, no Core) and
> can land in parallel with the Core/adapters WUs.

```
WU-1 (manifests) ── WU-2 (Core types+clock) ──┬── WU-3 (transition machine) ──┐
                                                ├── WU-4 (SessionRegistry) ─────┤
                                                ▼                               │
                          WU-5 (runners) ── WU-6 (OutputSignal producer)        │
                          WU-7 (provisioner: json+scope+file_io+adapter)        │
WU-8 (spectty-mcp stub, independent) ───────────────────────────────────────┐  │
                                                                             ▼  ▼
                              WU-9 (src-tauri: registry/pty unify + commands + runtime + events)
                                                                             │
                              WU-10 (UI: ipc + useSession + SpawnDialog + PaneHeader)
                                                                             │
                              WU-11 (real-PTY + stdio integration tests) ──── WU-12 (manual acceptance + ADR-0004 amend)
```

---

## WU-1 — Workspace manifests + spectty-mcp member  [ci]
**Commit**: `chore(deps): add serde_json to adapters and register crates/spectty-mcp workspace member`
**Depends on**: nothing. **Blocks**: WU-5/6/7 (adapters serde_json), WU-8 (new crate).
**Rollback**: revert → no new manifests; M1 still builds.
**PR slice**: PR1.

- [x] 1.1 Add `serde_json` to `crates/adapters/Cargo.toml` (JSON namespace editor); confirm `portable-pty` already present from M1. `[REQ:provisioning-port/json-namespace]` `[ci]` `[D17]` — already present from M1 (no change needed).
- [x] 1.2 Create `crates/spectty-mcp/Cargo.toml` (`[[bin]]`, deps `serde` + `serde_json` ONLY — NOT `spectty-core`, NOT `tauri`). `[REQ:provisioning-port/spectty-mcp-stub]` `[ci]` `[D16]`
- [x] 1.3 Add `"crates/spectty-mcp"` to the workspace `members` in root `Cargo.toml`. `[REQ:provisioning-port/spectty-mcp-stub]` `[ci]` `[D16]`
- [x] 1.4 Confirm `crates/core/Cargo.toml` runtime deps UNCHANGED (serde + thiserror only). `[REQ:hexagonal-core/core-no-new-deps]` `[ci]` — DEVIATION: added `serde_json` as a `[dev-dependencies]` (TEST-ONLY) so Core serde round-trip tests have a wire format; the library's runtime closure is still serde+thiserror, so `cargo deny check bans` stays green. Flagged for verify.
- [x] 1.5 Confirm `deny.toml` UNCHANGED (serde_json is adapters/dev-scoped, not in the Core runtime closure). `[REQ:hexagonal-core/cargo-deny-green]` `[ci]`
- [x] **Gate (WU-1)**: `cargo build --workspace` succeeds (empty spectty-mcp main stub OK); `cargo deny --manifest-path crates/core/Cargo.toml check bans` → `bans ok`; `cargo fmt --all -- --check` clean.

---

## WU-2 — Core value types + ClockPort (agent_spec, output_signal, clock, Session grow)  [unit]
**Commit**: `feat(core): add AgentSpec/AgentDescriptor, OutputSignal, ClockPort+Timestamp and grow Session`
**Depends on**: WU-1. **Blocks**: WU-3, WU-4 (registry uses these), WU-5/6/7 (adapters import them).
**Strict TDD**: RED serde round-trip tests first, then the pure value types.
**Rollback**: revert → Core back to M1 entities; nothing imports the new types yet.
**PR slice**: PR1.

- [x] 2.1 RED→GREEN: `agent_spec_round_trips_through_serde` (+ generic-command variant, bare-string AgentKind, descriptor round-trip). `[REQ:agent-runner/agent-spec-value-types → Scenario: AgentSpec round-trips]` `[unit]` `[D12]`
- [x] 2.2 GREEN: create `crates/core/src/entities/agent_spec.rs` — `AgentSpec`, `AgentKind(pub String)` serde-string newtype (NOT a closed enum, D12), `AgentTier { Cooperative, Generic }`, `AgentDescriptor`, `AgentCapabilities { reports_cost, structured_permissions, emits_diff_signals, requires_provisioning }`. NO agent-name literals. `[REQ:agent-runner/agent-spec-value-types]` `[unit]` `[D7][D12]`
- [x] 2.3 GREEN: create `crates/core/src/ports/clock.rs` — `Timestamp(pub u64)` (millis, serde + Ord + `elapsed_ms_until`) and `trait ClockPort: Send + Sync { fn now(&self) -> Timestamp; }`. No `std::time` leak across the boundary. `[REQ:output-signal/non-instant-time]` `[unit]` `[D10]`
- [x] 2.4 RED→GREEN: `output_signal_round_trips_and_carries_no_instant` + `output_signal_constructible_without_pty`. `[REQ:output-signal/non-instant-time]` `[unit]` `[D10]`
- [x] 2.5 GREEN: create `crates/core/src/entities/output_signal.rs` — `OutputSignal { text_window, is_active, exit_code, last_byte_at: Timestamp, idle_ms: u64 }` + `QuickAction { id, label, bytes }` + `CostDelta { input_tokens, output_tokens, estimated_usd }`. All serde. `[REQ:output-signal/non-instant-time]` `[unit]` `[D10]`
- [x] 2.6 RED→GREEN: `session_exposes_m2_fields_and_round_trips`. `[REQ:session-registry/session-aggregate → Scenario: Session exposes id/workspace/agent/status/title/created_at]` `[unit]`
- [x] 2.7 GREEN: GROW `crates/core/src/entities/session.rs` — added `agent: AgentSpec` + `created_at: Timestamp`; kept `id/workspace/status/title`. (CostMetrics skeleton deferred — not yet needed.) NO I/O, no agent name. `[REQ:session-registry/session-aggregate]` `[unit]`
- [x] 2.8 GREEN: re-export the new types from `crates/core/src/lib.rs`; `cargo fmt`; clippy `-D warnings` clean. `[REQ:hexagonal-core/core-no-new-deps]` `[ci]`
- [x] **Gate (WU-2)**: `cargo test --workspace` green (14 new Core tests); `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo deny --manifest-path crates/core/Cargo.toml check bans` → `bans ok`.

---

## WU-3 — Pure AgentStatus state machine (`transition` + `Observed`)  [unit]
**Commit**: `feat(core): add 6-variant AgentStatus and pure transition() with Observed`
**Depends on**: WU-2 (AgentStatus already exists as M0 placeholder; this grows it). **Blocks**: WU-4 (registry calls `transition`), WU-5 (runners return `Observed`).
**Strict TDD**: RED the full legal-transition TABLE first (every `(current, observed)` cell), then implement.
**Rollback**: revert → no state machine; registry not yet present.
**PR slice**: PR1.

- [ ] 3.1 RED: `agent_status_exposes_all_six_variants` — assert `Starting, Idle, Running, AwaitingInput, Completed, Error` all present. `[REQ:agent-status-machine/six-variants → Scenario: all six variants]` `[unit]`
- [ ] 3.2 RED: `transition_running_awaiting_running_round_trip` — `transition(Running, NeedsInput)==AwaitingInput` then `transition(AwaitingInput, Working)==Running` (the permission-prompt round trip, exit-criterion 3). `[REQ:agent-status-machine/pure-transition → Scenario: Running→AwaitingInput→Running]` `[unit]` `[D8]`
- [ ] 3.3 RED: `transition_illegal_jump_leaves_current` — `transition(Starting, Finished)==Starting` (illegal skip ignored). `[REQ:agent-status-machine/pure-transition → Scenario: illegal jump rejected]` `[unit]` `[D8]`
- [ ] 3.4 RED: `transition_any_state_to_error` — for every `current`, `transition(current, Failed)==Error`. `[REQ:agent-status-machine/pure-transition → Scenario: any state may go to Error]` `[unit]`
- [ ] 3.5 RED: `transition_starting_ready_reaches_idle` — `transition(Starting, Ready)==Idle` (exit-criterion 1). `[REQ:agent-status-machine/pure-transition → Scenario: Starting reaches Idle]` `[unit]`
- [ ] 3.6 RED: `transition_terminal_states_are_absorbing` — `Completed`/`Error` stay put for ANY observed. `[REQ:agent-status-machine/pure-transition]` `[unit]` `[D8]`
- [ ] 3.7 GREEN: GROW `crates/core/src/entities/agent_status.rs` — confirm the 6-variant enum; add `enum Observed { Ready, Working, NeedsInput, Finished, Failed }`; implement `#[must_use] pub fn transition(current: AgentStatus, observed: Observed) -> AgentStatus` matching the design §3.4 legal table (terminals absorbing; only changing cells return a new status). Total, deterministic, NO I/O, NO time, NO agent name. `[REQ:agent-status-machine/pure-transition]` `[REQ:agent-status-machine/six-variants]` `[unit]` `[D8]`
- [ ] 3.8 GREEN: re-export `Observed` + `transition` from `lib.rs`. `[REQ:hexagonal-core/core-no-new-deps]` `[ci]`
- [ ] **Gate (WU-3)**: `cargo test --workspace` green (full transition table); fmt/clippy clean; `cargo deny --manifest-path crates/core/Cargo.toml check bans` exits 0.

---

## WU-4 — Core SessionRegistry (&self interior mutability + apply_observed)  [unit]
**Commit**: `feat(core): add SessionRegistry with mint_id, insert, apply_observed, summaries, remove`
**Depends on**: WU-2 (Session/AgentSpec) + WU-3 (`transition`/`Observed`). **Blocks**: WU-9 (src-tauri manages it).
**Strict TDD**: RED registry tests first (mint/lookup/close/apply_observed diff), then the Mutex-backed registry.
**Rollback**: revert → no registry; types stand alone.
**PR slice**: PR1.

- [ ] 4.1 RED: `registry_create_then_lookup_returns_same_session` — mint+insert, look up by id, assert workspace+agent match. `[REQ:session-registry/create-lookup-close → Scenario: create then look up]` `[unit]`
- [ ] 4.2 RED: `registry_close_removes_from_lookup` — remove → subsequent lookup absent. `[REQ:session-registry/create-lookup-close → Scenario: close removes]` `[unit]`
- [ ] 4.3 RED: `registry_mints_distinct_ids_via_shared_ref` — two `mint_id()` through `&self` yield distinct monotonic ids (migrates `next_pty_id`). `[REQ:session-registry/create-lookup-close → Scenario: mints ids via &self]` `[unit]` `[D13]`
- [ ] 4.4 RED: `apply_observed_returns_some_only_on_change` — feed an observation that changes status → `Some(new)`; a legal no-op or terminal-absorbing → `None`. `[REQ:session-registry/create-lookup-close]` `[unit]` `[D19]`
- [ ] 4.5 RED: `registry_holds_no_os_handle` — assert the stored entry shape is `Session` domain state only (compile-level / structural; no `portable-pty`/`tauri` import in core). `[REQ:session-registry/distinct-from-ptyregistry → Scenario: holds no OS handle]` `[unit]`
- [ ] 4.6 GREEN: create `crates/core/src/entities/session_registry.rs` — `SessionRegistry { inner: Mutex<RegistryInner> }`, `RegistryInner { sessions, next_id }`; `mint_id(&self)`, `insert(&self, Session)`, `apply_observed(&self, &SessionId, Observed) -> Option<AgentStatus>` (calls `transition` INSIDE the lock — D19, avoids TOCTOU), `get`, `summaries() -> Vec<SessionSummary>`, `remove`. `SessionSummary { id, title, status, agent_kind }` serde. `[REQ:session-registry/create-lookup-close]` `[REQ:session-registry/distinct-from-ptyregistry]` `[unit]` `[D13][D19]`
- [ ] 4.7 GREEN: re-export `SessionRegistry` + `SessionSummary` from `lib.rs`. `[REQ:hexagonal-core/core-no-new-deps]` `[ci]`
- [ ] **Gate (WU-4)**: `cargo test --workspace` green; fmt/clippy clean; `cargo deny --manifest-path crates/core/Cargo.toml check bans` exits 0 — **Core domain complete and still quarantined.**

---

## WU-5 — AgentRunner port + GenericRunner + ClaudeCodeRunner + registry  [unit]
**Commit**: `feat(adapters): add AgentRunner port impls (Generic + ClaudeCode) with detect_status and launch_spec`
**Depends on**: WU-2 (OutputSignal/AgentSpec) + WU-3 (`Observed`). **Blocks**: WU-9 (resolves runners).
**Strict TDD**: RED detect_status tables + launch_spec equality first (D20 sorted env makes equality clean), then implement.
**Rollback**: revert → no runners; Core ports stand alone.
**PR slice**: PR2.

> The `AgentRunner` TRAIT itself is Core (`crates/core/src/ports/agent_runner.rs`,
> with `LaunchSpec` + `LaunchContext`). Define it here as the first GREEN step (it is
> pure data + a trait, no deps) then implement the two adapters. `detect_status`
> returns `Option<Observed>` (D8), NOT `Option<AgentStatus>`.

- [ ] 5.1 GREEN: create `crates/core/src/ports/agent_runner.rs` — `trait AgentRunner: Send + Sync` with `launch_spec(&self, &LaunchContext) -> LaunchSpec`, `detect_status(&self, &OutputSignal) -> Option<Observed>` (D8), `descriptor(&self) -> AgentDescriptor`, default-skeleton `parse_cost -> None` + `quick_actions -> Vec::new()`; `LaunchSpec { program, args, env: Vec<(String,String)> (sorted, D20), cwd, cols, rows }`; `LaunchContext { cwd, cols, rows, session_id, user_command }`. NO `provisioner()` method (D7/R9). NO agent-name literal. `[REQ:agent-runner/core-port-m2-subset]` `[unit]` `[D7][D8][D20]`
- [ ] 5.2 GREEN: re-export `AgentRunner`/`LaunchSpec`/`LaunchContext` from core `lib.rs`; `cargo deny` must stay green (still serde+thiserror). `[REQ:hexagonal-core/core-no-new-deps]` `[ci]`
- [ ] 5.3 RED: `generic_detect_status_table` — `(exit 0→Finished)`, `(exit≠0→Failed)`, `(is_active→Working)`, `(idle_ms≥timeout→Finished)`, `(quiescent<timeout→Ready)` using injected `OutputSignal.idle_ms` (no wall clock). `[REQ:agent-runner/generic-idle-timeout → Scenario: Completed after idle window]` `[unit]` `[D10]`
- [ ] 5.4 RED: `generic_launch_spec_uses_user_command_or_default_shell` — user_command maps through; absent → per-OS default shell (reuse `config::default_shell`). Exact `LaunchSpec` equality. `[REQ:agent-runner/claude-launch-spec]`(Generic analog) `[unit]` `[D20]`
- [ ] 5.5 GREEN: create `crates/adapters/src/agent/generic.rs` — `GenericRunner { idle_timeout_ms }`; `detect_status` per the table; `launch_spec`; `descriptor` (tier `Generic`, `requires_provisioning: false`). `[REQ:agent-runner/generic-idle-timeout]` `[REQ:agent-runner/agent-spec-value-types → Scenario: AgentTier Generic]` `[unit]` `[D7]`
- [ ] 5.6 RED: `claude_detect_status_patterns` — each `awaiting_input` pattern → `NeedsInput`; each `ready` pattern → `Ready`; `is_active` w/o match → `Working`; no match + inactive → `None`. `[REQ:agent-runner/claude-detect-patterns → Scenario: permission-prompt→AwaitingInput]` `[REQ:agent-runner/claude-detect-patterns → Scenario: unrecognized→None]` `[unit]` `[D11]`
- [ ] 5.7 RED: `claude_launch_spec_program_cwd_env` — program `claude`, `cwd==ctx.cwd`, env carries `SPECTTY_SESSION_ID`. Exact `LaunchSpec` equality. `[REQ:agent-runner/claude-launch-spec → Scenario: maps context to program/cwd/env]` `[unit]` `[D20]`
- [ ] 5.8 GREEN: create `crates/adapters/src/agent/claude_code.rs` — `ClaudeCodeRunner { patterns: ClaudePatterns }` with `awaiting_input`/`ready` as `&'static [&'static str]` DATA (R5/D11, hand-rolled `contains`, NO regex); `detect_status`; `launch_spec`; `descriptor` (tier `Cooperative`, `requires_provisioning: true`, `structured_permissions: false`). `[REQ:agent-runner/claude-detect-patterns]` `[REQ:agent-runner/claude-launch-spec]` `[REQ:agent-runner/agent-spec-value-types → Scenario: AgentTier Cooperative]` `[unit]` `[D11]`
- [ ] 5.9 RED+GREEN: `parse_cost_returns_zero_delta` + `quick_actions_returns_empty` on BOTH runners — assert the honest skeleton contract. `[REQ:agent-runner/skeletons → Scenario: parse_cost returns zero/empty]` `[unit]`
- [ ] 5.10 GREEN: create `crates/adapters/src/agent/mod.rs` — `AgentRunnerRegistry::with_builtin()` mapping `AgentKind` string → `Box<dyn AgentRunner>` (`"claude-code"` / `"generic"`, D12); `resolve(&AgentKind)`. Re-export runners + registry from adapters `lib.rs`. `[REQ:agent-runner/core-port-m2-subset]` `[unit]` `[D12]`
- [ ] **Gate (WU-5)**: `cargo test --workspace` green (detect tables + launch_spec equality + skeletons); fmt/clippy clean; `cargo deny --manifest-path crates/core/Cargo.toml check bans` exits 0 (the AgentRunner trait added NO Core dep, and NO agent name leaked into Core).

---

## WU-6 — OutputSignalProducer (ANSI strip + bounded rolling window)  [unit]
**Commit**: `feat(adapters): add OutputSignalProducer (ANSI-strip rolling window) with pure ingest/snapshot`
**Depends on**: WU-2 (OutputSignal/Timestamp). **Can run in parallel with WU-5.** **Blocks**: WU-9 (signal thread).
**Strict TDD**: RED ANSI-strip + window-truncation tests first, then the pure step machine.
**Rollback**: revert → no producer; runners (WU-5) stand alone.
**PR slice**: PR2.

- [ ] 6.1 RED: `producer_strips_ansi_csi_osc_esc` — feed raw chunks with CSI/OSC/ESC sequences → snapshot `text_window` contains only printable text. `[REQ:output-signal/independent-consumer → Scenario: strips ANSI, bounded window]` `[unit]`
- [ ] 6.2 RED: `producer_window_truncates_from_front_at_window_bytes` — feed > `window_bytes` → window bounded, oldest dropped. `[REQ:output-signal/independent-consumer → Scenario: bounded rolling window]` `[unit]`
- [ ] 6.3 RED: `producer_snapshot_carries_caller_supplied_time_fields` — `snapshot(last_byte_at, idle_ms, is_active, exit_code)` echoes those into the `OutputSignal` (clock owned by caller, D10). `[REQ:output-signal/non-instant-time]` `[unit]` `[D10]`
- [ ] 6.4 GREEN: create `crates/adapters/src/agent/output_signal.rs` — `OutputSignalProducer { window: String, ansi_state: AnsiState, window_bytes }`; `ingest(&mut self, &[u8])` (Ground/Esc/Csi/Osc state machine, append printable, truncate front); `#[must_use] snapshot(&self, Timestamp, u64, bool, Option<i32>) -> OutputSignal`. PURE — no clock, no thread, no I/O. `[REQ:output-signal/independent-consumer]` `[unit]` `[D9][D10]`
- [ ] 6.5 GREEN: re-export `OutputSignalProducer` from adapters `lib.rs`. `[REQ:output-signal/independent-consumer]` `[ci]`
- [ ] **Gate (WU-6)**: `cargo test --workspace` green; fmt/clippy clean; `cargo deny --manifest-path crates/core/Cargo.toml check bans` exits 0.

> NOTE: the bounded **drop-oldest channel** that prevents back-pressure on the M1
> render path (`[REQ:output-signal/independent-consumer → Scenario: cannot back-pressure render]`,
> D9) is a SRC-TAURI wiring concern — the bounded `sync_channel` + `try_send` lives
> in WU-9 (`session_runtime.rs`), tested there. The producer here is the pure half.

---

## WU-7 — Provisioner: pure JSON editor + scope resolver + file-IO seam + ProvisioningPort impl  [unit]
**Commit**: `feat(adapters): add spectty_* JSON namespace provisioner over an atomic ConfigFile seam`
**Depends on**: WU-1 (serde_json) + WU-2 (Core types). **Blocks**: WU-9 (inject/retract wiring).
**Strict TDD**: RED the round-trip / foreign-key-untouched property FIRST (R7 headline), then scope, then file-IO seam, then the adapter.
**Rollback**: revert → no provisioner; runners/producer stand alone.
**PR slice**: PR3.

> The `ProvisioningPort` TRAIT + `ProvisioningScope`/`ProvisioningHandle`/
> `ProvisioningError` are Core (`crates/core/src/ports/provisioning.rs`). Define them
> as the first GREEN step (pure trait + enums + thiserror — no new Core dep), then the
> adapter impl in `crates/adapters/src/provision/`.

- [ ] 7.1 GREEN: create `crates/core/src/ports/provisioning.rs` — `trait ProvisioningPort: Send + Sync { inject(&self, ProvisioningScope) -> Result<ProvisioningHandle, ProvisioningError>; retract(&self, &ProvisioningHandle) -> Result<(), ProvisioningError>; }`; `enum ProvisioningScope { Global, Project(String) }`; `struct ProvisioningHandle { scope }`; `enum ProvisioningError { Io(String), Parse(String) }` (thiserror). SEPARATE from AgentRunner (Lock 1/D7). NO refresh() (M3). NO agent name / config path in Core. Re-export from `lib.rs`. `[REQ:provisioning-port/core-trait-separate → Scenario: inject+retract, distinct trait]` `[unit]` `[D7]`
- [ ] 7.2 RED: `inject_then_retract_round_trips_foreign_keys_byte_identical` — config with a `user` + a `gentle-ai` mcpServers entry: inject spectty → both foreign entries preserved; retract → file byte-identical to pre-inject (the R7 headline property). `[REQ:provisioning-port/json-namespace → Scenario: inject leaves foreign keys untouched]` `[unit]` `[D17]`
- [ ] 7.3 RED: `retract_removes_only_spectty_keys` + `inject_is_idempotent` (double inject == single) + `inject_on_missing_mcpServers_creates_valid_json`. `[REQ:provisioning-port/json-namespace → Scenario: retract removes only spectty_*]` `[REQ:provisioning-port/json-namespace → Scenario: malformed/missing mcpServers handled]` `[unit]` `[D17]`
- [ ] 7.4 GREEN: create `crates/adapters/src/provision/json_namespace.rs` — PURE `inject_spectty_mcp(current_json, server_name, &McpServerEntry) -> Result<String, ProvisioningError>` + `retract_spectty_mcp(current_json, server_name) -> Result<String, ProvisioningError>` over `serde_json::Value`, mutate ONLY the `spectty`-prefixed sub-keys under `mcpServers`, re-serialize pretty. `McpServerEntry { command, args, env }`. NO text markers, NO `claude mcp add`. `[REQ:provisioning-port/json-namespace]` `[unit]` `[D17]`
- [ ] 7.5 RED: `resolve_scope_table` — fake `is_git_tracked`: tracked → `Project(root)` (`.mcp.json`); untracked/absent → `Global` (`~/.claude.json`). `[REQ:provisioning-port/scope-global-default-project-when-tracked → Scenario: git-tracked→PROJECT]` `[REQ:provisioning-port/scope-global-default-project-when-tracked → Scenario: untracked→GLOBAL]` `[unit]` `[D18]`
- [ ] 7.6 GREEN: create `crates/adapters/src/provision/scope.rs` — `resolve_scope(repo_root, config_path, is_git_tracked: impl Fn(&str)->bool) -> ProvisioningScope` (pure); plus the real `is_git_tracked` probe (`git ls-files --error-unmatch`, exit-0→true) kept SEPARATE from the pure resolver. `[REQ:provisioning-port/scope-global-default-project-when-tracked]` `[unit]` `[D18]`
- [ ] 7.7 RED: `first_write_creates_spectty_bak_and_writes_via_temp_rename` + `interrupted_write_leaves_original_intact` against an in-memory `FakeConfigFile`. `[REQ:provisioning-port/atomic-write-backup → Scenario: first write creates .spectty.bak]` `[REQ:provisioning-port/atomic-write-backup → Scenario: crash mid-write never partial]` `[unit]`
- [ ] 7.8 GREEN: create `crates/adapters/src/provision/file_io.rs` — `trait ConfigFile: Send + Sync { read(&self,&str)->io::Result<Option<String>>; write_atomic(&self,&str,&str)->io::Result<()>; }`; `RealConfigFile` (tmp→fsync→rename, `.spectty.bak` on first write); `FakeConfigFile` (HashMap, test-only). `[REQ:provisioning-port/atomic-write-backup]` `[unit]`
- [ ] 7.9 RED: `claude_provisioner_inject_writes_scope_path_and_backs_up` + `retract_absent_file_is_ok` against `FakeConfigFile`. `[REQ:provisioning-port/inject-on-create-retract-on-close]` `[unit]`
- [ ] 7.10 GREEN: create `crates/adapters/src/provision/claude_provisioner.rs` — `ClaudeJsonProvisioner<F: ConfigFile> { files, home_claude_json, mcp_entry }` impl `ProvisioningPort`: `inject` resolves path per scope, read-or-default, `inject_spectty_mcp`, `write_atomic`, return handle; `retract` read→`retract_spectty_mcp`→write (absent = `Ok(())`). `mcp_entry.command` points at the spectty-mcp binary. Re-export from adapters `lib.rs` + `provision/mod.rs`. `[REQ:provisioning-port/inject-on-create-retract-on-close]` `[unit]` `[D14]`
- [ ] **Gate (WU-7)**: `cargo test --workspace` green (round-trip/scope/file-IO/adapter); fmt/clippy clean; `cargo deny --manifest-path crates/core/Cargo.toml check bans` exits 0 (the ProvisioningPort trait added NO Core dep).

---

## WU-8 — spectty-mcp stub binary (stdio JSON-RPC, 5 tool schemas, ack-no-effects)  [unit]
**Commit**: `feat(spectty-mcp): add stdio JSON-RPC stub advertising the 5 Spectty tool schemas`
**Depends on**: WU-1 (crate registered). **INDEPENDENT of Core/adapters — can run in parallel.** **Blocks**: WU-11 (handshake integration test), WU-12 (acceptance points config at it).
**Strict TDD**: RED tools/list 5-name + unknown-call `-32601` tests first (pure request→response handlers), then the stdio loop.
**Rollback**: revert → no stub binary; provisioner's `mcp_entry.command` would point at a missing binary (only matters at runtime acceptance).
**PR slice**: PR4.

- [ ] 8.1 RED: `tools_list_advertises_exactly_five_schemas` — handle a `tools/list` request → response names exactly `spectty_spec, spectty_diff, spectty_approval, spectty_status, spectty_cost` with their declared input schemas (per agent-protocol.md). `[REQ:provisioning-port/spectty-mcp-stub → Scenario: advertises exactly five tool schemas]` `[unit]` `[D15]`
- [ ] 8.2 RED: `tools_call_known_returns_ack_no_effect` + `tools_call_unknown_returns_-32601` + `bad_params_returns_-32602`. `[REQ:provisioning-port/spectty-mcp-stub → Scenario: stub call returns ack, no side effect]` `[unit]` `[D15]`
- [ ] 8.3 RED: `initialize_returns_protocol_and_serverinfo` — `initialize` → `{ protocolVersion, capabilities:{tools:{}}, serverInfo:{name:"spectty-mcp",version} }`. `[REQ:provisioning-port/spectty-mcp-stub]` `[unit]`
- [ ] 8.4 GREEN: implement `crates/spectty-mcp/src/main.rs` — pure request-dispatch fns (`handle_initialize`/`handle_tools_list`/`handle_tools_call`) testable WITHOUT spawning the process, wrapped by a stdio JSON-RPC 2.0 read loop. Known tool → `result { content:[text ack], isError:false }`; unknown → `-32601`; bad params → `-32602`. Schemas FROZEN as the M3-swap contract (R4/D15). serde/serde_json only — NO core, NO tauri (D16). `[REQ:provisioning-port/spectty-mcp-stub]` `[unit]` `[D15][D16]`
- [ ] **Gate (WU-8)**: `cargo test --workspace` green (handler unit tests); fmt/clippy clean; `cargo build -p spectty-mcp` produces the binary. NOTE: stdio framing (line-delimited vs Content-Length) is an OPEN question (design §9) — pin against a real `claude` launch in WU-11/WU-12.

---

## WU-9 — src-tauri: id unification + session commands + status runtime + events  [unit]
**Commit**: `feat(tauri): add session lifecycle commands, status runtime, and bounded signal pipeline`
**Depends on**: WU-4 (SessionRegistry) + WU-5 (runners+registry) + WU-6 (producer) + WU-7 (provisioner). **Blocks**: WU-10 (UI invokes these), WU-11 (real-PTY drives this).
**Strict TDD**: RED `*_impl` free-fn tests against `FakePtyTransport` (M1) + fake `ProvisioningPort` + fake `AgentRunnerRegistry`; RED `observe_and_diff` against a fake runner + real registry; RED bounded-channel drop-oldest. Then wire.
**Rollback**: revert → commands gone; adapters/core compile standalone.
**PR slice**: PR5. **⚠ This slice is the largest — see Review Workload Forecast for the PR5a/PR5b sub-split.**

- [ ] 9.1 GROW `src-tauri/src/pty_state.rs` — `pub type PtyId = String` now sourced from `SessionRegistry::mint_id()`; RETIRE `next_pty_id` (D13). `PtyRegistry` keeps OS handles only; keyed by the SAME string as `SessionRegistry` (lockstep, no cross-map). `[REQ:session-registry/distinct-from-ptyregistry]` `[unit]` `[D13]`
- [ ] 9.2 RED: `observe_and_diff_emits_only_on_change` — fake `AgentRunner` returns observations; real `SessionRegistry`; assert `Some(new)` only when status changes, `None` on no-op / `detect_status` `None`. `[REQ:agent-session-ui/status-event → Scenario: status_changed only on actual change]` `[unit]` `[D8][D19]`
- [ ] 9.3 GREEN: create `src-tauri/src/session_runtime.rs` — pure `observe_and_diff(runner, sessions, id, signal) -> Option<AgentStatus>` (= `runner.detect_status(signal).and_then(|o| sessions.apply_observed(id, o))`). `[REQ:agent-session-ui/status-event]` `[unit]` `[D8]`
- [ ] 9.4 RED: `bounded_signal_channel_drops_oldest_never_blocks` — fill a bounded `sync_channel` (cap N), assert the read-thread `try_send` side never blocks and drops on overflow (the R6 render-protection seam). `[REQ:output-signal/independent-consumer → Scenario: cannot back-pressure render]` `[unit]` `[D9]`
- [ ] 9.5 GREEN: wire the signal thread in `session_runtime.rs` — tee a SECOND bounded `sync_channel` off the M1 read thread (`try_send`+drop, the render `tx` keeps its M1 unbounded behavior), THIRD thread runs `producer.ingest` → `clock.now()` stamp → `producer.snapshot` → `observe_and_diff` → emit `status_changed`; `recv_timeout(QUIESCE)` so idle ticks fire while quiescent (the M1 R3 insight); EOF builds a final signal with `exit_code` set so the terminal status is emitted. `[REQ:output-signal/independent-consumer]` `[REQ:agent-session-ui/status-event]` `[unit]/[manual]` `[D9][D10]`
- [ ] 9.6 RED: `spawn_session_impl_mints_inserts_and_injects_only_when_required` — fake transport + fake provisioner (records inject/retract) + fake runner registry: assert id minted, `inject` called ONLY when `requires_provisioning` (Claude yes, Generic no), registry insert, NO real PTY. `[REQ:provisioning-port/inject-on-create-retract-on-close]` `[REQ:agent-runner/agent-spec-value-types]` `[unit]` `[D7]`
- [ ] 9.7 RED: `close_session_impl_kills_pty_then_retracts_and_removes` — assert PTY kill (M1 path) THEN `retract` for the session's stored scope THEN registry remove. `[REQ:provisioning-port/inject-on-create-retract-on-close → Scenario: close retracts after killing PTY]` `[unit]`
- [ ] 9.8 GREEN: create `src-tauri/src/commands/session.rs` — `spawn_session` (async, owned types: `agent: AgentSpec, workspace_path, title, cols, rows, on_output: Channel<Vec<u8>>` + `State` for SessionRegistry/PtyRegistry/AgentRunnerRegistry/`Arc<dyn ProvisioningPort>`/`Arc<dyn ClockPort>`) following design §6.1 steps 1–10; `close_session`; `list_sessions`; `get_session`. Free `*_impl` fns hold the testable logic. Errors → `String` (M0/M1 convention). `[REQ:agent-session-ui/bridge-commands]` `[unit]` `[D7][D13]`
- [ ] 9.9 GREEN: `StatusChanged { session_id, status, quick_actions }` (`Clone, Serialize`) emitted via v2 `Emitter` ONLY on a real transition change; also `session_created(SessionSummary)` + `session_closed(SessionId)`; `pty_exit` kept from M1. `[REQ:agent-session-ui/status-event → Scenario: payload carries session_id/status/quick_actions]` `[unit]/[manual]`
- [ ] 9.10 GREEN: register the 4 new commands in `generate_handler!` and `.manage(SessionRegistry::default())` + `.manage(AgentRunnerRegistry::with_builtin())` + `.manage::<Arc<dyn ProvisioningPort>>(...)` + `.manage::<Arc<dyn ClockPort>>(SystemClock)` in `lib.rs`; `pub mod session;` in `commands/mod.rs`; `pub mod session_runtime;`. Create `crates/adapters/src/clock.rs` `SystemClock` (impl `ClockPort`) here if not already. `[REQ:agent-session-ui/bridge-commands → Scenario: spawn and close registered]` `[unit]` `[D10]`
- [ ] 9.11 Verify `capabilities/default.json` is UNCHANGED (custom commands + Channel + events ride `core:default`/`core:event:default`, same as M1). `[REQ:agent-session-ui/bridge-commands]` `[ci]/[manual]`
- [ ] **Gate (WU-9)**: `cargo test --workspace` green (impl fakes + observe_and_diff + bounded-channel); fmt/clippy clean; `cargo deny --manifest-path crates/core/Cargo.toml check bans` exits 0; `cargo build -p spectty` succeeds.

---

## WU-10 — UI: session ipc + useSession hook + SpawnDialog + PaneHeader badge  [unit]
**Commit**: `feat(ui): add session spawn flow (useSession + SpawnDialog) and Pane-header status badge`
**Depends on**: WU-9 (commands+events exist for runtime; compiles/tests standalone via vitest mocks). **Blocks**: WU-12.
**Strict TDD**: RED vitest first (mirror `useTerminal`/`usePingPong`): mock `@tauri-apps/api/core` invoke + `@tauri-apps/api/event` listen. Then implement.
**Rollback**: revert → no spawn UI; M1 terminal pane unchanged.
**PR slice**: PR6.

- [ ] 10.1 GREEN: create `ui/src/session/ipc.ts` — typed wrappers `spawnSession(agent, workspacePath, title, cols, rows, onOutput)`, `closeSession(id)`, `listSessions()`, `getSession(id)` + `status_changed`/`session_created`/`session_closed` event types (camel↔snake mapping). `[REQ:agent-session-ui/ui-spawn-and-status]` `[unit]`
- [ ] 10.2 RED: `useSession.test.ts` — `selecting_agent_and_workspace_invokes_spawn` (spawn command invoked with chosen agent+workspace). `[REQ:agent-session-ui/ui-spawn-and-status → Scenario: selecting agent+workspace invokes spawn]` `[unit]`
- [ ] 10.3 RED: `status_changed_updates_badge_and_shows_title` — fire a mocked `status_changed` for the session → badge reflects the new `AgentStatus`, title displayed. `[REQ:agent-session-ui/ui-spawn-and-status → Scenario: Pane header badge updates on status_changed]` `[unit]`
- [ ] 10.4 RED: `close_session_invoked_on_unmount`. `[REQ:agent-session-ui/ui-spawn-and-status]` `[unit]`
- [ ] 10.5 GREEN: create `ui/src/hooks/useSession.ts` — spawn/close orchestration + the 3 event listeners (mirrors `useTerminal`); React 19 named imports; NO manual `useMemo`/`useCallback`. `[REQ:agent-session-ui/ui-spawn-and-status]` `[unit]/[manual]`
- [ ] 10.6 GREEN: create `ui/src/components/SpawnDialog.tsx` — agent radio (Claude Code | Generic + free-text command), cwd text field (avoids a dialog plugin/permission in M2), title field → `spawnSession`. `[REQ:agent-session-ui/ui-spawn-and-status]` `[unit]/[manual]`
- [ ] 10.7 GREEN: create `ui/src/components/PaneHeader.tsx` — `title` + `AgentStatus` badge (Starting=grey, Idle=blue, Running=green-pulse, AwaitingInput=amber-pulse, Completed=grey-check, Error=red), subscribes to `status_changed` filtered by `session_id`. UI NEVER computes status locally (backend authoritative). Wire `<SpawnDialog/>`+`<PaneHeader/>` around the M1 `<Terminal/>` in `App.tsx`. `[REQ:agent-session-ui/ui-spawn-and-status]` `[unit]/[manual]`
- [ ] **Gate (WU-10)**: `pnpm -C ui test` green (useSession suite); `pnpm -C ui build` typechecks (TS strict) and bundles.

---

## WU-11 — Integration tests: real-PTY Generic spawn + spectty-mcp stdio handshake  [unit/ci]
**Commit**: `test(m2): add #[cfg(unix)] real-PTY Generic spawn and spectty-mcp stdio handshake integration`
**Depends on**: WU-9 (runtime) + WU-8 (stub binary). **Blocks**: WU-12 (acceptance leans on these as the automated floor).
**Strict TDD**: these are the INTEGRATION layer (`#[cfg(unix)]`, CI-safe) — they assert the wired pipeline, not new behavior.
**Rollback**: revert → lose the integration floor; unit gates still hold.
**PR slice**: PR5 (real-PTY, with the src-tauri runtime) + PR4 (stdio handshake, with the stub). Split per its dependency.

- [ ] 11.1 `#[cfg(unix)]` `real_pty_generic_reaches_running_then_completed` — spawn a deterministic Generic command (`/bin/sh -c "printf ...; sleep"`), drive the REAL read thread → OutputSignal producer → `detect_status`, assert status reaches `Running` then `Completed` on exit (Generic baseline, exit-criterion 5 in miniature, no wall-clock idle). Lives with WU-9. `[REQ:roadmap-exit/criterion-5]` `[unit/ci]`
- [ ] 11.2 `spectty_mcp_stdio_handshake` — spawn the built `spectty-mcp` binary, send `initialize` + `tools/list` over stdio, assert the 5 tool names come back AND an unknown `tools/call` returns `-32601` (the R4 contract end-to-end). Lives with WU-8. Pin the stdio framing (line-delimited vs Content-Length) here against the real expectation. `[REQ:provisioning-port/spectty-mcp-stub]` `[unit/ci]` `[D15]`
- [ ] **Gate (WU-11)**: `cargo test --workspace` green incl. the `#[cfg(unix)]` integration tests on macOS; fmt/clippy clean; `cargo deny ...` exits 0.

---

## WU-12 — Manual acceptance (roadmap exit gate) + ADR-0004 amendment  [manual]
**Commit**: `docs(m2): record M2 manual acceptance + append ADR-0004 "Superseded for M2+" note`
**Depends on**: ALL prior WUs landed (full slice running). This is the `sdd-verify` pass/fail gate.
**Rollback**: n/a (verification + doc artifact).
**PR slice**: PR7 (verify/doc, ~0 code lines).

> Maps verbatim to the roadmap M2 exit criteria. CANNOT be unit-tested. Run the real
> app on macOS (gating); Windows agent spawn is best-effort and MUST NOT block.
> The Claude Code `AwaitingInput`/permission-prompt patterns (R5) are EMPIRICAL —
> refining a pattern during this run is a one-line DATA edit in `ClaudeCodeRunner`
> + a new unit test, never a Core change.

- [ ] 12.1 Spawn a Claude Code session on a local git repo → status reaches `Idle` (badge blue). `[REQ:roadmap-exit/criterion-1]` `[manual]`
- [ ] 12.2 Inspect `~/.claude.json` (or `.mcp.json` per scope) → the managed `spectty_*` `mcpServers` registration is present, inspectable, and coexists with user/gentle-ai entries. `[REQ:roadmap-exit/criterion-2]` `[REQ:provisioning-port/spectty-mcp-stub → Scenario: injected entry points at stub binary]` `[manual]`
- [ ] 12.3 Give a task → `Running`; hit a permission prompt → `AwaitingInput`; give input → `Running` (validate + refine R5 patterns; add a unit test for any refined pattern). `[REQ:roadmap-exit/criterion-3]` `[manual]`
- [ ] 12.4 Close the session → PTY terminates AND the managed `spectty_*` section is removed (foreign entries intact). `[REQ:roadmap-exit/criterion-4]` `[REQ:provisioning-port/inject-on-create-retract-on-close]` `[manual]`
- [ ] 12.5 Spawn `bash` via Generic → reaches `Idle` → after the configurable inactivity window → `Completed`. `[REQ:roadmap-exit/criterion-5]` `[REQ:agent-runner/generic-idle-timeout]` `[manual]`
- [ ] 12.6 (best-effort, ungated) Windows agent-spawn smoke if a Windows host is available — failure does NOT block M2. `[REQ:cross-platform/macos-gating-windows-best-effort]` `[manual]`
- [ ] 12.7 DOC: append a short "Superseded for M2+" note to ADR-0004 and `agent-abstraction.md` pointing at the separate `ProvisioningPort` (D7 — the ADR's agent-agnostic intent is preserved; only the mechanism moved from an `AgentRunner::provisioner()` method to a sibling Core port). The CODE is the source of truth. `[REQ:agent-runner/core-port-m2-subset]` `[D7]`
- [ ] 12.8 VERIFY-FLAG: confirm the R8 deferral (no boot-time orphan sweep; `.spectty.bak` + idempotent retract as the escape hatch) is recorded as a conscious, documented deferral for `sdd-verify` and carried to M3. `[REQ:provisioning-port/atomic-write-backup]` `[D14]`
- [ ] **Gate (WU-12)**: all macOS criteria (12.1–12.5) pass → M2 acceptance PASS; record results for `sdd-verify`. Windows (12.6) informational only.

---

## Cross-cutting gates (apply to every code WU)
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo deny --manifest-path crates/core/Cargo.toml check bans` (Core quarantine stays green — NO new Core dep, NO agent name in Core)
- `pnpm -C ui test` (= `vitest run`) and `pnpm -C ui build` (UI WUs)
- VibeLens: after edits in a WU, call `show_diff_explanation` with that WU's `git diff HEAD`.

---

## Review Workload Forecast

**Estimated changed lines per WU** (additions + deletions, approximate, excluding lockfile churn):
- WU-1 manifests + empty stub main: ~30
- WU-2 Core types (agent_spec/output_signal/clock/Session grow) + serde tests: ~200
- WU-3 transition machine + full table tests: ~180
- WU-4 SessionRegistry + tests: ~190
- WU-5 AgentRunner port + 2 runners + registry + tables: ~340
- WU-6 OutputSignalProducer + ANSI tests: ~170
- WU-7 provisioner (json+scope+file_io+adapter+port) + tests: ~400
- WU-8 spectty-mcp stub + handler tests: ~230
- WU-9 src-tauri commands+runtime+events+fakes+id-unify: ~470
- WU-10 UI ipc+hook+SpawnDialog+PaneHeader+tests: ~330
- WU-11 integration tests (real-PTY + stdio): ~120
- WU-12 docs/acceptance (ADR note): ~30
- **Total reviewable: ~2690 changed lines** (≫ 400 — chaining mandatory).

`Chained PRs recommended: Yes`
`400-line budget risk: High`
`Estimated total changed lines: 2690`
`Proposed PR count: 7` (one-line title + estimate each):
- PR1 — Core domain (types + ClockPort + transition + SessionRegistry) [WU-1+WU-2+WU-3+WU-4] — ~600 lines. **>400**: stacked sub-split available → PR1a Core value types + clock [WU-1/WU-2] ~230 / PR1b transition + SessionRegistry [WU-3/WU-4] ~370.
- PR2 — AgentRunner port + runners + OutputSignal producer [WU-5+WU-6] — ~510 lines. **>400**: sub-split → PR2a runners [WU-5] ~340 / PR2b producer [WU-6] ~170.
- PR3 — Provisioner (ProvisioningPort + JSON editor + scope + file-IO + adapter) [WU-7] — ~400 lines. At budget; keep single, watch on apply.
- PR4 — spectty-mcp stub + stdio handshake integration [WU-8 + WU-11.2] — ~280 lines.
- PR5 — src-tauri session bridge (commands + runtime + events + id-unify + real-PTY) [WU-9 + WU-11.1] — ~590 lines. **>400**: sub-split → PR5a runtime + signal pipeline + id-unify [WU-9.1–9.5 + WU-11.1] ~300 / PR5b session commands + events + registration [WU-9.6–9.11] ~290.
- PR6 — UI session spawn flow + status badge [WU-10] — ~330 lines.
- PR7 — M2 manual acceptance + ADR-0004 amendment [WU-12] — ~30 lines (verify/doc).

`Decision needed before apply: Yes` — delivery is **chained-PRs, stacked-to-main**; 3 slices (PR1, PR2, PR5) exceed the 400-line budget and carry pre-planned stacked sub-splits (PR1a/b, PR2a/b, PR5a/b). The orchestrator's Review Workload Guard should confirm whether to ship the 7 top-level PRs (accepting PR1/PR2/PR5 at ~510–600 with a recorded `size:exception` each) OR the 10-PR fully-split chain (every slice ≤400). Recommended: the **10-PR fully-split** chain to hold the ≤400 / ≤60-min budget cleanly.
