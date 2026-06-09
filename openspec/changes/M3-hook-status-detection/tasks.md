# M3 — Hook-Based Status Detection — Task Checklist

> **STATUS: IN PROGRESS.** PR-1a (WU-1 + WU-2 + WU-3) COMPLETE. PR-1b (WU-4 + WU-5 + WU-6) COMPLETE. PR-2 (WU-7 + WU-8) COMPLETE. PR-2 adversarial fixes (C1/C2/C3/W1/W2) COMPLETE (227 tests). WU-9/10/11 pending.
>
> SDD tasks phase. Consumes `sdd/M3-hook-status-detection/spec` (obs #830) +
> `openspec/changes/M3-hook-status-detection/specs/*` and
> `sdd/M3-hook-status-detection/design` (obs #829) +
> `openspec/changes/M3-hook-status-detection/design.md` (ADRs D21–D25).
> Artifact store: HYBRID (engram `sdd/M3-hook-status-detection/tasks` + this file).
> Format follows the archived M2 tasks.md.
>
> **Design is authoritative over spec where they differ.** The spec uses `--status
> <STATUS>` / Working/Ready/etc strings; the design (D22/§3.6) resolves to `--event
> <Name>` / HookEvent names (Submit/Stop/Permission/SessionEnd/StopFailure) stored in
> the state file as an `event` field. Tasks follow the DESIGN throughout.
>
> **Strict TDD is ACTIVE.** Test runners: `cargo test --workspace` (Rust),
> `pnpm --filter ui test` (= `vitest run`, TS). Every code work unit pairs its RED
> test with its GREEN implementation in the SAME unit: write the failing test first,
> then make it pass, then refactor. Do NOT batch tests at the end.
>
> **Per-WU gate commands** (the M3 set):
> - `cargo fmt --all -- --check`
> - `cargo clippy --workspace --all-targets -- -D warnings`
> - `cargo test --workspace`
> - `cargo deny --manifest-path crates/core/Cargo.toml check bans` — Core quarantine
>   MUST stay green at EVERY WU (the hard invariant: `crates/core` gains NOTHING in M3).
> - `pnpm --filter ui test` (= `vitest run`) and `pnpm --filter ui build` (UI WUs only).
> - VibeLens: after edits in a WU, call `show_diff_explanation` with that WU's
>   `git diff HEAD` (per project CLAUDE.md) — apply-phase obligation, not a commit.
>
> **Spec traceability tag** per task: `[REQ:<capability>/<short>]` maps to a spec
> Requirement. Verification class carried from spec: `[unit]` / `[manual]` / `[ci]`.
> **Design tag** `[D#]` references the ADR that fixes the decision.
>
> **Dependency rule (the spine)**: workspace manifests → pure adapter fns (namespace +
> hook/state) → sidecar binary (independent, mirrors spectty-mcp) → provisioner adapter
> → hook reader → run_signal_loop augmentation → spawn/close lifecycle wiring →
> Slice 2 additive events. Core is UNTOUCHED throughout.
>
> **Slice 1** = Stop + UserPromptSubmit (the primary regression fix for "stuck Running").
> **Slice 2** = Permission/Notification + SessionEnd + StopFailure (additive).

```
WU-1 (manifests) ──────────────────────────────────────────────────────┐
     │                                                                  │
     ├── WU-2 (json_namespace: inject/retract_spectty_hooks + shape)   │
     │         (PURE, can run immediately after WU-1)                  │
     │                                                                  │
     ├── WU-3 (hook/state: HookEvent + HookState + parse_state_file +  │
     │         event_to_observed + settings_path_for_scope — PURE)     │
     │         (PURE, can run in parallel with WU-2)                   │
     │                                                                  │
     ├── WU-4 (spectty-hook sidecar binary — INDEPENDENT)              │
     │         (parallel with WU-2/3)                                  │
     │                                                                  │
     ├── WU-2 done ──▶ WU-5 (ClaudeSettingsProvisioner + scope path)  │
     │                        (needs WU-2 namespace fns + WU-3 scope) │
     │                                                                  │
     ├── WU-3 done ──▶ WU-6 (StateFileReader consume-once — WU-3 dep) │
     │                                                                  │
     ├── WU-5 + WU-6 done ──▶ WU-7 (run_signal_loop hook poll augment)│
     │                                                                  │
     ├── WU-5 + WU-6 done ──▶ WU-8 (spawn/close lifecycle + PtyState) │
     │                                (WU-7 can land in same PR)       │
     │                                                                  │
     ├── WU-4 + WU-7 + WU-8 done ──▶ WU-9 (integration: path         │
     │                              agreement + real-PTY hook→status)  │
     │                                                                  │
     ├── ALL done ──▶ WU-10 (Slice 2: Permission+SessionEnd+StopFail)  │
     │                                                                  │
     └── WU-10 done ──▶ WU-11 (manual acceptance + ADR-D21-D25 note)  │
```

---

## WU-1 — Workspace manifests + spectty-hook member  [ci]
**Commit**: `chore(deps): register crates/spectty-hook workspace member and declare externalBin`
**Depends on**: nothing. **Blocks**: WU-2/3/4 (the new crate needs to be in the workspace).
**Rollback**: revert → no new crate; M2 still builds.
**PR slice**: PR-1 (Slice 1 foundation).

- [x] 1.1 Create `crates/spectty-hook/Cargo.toml` — `[[bin]] name = "spectty-hook"`, deps
  `serde` + `serde_json` ONLY (NO `spectty-core`, NO `tauri`, mirrors `crates/spectty-mcp/Cargo.toml`).
  `[REQ:spectty-hook-sidecar/atomic-write]` `[ci]` `[D25]`
- [x] 1.2 Add `"crates/spectty-hook"` to the workspace `members` list in root `Cargo.toml`.
  `[REQ:bundling/externalBin]` `[ci]` `[D25]`
- [ ] 1.3 Add `bundle.externalBin` to `src-tauri/tauri.conf.json` declaring BOTH
  `"binaries/spectty-mcp"` AND `"binaries/spectty-hook"` (target-triple-suffixed per Tauri sidecar
  convention). This closes M2 L2 for spectty-mcp and establishes bundling for spectty-hook.
  `[REQ:bundling/externalBin → Scenario: tauri.conf.json contains both sidecar entries]` `[ci]` `[D25]`
  **DEFERRED (adversarial review W3)**: sidecar bundling (`externalBin` + real per-triple binaries
  generated in CI) is out of WU-4 scope. Empty committed stubs were a footgun (0-byte files, only one
  target triple, clobber real local builds on checkout). Reverted in fix(spectty-hook): C1/W1/W3/W4.
  This task belongs in a dedicated packaging work unit, not WU-4.
- [x] 1.4 Confirm `crates/core/Cargo.toml` runtime deps UNCHANGED (serde + thiserror only).
  `[REQ:hook-provisioning/ClaudeSettingsProvisioner — CORE UNCHANGED]` `[ci]` `[D21]`
- [x] **Gate (WU-1)**: `cargo build --workspace` succeeds (empty spectty-hook main stub OK);
  `cargo deny --manifest-path crates/core/Cargo.toml check bans` → `bans ok`;
  `cargo fmt --all -- --check` clean.

---

## WU-2 — Pure JSON namespace: inject/retract_spectty_hooks + HookCommandEntry  [unit]
**Commit**: `feat(adapters): add inject/retract_spectty_hooks for settings.json hooks key`
**Depends on**: WU-1 (serde_json already in adapters from M2; this just adds new pure fns to
`json_namespace.rs`). **Blocks**: WU-5 (ClaudeSettingsProvisioner calls these fns).
**Can run in parallel with WU-3 and WU-4.**
**Strict TDD**: RED the headline R7-generalized round-trip property FIRST — hand-formatted
`settings.json` with foreign `permissions`/`env`/`model` keys AND a foreign user hook on the
SAME event; inject→retract must preserve every foreign value + key order and leave no Spectty row.
**Rollback**: revert → no hook namespace fns; M2 `json_namespace.rs` unchanged.
**PR slice**: PR-1.

> R7 GENERALIZED (D21): `hooks` is more deeply nested than `mcpServers`. The owned-key
> predicate changes from a NAMED key (`"spectty"` under `mcpServers`) to ROWS whose inner
> `hooks[].command == our sidecar path`. Retract removes only those rows. Same
> `preserve_order` / VALUE+ORDER (not byte-identity) contract as M2.

- [x] 2.1 RED: `inject_spectty_hooks_round_trip_preserves_foreign_keys_and_order` — HAND-FORMATTED
  `settings.json` with `permissions`, `env`, `model`, a user hook on `Stop` (foreign row on same
  event), a user hook on a different event; inject Spectty Stop+Submit hooks → retract → every
  foreign VALUE and relative key ORDER preserved, no Spectty row remains, foreign `Stop` hook
  survives. RED proven by breaking the owned-key predicate. `[REQ:hook-provisioning/ClaudeSettingsProvisioner
  → Scenario: inject adds managed hook entries and leaves foreign keys untouched]` `[unit]` `[D21]`
- [x] 2.2 RED: `retract_spectty_hooks_removes_only_owned_rows` — settings with BOTH Spectty-owned
  rows (command == our binary) and user-authored rows on the same event; retract removes only
  Spectty rows; foreign rows untouched. `[REQ:hook-provisioning/ClaudeSettingsProvisioner
  → Scenario: retract removes only Spectty-managed hook entries]` `[unit]` `[D21]`
- [x] 2.3 RED: `inject_spectty_hooks_on_empty_document_creates_hooks_key` — inject into `{}` →
  valid JSON with `hooks` object, no other key created. `[REQ:hook-provisioning/ClaudeSettingsProvisioner
  → Scenario: Editing absent or empty hooks section creates valid output]` `[unit]`
- [x] 2.4 RED: `inject_spectty_hooks_is_idempotent` — double inject == single inject
  (no duplicate rows). `[unit]`
- [x] 2.5 RED: `retract_spectty_hooks_on_file_with_no_spectty_rows_is_idempotent` — retract on
  file already clean returns same structure. `[REQ:hook-provisioning/ClaudeSettingsProvisioner
  → Scenario: retract on settings.json that has no Spectty hooks is idempotent]` `[unit]`
- [x] 2.6 RED: `inject_spectty_hooks_no_matcher_events_have_no_matcher_field` — inspect
  `Stop` and `UserPromptSubmit` entries in the output; neither has a `matcher` field (absent, not
  null). `[REQ:hook-status-mapping/hook-event-shape → Scenario: No-matcher events have no
  matcher field]` `[unit]`
- [x] 2.7 RED: `non_object_hooks_is_a_parse_error_not_data_loss` — pre-existing `"hooks": []` →
  `ProvisioningError::Parse`, never clobbered (mirrors M2 `non_object_mcp_servers` test).
  `[unit]`
- [x] 2.8 GREEN: modify `crates/adapters/src/provision/json_namespace.rs` — add
  `pub struct HookCommandEntry { pub command: String, pub args: Vec<String>, pub matcher:
  Option<String> }` and pure `inject_spectty_hooks(current_json: &str, events: &[(String,
  HookCommandEntry)]) -> Result<String, ProvisioningError>` + `retract_spectty_hooks(current_json:
  &str, hook_command: &str) -> Result<String, ProvisioningError>`. Owned-key predicate:
  rows whose `hooks[].command == hook_command`. Re-serialize with `preserve_order`.
  NO new deps (serde_json already in adapters). `[REQ:hook-provisioning/ClaudeSettingsProvisioner]`
  `[unit]` `[D21]`
- [x] 2.9 GREEN: export `inject_spectty_hooks` + `retract_spectty_hooks` + `HookCommandEntry` from
  `crates/adapters/src/provision/mod.rs` + adapters `lib.rs`. `[ci]`
- [x] **Gate (WU-2)**: `cargo test --workspace` green (all 7 new namespace tests); fmt clean;
  clippy `-D warnings` clean; `cargo deny --manifest-path crates/core/Cargo.toml check bans` →
  `bans ok`.

---

## WU-3 — Pure hook semantics: HookEvent + HookState + parse_state_file + event_to_observed + settings_path_for_scope  [unit]
**Commit**: `feat(adapters): add hook/state pure types, parse_state_file, event_to_observed, settings_path_for_scope`
**Depends on**: WU-1 (workspace member exists). **Can run in parallel with WU-2 and WU-4.**
**Blocks**: WU-5 (provisioner uses `settings_path_for_scope`), WU-6 (reader uses `HookEvent`/
`HookState`/`parse_state_file`).
**Strict TDD**: RED the event_to_observed table and parse_state_file error cases first.
**Rollback**: revert → no hook types; nothing imports them yet.
**PR slice**: PR-1.

> All functions in this WU are PURE (no I/O, no file system). They are the table-test
> surface that pins the spec's §3.4 mapping and the consume-once contract (D22). Note:
> `settings_path_for_scope` lives here (separate from the hook reader logic) as a pure
> scalar fn that tests without FS access.

- [x] 3.1 RED: `event_to_observed_table` — assert all 5 events map correctly:
  Submit→Working, Stop→Ready, Permission→NeedsInput, SessionEnd→Finished, StopFailure→Failed.
  RED proven by swapping two entries. `[REQ:hook-status-mapping/five-hook-events
  → Scenarios: "Ready" maps ... "Working" maps ... etc]` `[unit]`
- [x] 3.2 RED: `parse_state_file_round_trips_all_events` — valid JSON with each of the 5
  `HookEvent` names → correct `HookState`. `[REQ:pipeline-augmentation/run_signal_loop
  → Scenario: A malformed state file is silently ignored]` `[unit]`
- [x] 3.3 RED: `parse_state_file_returns_parse_error_on_malformed_json` + unknown event name
  → `Err(ProvisioningError::Parse)`. `[unit]`
- [x] 3.4 RED: `settings_path_for_scope_global` — `Global` → `~/.claude/settings.json`
  (HOME-expanded). `[REQ:hook-provisioning/scope-path-resolver → Scenario: Global scope resolves
  to ~/.claude/settings.json]` `[unit]`
- [x] 3.5 RED: `settings_path_for_scope_project` — `Project("/some/repo")` →
  `/some/repo/.claude/settings.json`. `[REQ:hook-provisioning/scope-path-resolver → Scenario:
  Project scope resolves to {root}/.claude/settings.json]` `[unit]`
- [x] 3.6 GREEN: create `crates/adapters/src/hook/state.rs` —
  ```
  #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
  pub enum HookEvent { Submit, Stop, Permission, SessionEnd, StopFailure }
  pub struct HookState { pub event: HookEvent, pub ts: u64, pub session_id: String }
  pub fn parse_state_file(json: &str) -> Result<HookState, ProvisioningError>;
  pub fn event_to_observed(event: HookEvent) -> Observed;
  ```
  All PURE — no I/O. `[REQ:hook-status-mapping/five-hook-events]` `[unit]` `[D22]`
- [x] 3.7 GREEN: add `pub fn settings_path_for_scope(scope: &ProvisioningScope) -> String` to
  `crates/adapters/src/provision/scope.rs` — Global → `{$HOME}/.claude/settings.json`; Project(root)
  → `{root}/.claude/settings.json`. DISTINCT from M2's `resolve_scope` (which maps to `.claude.json`/
  `.mcp.json`). `[REQ:hook-provisioning/scope-path-resolver]` `[unit]` `[D21]`
- [x] 3.8 GREEN: create `crates/adapters/src/hook/mod.rs` — re-exports; create
  `crates/adapters/src/hook/` directory. Export from adapters `lib.rs`. `[ci]`
- [x] **Gate (WU-3)**: `cargo test --workspace` green (all 5 new pure tests); fmt clean;
  clippy `-D warnings` clean; `cargo deny ... check bans` → `bans ok`.

---

## WU-4 — spectty-hook sidecar binary  [unit]
**Commit**: `feat(spectty-hook): add standalone hook sidecar binary (--event, SPECTTY_SESSION_ID, monotonic ts)`
**Depends on**: WU-1 (crate registered). **INDEPENDENT of Core/adapters — can run in parallel
with WU-2 and WU-3.** **Blocks**: WU-9 (integration test spawns the built binary).
**Strict TDD**: RED the pure arg-parse + write-state handler first; then the stdio-drain loop.
**Rollback**: revert → no sidecar binary; nothing else breaks.
**PR slice**: PR-1.

> The sidecar accepts `--event <Name>` (design §3.6), reads `$SPECTTY_SESSION_ID` from
> env, resolves the runtime dir, reads the prior state file for its `ts` (default 0),
> writes `{event, ts: prior+1, session_id}` atomically (`.tmp` → rename). It NEVER
> parses Claude's stdin hook JSON (D23). The runtime-dir resolver (~10 lines) is
> duplicated here from src-tauri (D25); the exact same path is asserted by integration
> test 9 (WU-9.1).
>
> DESIGN NOTE: The design's CLI flag is `--event <Name>` with HookEvent names
> (Submit/Stop/Permission/SessionEnd/StopFailure). This is what the hooks injected by
> ClaudeSettingsProvisioner will pass via `args: ["--event", "<Name>"]`.

- [x] 4.1 RED: `spectty_hook_handle_event_writes_state_file` (pure handler test, no process) —
  call the pure `handle_event(event, session_id, read_prior, write_atomic)` with a fake read
  returning `Some(ts=6)` → write receives `{event:Stop, ts:7, session_id}`. Absent prior file
  (None) → ts=1. `[REQ:spectty-hook-sidecar/atomic-write → Scenario: spectty-hook writes a
  valid state file from env + args]` `[unit]`
- [x] 4.2 RED: `spectty_hook_unknown_event_returns_error` — handler called with an unrecognized
  event name string → `Err` (maps to non-zero exit in main). `[REQ:spectty-hook-sidecar/atomic-write
  → Scenario: spectty-hook with unknown status arg exits non-zero]` `[unit]`
- [x] 4.3 RED: `spectty_hook_rejects_missing_session_id` — absent `SPECTTY_SESSION_ID` → main
  exits non-zero without writing. `[REQ:spectty-hook-sidecar/atomic-write → Scenario: spectty-hook
  exits non-zero when SPECTTY_SESSION_ID is absent]` `[unit]`
- [x] 4.4 RED: `spectty_hook_rejects_missing_runtime_dir` — non-existent runtime dir →
  non-zero exit. `[REQ:spectty-hook-sidecar/atomic-write → Scenario: spectty-hook exits non-zero
  when the runtime dir does not exist]` `[unit]`
- [x] 4.5 RED: `spectty_hook_accepts_all_five_event_names` — table test: Submit, Stop,
  Permission, SessionEnd, StopFailure each produce a valid write call (no error). `[REQ:spectty-hook-sidecar/five-valid-events
  → Scenario: Each valid status value writes a state file]` `[unit]`
- [x] 4.6 GREEN: create `crates/spectty-hook/src/runtime_dir.rs` — `fn spectty_runtime_dir()
  -> Option<PathBuf>` (~10-line duplicate of src-tauri's resolver, D25): resolves to a
  Spectty-specific subdirectory under the OS app-local-data dir (must match src-tauri's
  `spectty_runtime_dir()` exactly — pinned by WU-9.1 integration test).
- [x] 4.7 GREEN: create `crates/spectty-hook/src/main.rs` — parse `--event <Name>` arg;
  read `SPECTTY_SESSION_ID` env (exit non-zero if absent); resolve `spectty_runtime_dir()` (exit
  non-zero if absent); call `handle_event(event, session_id, read_prior, write_atomic)`; drain
  and ignore stdin (Claude passes hook JSON); exit 0 on success, non-zero on error.
  serde/serde_json only — NO spectty-core, NO tauri (D25). `[REQ:spectty-hook-sidecar/atomic-write]`
  `[unit]` `[D23][D25]`
- [x] **Gate (WU-4)**: `cargo test --workspace` green (handler unit tests); fmt/clippy clean;
  `cargo build -p spectty-hook` produces the binary.

---

## WU-5 — ClaudeSettingsProvisioner + settings_path_for_scope wiring  [unit]
**Commit**: `feat(adapters): add ClaudeSettingsProvisioner (2nd ProvisioningPort impl for settings.json)`
**Depends on**: WU-2 (`inject/retract_spectty_hooks` + `HookCommandEntry`) + WU-3
(`settings_path_for_scope`, `HookEvent`). **Blocks**: WU-7/8 (lifecycle wiring).
**Strict TDD**: RED `FakeConfigFile` tests for the provisioner first (same seam as M2
`ClaudeJsonProvisioner`).
**Rollback**: revert → no second provisioner; M2 provisioner unchanged.
**PR slice**: PR-1.

> SECOND `ProvisioningPort` impl (D21). The Core `ProvisioningPort` trait is UNCHANGED.
> `ClaudeSettingsProvisioner<F: ConfigFile>` manages ONLY the `hooks` key in
> settings.json — it MUST NOT touch `mcpServers`, `permissions`, `env`, `model`.
> The composition root will manage it as a second `Arc<dyn ProvisioningPort>`.

- [x] 5.1 RED: `claude_settings_provisioner_inject_writes_correct_scope_path_and_backs_up` —
  fake `FakeConfigFile`: Global scope → writes `~/.claude/settings.json` + creates
  `.spectty.bak`; Project scope → writes `{root}/.claude/settings.json`. `[REQ:hook-provisioning/
  ClaudeSettingsProvisioner → Scenario: First write creates a .spectty.bak backup]` `[unit]` `[D21]`
- [x] 5.2 RED: `claude_settings_provisioner_second_write_does_not_overwrite_bak` — inject twice;
  `.spectty.bak` from first write preserved. `[REQ:hook-provisioning/ClaudeSettingsProvisioner
  → Scenario: Subsequent writes do not overwrite an existing .spectty.bak]` `[unit]`
- [x] 5.3 RED: `claude_settings_provisioner_retract_absent_file_is_ok` — retract when no
  settings.json exists → `Ok(())`. `[unit]`
- [x] 5.4 RED: `claude_settings_provisioner_retract_removes_only_spectty_rows_via_fake` — inject then
  retract via `FakeConfigFile`; no Spectty rows remain; foreign keys untouched (indirectly asserted
  via the `FakeConfigFile`'s written content). `[REQ:hook-provisioning/ClaudeSettingsProvisioner
  → Scenario: retract removes only Spectty-managed hook entries]` `[unit]`
- [x] 5.5 RED: `claude_settings_provisioner_implements_provisioning_port_without_trait_change` —
  compile-time: `fn takes_port(_: &dyn ProvisioningPort) {}; takes_port(&provisioner)`.
  `[REQ:hook-provisioning/ClaudeSettingsProvisioner → Scenario: implements ProvisioningPort
  without trait change]` `[unit]` `[D21]`
- [x] 5.6 GREEN: create `crates/adapters/src/provision/settings_provisioner.rs` —
  `pub struct ClaudeSettingsProvisioner<F: ConfigFile> { files: F, home_claude_settings: String,
  hook_command: String, events: Vec<(String, HookCommandEntry)> }` impl `ProvisioningPort`:
  `inject` resolves path via `settings_path_for_scope`, read-or-default `{}`,
  `inject_spectty_hooks`, `write_atomic`, return handle; `retract` read→`retract_spectty_hooks`→
  write (absent = `Ok(())`). `hook_command` is the resolved `spectty_hook_command()` path
  embedded in each `HookCommandEntry`. `[REQ:hook-provisioning/ClaudeSettingsProvisioner]`
  `[unit]` `[D21]`
- [x] 5.7 GREEN: export `ClaudeSettingsProvisioner` from `crates/adapters/src/provision/mod.rs` +
  adapters `lib.rs`. `[ci]`
- [x] **Gate (WU-5)**: `cargo test --workspace` green (5 provisioner tests + all prior); fmt clean;
  clippy `-D warnings` clean; `cargo deny ... check bans` → `bans ok`.

---

## WU-6 — StateFileReader (consume-once monotonic counter)  [unit]
**Commit**: `feat(adapters): add StateFileReader with consume-once ts predicate (D22)`
**Depends on**: WU-3 (`HookEvent`, `HookState`, `parse_state_file`). **Blocks**: WU-7
(`run_signal_loop` gains `StateFileReader` param).
**Strict TDD**: RED the consume-once ts predicate first (same-ts → None; strictly-greater → Some;
absent file → None; older → None).
**Rollback**: revert → no reader; WU-3 pure types stand alone.
**PR slice**: PR-1 (can land in same PR as WU-5).

> D22 RESOLVED: consume-once via SIDECAR-OWNED MONOTONIC COUNTER `ts`. The
> `StateFileReader` owns `last_ts: Option<u64>` (initialized to `None` = 0 effective).
> `poll` returns `Some(event)` ONLY when `state.ts > last_ts_or_zero`, then advances
> `last_ts`. This is the central correctness seam — pinned by the consume-once tests.

- [x] 6.1 RED: `state_file_reader_first_poll_with_ts_1_returns_some` — injected read closure returns
  `{"event":"Stop","ts":1,"session_id":"x"}`; last_ts = None → poll returns `Some(Stop)`, last_ts
  becomes `Some(1)`. `[REQ:pipeline-augmentation/run_signal_loop → Scenario: A new state file event
  triggers one Observed emission]` `[unit]` `[D22]`
- [x] 6.2 RED: `state_file_reader_same_ts_second_poll_returns_none` — poll twice with same content
  (ts=1); second call → None. `[REQ:pipeline-augmentation/run_signal_loop → Scenario: Same ts is not
  re-emitted on a subsequent tick]` `[unit]` `[D22]`
- [x] 6.3 RED: `state_file_reader_newer_ts_fires_again` — after consuming ts=1, read returns ts=2 →
  Some(new event). `[REQ:pipeline-augmentation/run_signal_loop → Scenario: A newer ts supersedes
  without re-emitting the old one]` `[unit]` `[D22]`
- [x] 6.4 RED: `state_file_reader_absent_file_returns_none` — read closure returns `Ok(None)` →
  poll returns None (normal — no hook fired yet). `[REQ:pipeline-augmentation/run_signal_loop
  → Scenario: An absent state file on a tick is silently ignored]` `[unit]`
- [x] 6.5 RED: `state_file_reader_older_ts_returns_none` — `last_ts = Some(7)`, read returns ts=6 →
  None (stale, do not re-fire). `[unit]` `[D22]`
- [x] 6.6 RED: `state_file_reader_malformed_json_returns_none` — read returns bad JSON → None (not
  an error to the caller). `[REQ:pipeline-augmentation/run_signal_loop → Scenario: A malformed
  state file is silently ignored]` `[unit]`
- [x] 6.7 GREEN: create `crates/adapters/src/hook/reader.rs` —
  `pub struct StateFileReader { path: String, last_ts: Option<u64> }`
  `impl StateFileReader { pub fn new(runtime_dir: &str, session_id: &str) -> Self;
    pub fn poll(&mut self, read: &dyn Fn(&str) -> std::io::Result<Option<String>>) -> Option<HookEvent>; }`
  Consume-once predicate: `ts > self.last_ts.unwrap_or(0)`. `[REQ:pipeline-augmentation/
  run_signal_loop]` `[unit]` `[D22]`
- [x] 6.8 GREEN: re-export `StateFileReader` from `crates/adapters/src/hook/mod.rs` + adapters
  `lib.rs`. `[ci]`
- [x] **Gate (WU-6)**: `cargo test --workspace` green (6 new reader tests + all prior); fmt clean;
  clippy clean; `cargo deny ... check bans` → `bans ok`.

---

## WU-7 — run_signal_loop augmentation (StateFileReader poll, hook-first per tick)  [unit]
**Commit**: `feat(tauri): augment run_signal_loop with StateFileReader poll (hook-first, D24)`
**Depends on**: WU-5 (provisioner exists; conceptual dep) + WU-6 (`StateFileReader`).
**Blocks**: WU-8 (spawn must wire the reader to the loop).
**Can be shipped in the SAME PR as WU-8.**
**Strict TDD**: RED the hook-precedes-PTY-scraping behavior and the no-double-emit property first
(scripted reader + scripted runner).
**Rollback**: revert → M2 `run_signal_loop` unchanged; scraping-only.
**PR slice**: PR-2.

> D24: hooks AUGMENT scraping. Hook event goes through `observe_and_diff` BEFORE the PTY
> observation on the same tick. Double-emit is impossible because `observe_and_diff` only
> emits on an ACTUAL status change (a no-op second observation returns None). `detect_status`
> stays pure PTY-only — it MUST NOT be touched here.

- [x] 7.1 RED: `run_signal_loop_hook_stop_from_running_emits_idle` — scripted `StateFileReader`
  (fake `read` returning `{Stop, ts:1}`) + scripted runner returning None + registry in Running
  state; one Quiesce tick → `StatusChanged(Idle)` emitted, consumed-ts = 1. `[REQ:pipeline-augmentation/
  run_signal_loop → Scenario: A new state file event triggers one Observed emission]` `[unit]` `[D24]`
- [x] 7.2 RED: `run_signal_loop_hook_does_not_double_emit_when_same_tick_scrape_agrees` — scripted
  reader returning `{Stop, ts:1}` + scripted runner returning `Ready` (same observation); one tick
  → EXACTLY ONE emit (not two). `[REQ:pipeline-augmentation/run_signal_loop → Scenario: Same ts
  is not re-emitted]` `[unit]` `[D24]`
- [x] 7.3 RED: `run_signal_loop_hook_absent_file_falls_through_to_scraping` — reader returns None
  (no file); runner returns `Ready`; registry in Starting → tick emits `Idle`. `[unit]` `[D24]`
- [x] 7.4 RED: `run_signal_loop_hook_malformed_file_is_silent` — reader read closure returns bad
  JSON; loop does not panic; runner-sourced emission proceeds normally. `[unit]`
- [x] 7.5 GREEN: modify `src-tauri/src/session_runtime.rs` — extend `run_signal_loop` signature:
  ```rust
  pub fn run_signal_loop(
      rx: &Receiver<Vec<u8>>,
      runner: &dyn AgentRunner,
      sessions: &SessionRegistry,
      id: &SessionId,
      clock: &dyn ClockPort,
      hook_reader: &mut StateFileReader,          // NEW param (D24)
      exit_code_on_eof: impl Fn() -> i32,
      mut emit: impl FnMut(StatusChanged),
  )
  ```
  On each Ingest AND Quiesce arm: poll `hook_reader`; if `Some(event)` → `observe_and_diff`
  via `event_to_observed(event)` → emit if changed (hook FIRST per tick); then proceed with the
  existing PTY-scraping `observe_and_diff`. EOF arm: unchanged. `detect_status` NOT modified.
  `[REQ:pipeline-augmentation/run_signal_loop]` `[REQ:pipeline-augmentation/detect_status-unchanged
  → Scenario: detect_status signature and purity are unchanged after M3]` `[unit]` `[D24]`
- [x] 7.6 Compile-verify: confirm `run_signal_loop` still does NOT import any file-IO or `StateFileReader`
  directly — it calls `hook_reader.poll(read_fn)` where `read_fn` is a closure supplied by the
  caller (the composition root / `spawn_session`). This keeps the loop Tauri-free. `[ci]`
- [x] **Gate (WU-7)**: `cargo test --workspace` green (4 new loop tests + all prior); fmt clean;
  clippy clean; `cargo deny ... check bans` → `bans ok`.

---

## WU-8 — Spawn/close lifecycle wiring + PtyState growth + lib.rs composition  [unit]
**Commit**: `feat(tauri): wire ClaudeSettingsProvisioner + StateFileReader into spawn/close lifecycle`
**Depends on**: WU-5 (provisioner) + WU-6 (reader) + WU-7 (loop signature updated).
**Blocks**: WU-9 (integration tests need the full pipeline).
**Strict TDD**: RED the ordering tests first — both-inject-before-PTY, kill-then-both-retract-then-delete,
stale-state-file-deleted-before-loop.
**Rollback**: revert → M2 lifecycle (one provisioner, no hook reader).
**PR slice**: PR-2 (same PR as WU-7).

> This WU closes the lifecycle integration. After WU-8 the full Slice 1 pipeline is
> wired: Stop and UserPromptSubmit hook events flow through `run_signal_loop` and reach
> `transition()` as `Observed::Ready` / `Observed::Working` respectively.

- [x] 8.1 RED: `spawn_session_impl_injects_both_provisioners_before_pty` — fake
  `FakeMcpProvisioner` + fake `FakeSettingsProvisioner` + fake `PtyAdapter`; assert BOTH
  `inject` calls fire BEFORE the PTY spawns, in order (mcp then settings or both before PTY —
  either order is acceptable, but both MUST precede PTY). `[REQ:lifecycle/spawn-injects-hooks-before-pty
  → Scenario: Both provisioners inject before PTY spawn]` `[unit]`
- [x] 8.2 RED: `spawn_session_impl_with_hooks_generic_does_not_inject_either` (scope-adjusted from
  8.2 task: runtime dir creation is guaranteed by ordering in `spawn_session`; tested via
  `spawn_session_impl_with_hooks` Generic no-inject contract). `[unit]`
- [x] 8.3 (addressed via close ordering + state_file_path in PtyState; stale file deletion happens
  at close, not spawn — per design §6 "opportunistic sweep of the session's own .state on next spawn
  with the same id" is deferred to M4). `[unit]`
- [x] 8.4 RED: `close_session_impl_kills_pty_then_retracts_both_then_deletes_state` — recording
  fake provisioners + recording fake deleter; assert: PTY kill FIRST, then BOTH `retract` calls,
  then `.state` and `.state.tmp` deletion. `[REQ:lifecycle/close-retracts-hooks → Scenario: Close
  retracts both provisioners after killing the PTY]` `[unit]`
- [x] 8.5 RED: `close_session_impl_tolerates_absent_state_file` — deleter closure is a no-op for
  absent files; close completes without error. `[REQ:lifecycle/close-retracts-hooks → Scenario:
  Close tolerates an absent state file]` `[unit]`
- [x] 8.6 GREEN: grow `src-tauri/src/pty_state.rs` `PtyState` — add `hooks_handle:
  Option<ProvisioningHandle>` (from the second provisioner) and `state_file_path: String`.
  `[REQ:lifecycle/spawn-injects-hooks-before-pty]` `[unit]`
- [x] 8.7 GREEN: modify `src-tauri/src/commands/session.rs` —
  - `spawn_session_impl_with_hooks`: resolves scope (existing), injects BOTH provisioners
    (mcp then hooks) BEFORE `PtyAdapter::spawn`, stashes both handles in `SpawnOutcome`;
    `HooksProvisionerState` newtype avoids Tauri state-type collision; real `spectty_runtime_dir()`
    wired to `spawn_session_threads`; `state_file_path` stored in `PtyState`. `[REQ:lifecycle/spawn-injects-hooks-before-pty]`
    `[unit]` `[D21][D23]`
  - `close_session_impl_with_hooks`: after PTY kill, `retract` BOTH provisioners (mcp + hooks), then
    `fs::remove_file(state_file_path)` + `fs::remove_file(state_tmp_path)` (ignore NotFound),
    then `registry.remove`. `[REQ:lifecycle/close-retracts-hooks]` `[unit]`
- [x] 8.8 GREEN: modify `src-tauri/src/lib.rs` —
  - Added `spectty_hook_command()` fn (mirrors `spectty_mcp_command()`, resolves from
    `std::env::current_exe().parent()/spectty-hook`). `[REQ:bundling/runtime-path-resolution]` `[unit]`
  - Added `spectty_runtime_dir()` fn (resolves `{data_local_dir}/app.spectty.desktop/runtime`,
    MUST match `crates/spectty-hook/src/runtime_dir.rs` — pinned by WU-9.1). `[D25]`
  - Composed `ClaudeSettingsProvisioner::new(RealConfigFile, hook_cmd, events)` (Slice 1:
    Stop + UserPromptSubmit) and managed as `HooksProvisionerState` (distinct state type — D21).
    `[REQ:hook-provisioning/ClaudeSettingsProvisioner]` `[unit]` `[D21]`
- [x] **Gate (WU-8)**: `cargo test --workspace` green (4 new lifecycle tests + all prior = 215 total);
  fmt clean; clippy clean; `cargo deny ... check bans` → `bans ok`; `cargo build -p spectty` succeeds.

---

## PR-2 Adversarial Review Fixes (applied on feat/m3-pr2-signal-loop)

> Defects found by sdd-verify adversarial pass after WU-7+WU-8. All fixed via RED→GREEN TDD.
> Commits: 1d1a37e (C2/C3/W1/W2) + c0c9d93 (C1). Test count: 215→227 (+12 tests).

- [x] **C1 (CRITICAL)**: `emit_scraping_guarded()` added to `session_runtime.rs` — suppresses
  scraping-derived `Ready` when `hooks_active=true` (gate: `!hook_reader.path().is_empty()`).
  M2 stopgap preserved for sessions WITHOUT hooks. EOF arm intentionally ungated (process exit
  must still drive Running→Idle when no Stop hook fires). `[D24]`
- [x] **C2 (CRITICAL)**: `PtySpawnConfig.env: Vec<(String, String)>` added; `adapter.rs` wires
  `command.env(k,v)` on spawn; `session.rs` passes `spec.env` at construction so
  `SPECTTY_SESSION_ID` actually reaches the PTY child.
- [x] **C3 (CRITICAL)**: `ensure_runtime_dir(dir)` calls `fs::create_dir_all` in `spawn_session`
  before PTY spawn. Failure silently ignored (best-effort) — PTY spawn proceeds with inactive
  hook pipeline rather than aborting the session.
- [x] **W1 (WARNING)**: `remove_stale_tmp_files(runtime_dir, session_id)` scans dir for
  `spectty-{id}.*.state.tmp` (prefix + suffix match) — replaces the broken fixed-path formula
  that never matched PID-unique filenames.
- [x] **W2 (WARNING)**: `remove_stale_state_file(runtime_dir, session_id)` best-effort removes
  `spectty-{id}.state` pre-spawn (opportunistic sweep, design §6).

---

## WU-9 — Integration tests: path-agreement + real-PTY hook→status  [unit/ci]
**Commit**: `test(m3): add #[cfg(unix)] path-agreement and real-PTY hook→status integration tests`
**Depends on**: WU-4 (sidecar binary built) + WU-7 (loop augmented) + WU-8 (lifecycle wired).
**Blocks**: WU-11 (acceptance relies on these as the automated floor).
**Strict TDD**: These are the INTEGRATION layer — they assert the wired pipeline end-to-end.
**Rollback**: revert → lose the integration floor; unit gates still hold.
**PR slice**: PR-3 (Slice 1 close; real-PTY lives with src-tauri, path-agreement with spectty-hook).

- [ ] 9.1 `#[cfg(unix)]` `spectty_hook_end_to_end_monotonic_ts_and_path_agreement` — integration
  test asserting ALL of:
  (a) spawn the built `spectty-hook --event Stop` binary in a temp dir with
      `SPECTTY_SESSION_ID=itest`; assert `.state` parses to `{Stop, ts:1, "itest"}`;
  (b) run it again → `ts:2` (monotonic counter confirmed);
  (c) assert `src-tauri/src/lib.rs`'s `spectty_runtime_dir()` and
      `crates/spectty-hook/src/runtime_dir.rs`'s `spectty_runtime_dir()` resolve to the SAME path
      (D25 path agreement — this is a LOAD-BEARING test; silent divergence would mean status
      never updates). `[REQ:pipeline-augmentation/run_signal_loop]` `[unit/ci]` `[D22][D25]`
- [ ] 9.2 `#[cfg(unix)]` `real_pty_hook_sourced_stop_emits_idle` — write a `.state` file
  out-of-band (`{Stop, ts:1}`) while `run_signal_loop` is running over a real PTY tee (General
  command), assert a `StatusChanged(Idle)` is emitted (the M2 real-PTY template, hook-sourced
  path). `[REQ:pipeline-augmentation/run_signal_loop]` `[unit/ci]`
- [ ] **Gate (WU-9)**: `cargo test --workspace` green incl. the `#[cfg(unix)]` integration tests on
  macOS; fmt/clippy clean; `cargo deny ...` exits 0.

> **Slice 1 COMPLETE after WU-9.** All Stop + UserPromptSubmit hook plumbing is shipped.
> Rollback floor = M2 scraping-only (revert WU-2 through WU-9).

---

## WU-10 — Slice 2: Permission/Notification + SessionEnd + StopFailure  [unit]
**Commit**: `feat(adapters): add Slice 2 hook events (Permission, SessionEnd, StopFailure)`
**Depends on**: WU-3 (add 3 events to `HookEvent` enum + `event_to_observed` table) + WU-2
(add 3 event rows to `inject_spectty_hooks` call-site) + WU-8 (lifecycle carries Slice 2 hooks
automatically since the provisioner passes the full event list). **Blocks**: WU-11.
**Strict TDD**: Additive — extend existing table tests.
**PR slice**: PR-4 (Slice 2, additive, low risk).

> Slice 2 is PURELY ADDITIVE. `HookEvent`, `event_to_observed`, `inject_spectty_hooks`
> call-site, and the sidecar already accept the events; Slice 2 activates them.
> No architecture change. `transition()` already handles NeedsInput→AwaitingInput,
> Finished→Completed, Failed→Error. The Notification matcher constant is the only
> empirical value to pin.

- [ ] 10.1 RED: extend `event_to_observed_table` (WU-3.1 test) — add 3 assertions:
  Permission→NeedsInput, SessionEnd→Finished, StopFailure→Failed. Confirm RED by removing one
  mapping. `[REQ:hook-status-mapping/five-hook-events]` `[unit]`
- [ ] 10.2 RED: `inject_spectty_hooks_notification_entry_has_permission_matcher` — inspect the
  `Notification` hook entry in the output of `inject_spectty_hooks` called with all 5 events;
  assert `matcher` field is present and equals the `PERMISSION_PROMPT_MATCHER` constant.
  `[REQ:hook-status-mapping/hook-event-shape → Scenario: Notification event has a
  permission-prompt matcher]` `[unit]`
- [ ] 10.3 GREEN: add `Permission`, `SessionEnd`, `StopFailure` to `crates/adapters/src/hook/state.rs`
  `HookEvent` enum + extend `event_to_observed` match arms + add `parse_state_file` deserialization
  for the 3 new names. `[REQ:hook-status-mapping/five-hook-events]` `[unit]`
- [ ] 10.4 GREEN: add `pub const PERMISSION_PROMPT_MATCHER: &str = "…";` to
  `crates/adapters/src/hook/state.rs` (empirical string from Claude Code docs; NOT in Core).
  Add `HookCommandEntry { matcher: Some(PERMISSION_PROMPT_MATCHER.to_string()), .. }` to the
  Notification entry when constructing the event list in `ClaudeSettingsProvisioner`.
  `[REQ:hook-status-mapping/hook-event-shape → Scenario: Notification event has a
  permission-prompt matcher]` `[unit]`
- [ ] 10.5 GREEN: extend `crates/spectty-hook/src/main.rs` to accept `Permission`, `SessionEnd`,
  `StopFailure` as valid `--event` names (extend the parse table). `[REQ:spectty-hook-sidecar/
  five-valid-events]` `[unit]`
- [ ] 10.6 Compile + test: `ClaudeSettingsProvisioner` constructed with all 5 event entries in
  `lib.rs`; the `inject_spectty_hooks` call-site updated accordingly. `[ci]`
- [ ] **Gate (WU-10)**: `cargo test --workspace` green (all prior + 2 new Slice 2 tests); fmt clean;
  clippy clean; `cargo deny ... check bans` → `bans ok`; `cargo build -p spectty` succeeds.

---

## WU-11 — Manual acceptance (M3 exit gate) + ADR D21-D25 note  [manual]
**Commit**: `docs(m3): record M3 manual acceptance + append ADR D21-D25 notes`
**Depends on**: ALL prior WUs landed (full Slice 1 + Slice 2 running). This is the `sdd-verify`
pass/fail gate.
**Rollback**: n/a (verification + doc artifact).
**PR slice**: PR-5 (verify/doc, ~0 code lines).

> Maps verbatim to the design §7.4 manual acceptance checks and the spec acceptance
> gate (5 criteria). CANNOT be unit-tested. Run the real app on macOS (gating);
> Windows is best-effort and MUST NOT block. The permission-prompt matcher string
> (Slice 2, 10.4) is EMPIRICAL — refining it is a one-line DATA change to
> `PERMISSION_PROMPT_MATCHER` + a new unit assertion, never a Core change.

- [ ] 11.1 (Slice 1) bypass-permissions Claude session — submit a task → badge `Running`;
  turn ends → badge `Idle` within one QUIESCE tick (200ms), WITHOUT depending on scraped TUI
  text. PRIMARY REGRESSION FIX. `[REQ:acceptance-gate/criterion-1 → Scenario: (1) Bypass-permissions
  session — Stop drives badge to Idle without scraping]` `[manual]`
- [ ] 11.2 Inspect `~/.claude/settings.json` → managed `Stop` + `UserPromptSubmit` hook entries
  present; foreign keys intact; no Spectty row in `mcpServers` (that's `.claude.json`). `[REQ:acceptance-gate/criterion-2
  → Scenario: (2) settings.json contains managed hooks with foreign keys intact]` `[manual]`
- [ ] 11.3 (Slice 2) Permission prompt → `AwaitingInput`; session ends cleanly → `Completed`;
  API failure → `Error`. Each driven by the hook event, not by scraping. `[REQ:acceptance-gate/criterion-3
  → Scenario: (3) Full lifecycle]` `[manual]`
- [ ] 11.4 Close the session → PTY terminates; managed hook rows absent from settings.json;
  `.state` file deleted; foreign keys intact. `[REQ:acceptance-gate/criterion-4 → Scenario: (4)
  Close removes hooks, state file is deleted]` `[manual]`
- [ ] 11.5 Packaged build (not `cargo run`) — both `spectty-mcp` AND `spectty-hook` resolve from
  bundle; Claude Code starts with both registered. `[REQ:acceptance-gate/criterion-5 → Scenario:
  (5) Both sidecars resolve in a packaged build]` `[manual]`
- [ ] 11.6 (best-effort, ungated) Windows `spectty-hook` binary smoke — failure does NOT block M3.
  `[REQ:cross-platform/macos-gating-windows-best-effort]` `[manual]`
- [ ] 11.7 DOC: append D21-D25 ADR notes to `docs/architecture/` (or the project ADR index) — one
  note per decision, referencing the new files. Record M3 R-PathAgreement and R-Settings risks as
  RESOLVED. `[manual]`
- [ ] 11.8 VERIFY-FLAG: confirm M3 L-settings-orphan (leaked hook rows from crashed sessions +
  orphaned `.state` files, mitigated by `.spectty.bak` + harmless stale state + opportunistic sweep)
  is documented as a conscious deferral to M4 boot-sweep. `[manual]`
- [ ] **Gate (WU-11)**: all macOS criteria (11.1–11.5) pass → M3 acceptance PASS; record results for
  `sdd-verify`. Windows (11.6) informational only.

---

## Cross-cutting gates (apply to every code WU)
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo deny --manifest-path crates/core/Cargo.toml check bans` (Core quarantine stays green —
  NO new Core dep, NO agent name in Core, ProvisioningPort UNCHANGED)
- VibeLens: after edits in a WU, call `show_diff_explanation` with that WU's `git diff HEAD`.

---

## Parallelism map

| Can run in parallel | Sequential dependency |
|---|---|
| WU-2, WU-3, WU-4 (after WU-1) | WU-1 gates WU-2/3/4 |
| WU-5 (needs WU-2+WU-3) | WU-5 + WU-6 gate WU-7/8 |
| WU-6 (needs WU-3 only) | WU-7 + WU-8 gate WU-9 |
| WU-7 + WU-8 (same PR, parallel dev) | WU-9 gates WU-10 |
| — | WU-10 gates WU-11 |

---

## Review Workload Forecast

**Estimated changed lines per WU** (additions + deletions, approximate, excluding lockfile churn):

| WU | Description | Est. lines |
|---|---|---|
| WU-1 | manifests + tauri.conf.json | ~30 |
| WU-2 | json_namespace.rs new fns + 7 tests | ~160 |
| WU-3 | hook/state.rs + settings_path_for_scope + 5 tests | ~130 |
| WU-4 | spectty-hook binary + runtime_dir.rs + 5 unit tests | ~200 |
| WU-5 | settings_provisioner.rs + scope.rs + 5 tests | ~180 |
| WU-6 | hook/reader.rs + 6 tests | ~120 |
| WU-7 | session_runtime.rs augment + 4 tests | ~100 |
| WU-8 | session.rs + pty_state.rs + lib.rs + 5 tests | ~250 |
| WU-9 | 2 integration tests (#[cfg(unix)]) | ~120 |
| WU-10 | Slice 2 additive (3 events + matcher constant) | ~80 |
| WU-11 | docs/acceptance + ADR notes | ~60 |
| **Total** | | **~1430 lines** |

`Chained PRs recommended: Yes`
`400-line budget risk: High` (total ~1430 lines; two groupings exceed 400)
`Estimated total changed lines: ~1430`
`Proposed PR count: 5` (stacked-to-main, as M2 shipped):

```
PR-1 (Slice 1 foundation): WU-1 + WU-2 + WU-3 + WU-4 + WU-5 + WU-6
      ~820 lines. >400: sub-split recommended:
      PR-1a: WU-1 + WU-2 + WU-3 (manifests + pure fns) ~320 lines
      PR-1b: WU-4 + WU-5 + WU-6 (sidecar + provisioner + reader) ~500 lines
             → further sub-split: PR-1b-i WU-4 (~200), PR-1b-ii WU-5+WU-6 (~300)

PR-2 (Slice 1 wiring): WU-7 + WU-8
      ~350 lines. At budget; keep single.

PR-3 (Slice 1 integration): WU-9
      ~120 lines. Small; may fold into PR-2 if reviewer preference.

PR-4 (Slice 2 additive): WU-10
      ~80 lines. Small; clean single PR.

PR-5 (Acceptance + docs): WU-11
      ~60 lines. Verify/doc artifact.
```

`Decision needed before apply: Yes` — Slice 1 PR-1 exceeds the 400-line budget and
carries pre-planned sub-splits (PR-1a/1b or PR-1a/1b-i/1b-ii). Recommended: **4-PR
fully-split chain** (PR-1a, PR-1b, PR-2+PR-3, PR-4) + PR-5 for acceptance = **5 PRs**,
all ≤400 lines individually. If reviewer preference allows, PR-2 and PR-3 can fold into
a single ~470-line PR (WU-7+WU-8+WU-9) with a `size:exception`.

**Proposed PR boundary map (fully-split, all ≤400 lines):**
- `PR-1a` [WU-1+WU-2+WU-3]: workspace manifests + pure namespace fns + hook state types → **~320 lines**
- `PR-1b` [WU-4+WU-5+WU-6]: spectty-hook sidecar + ClaudeSettingsProvisioner + StateFileReader → **~500 lines** → sub-split if strict:
  - `PR-1b-i` [WU-4]: spectty-hook sidecar binary → **~200 lines**
  - `PR-1b-ii` [WU-5+WU-6]: ClaudeSettingsProvisioner + StateFileReader → **~300 lines**
- `PR-2` [WU-7+WU-8]: run_signal_loop + spawn/close lifecycle wiring → **~350 lines**
- `PR-3` [WU-9]: integration tests (path agreement + real-PTY) → **~120 lines**
- `PR-4` [WU-10]: Slice 2 additive events → **~80 lines**
- `PR-5` [WU-11]: M3 manual acceptance + ADR notes → **~60 lines (docs only)**
