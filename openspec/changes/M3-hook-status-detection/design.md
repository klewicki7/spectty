# M3 — Hook-Based Status Detection: Technical Design

> Status: design (the HOW at architectural level). Consumed by `sdd-tasks`.
> Reads: proposal (obs 828 / `proposal.md`), explore (obs 827), and the **real** merged
> M0+M1+M2 code (verified on disk: `crates/core/src/ports/provisioning.rs`,
> `crates/adapters/src/provision/{json_namespace,claude_provisioner,scope,file_io,mod}.rs`,
> `crates/adapters/src/agent/claude_code.rs`, `src-tauri/src/{lib.rs,session_runtime.rs,commands/session.rs}`,
> `crates/spectty-mcp/Cargo.toml`, `src-tauri/tauri.conf.json`). ADR/D-series continues
> from M2 (M2 used D7–D20); M3 introduces **D21–D25**.

## 0. Design Goals & Non-Goals

**Goal**: make Anthropic's official hooks the AUTHORITATIVE Claude Code status source so the
bypass-permissions "stuck Running" bug dies. A new `ClaudeSettingsProvisioner` injects a `hooks`
block into `~/.claude/settings.json` (Global) / `.claude/settings.json` (Project); each lifecycle
event runs a standalone `spectty-hook` sidecar that atomically writes a per-session JSON state file
keyed by `$SPECTTY_SESSION_ID`; `run_signal_loop`'s existing 200ms QUIESCE tick polls that file,
maps the event to an `Observed`, and pushes it through the SAME `observe_and_diff → transition()`
authority as PTY bytes. Scraping stays as fallback.

**Non-Goals (M4)**: `notify`-crate FS watching (poll is M3; format chosen so the upgrade is
non-breaking); HTTP-callback IPC; `SessionStart`/`idle_prompt` nudge; removing scraping; full
boot-time orphan sweep; Windows CI gating (best-effort, macOS-gating from M2 holds).

**Hard invariant (the gate)**: `crates/core` gains **NOTHING** — `ProvisioningPort` is UNCHANGED
(D21). All new behavior is a SECOND adapter impl + new pure namespace fns + a new sidecar binary +
one new loop input. `cargo deny` core-scope stays green.

---

## 1. Architecture Approach

**Pattern**: hexagonal, exactly as M2. The whole change reuses M2 seams with ZERO trait change:
the `ProvisioningPort` trait already generalizes to a second managed file; `run_signal_loop`
already ticks at 200ms; `LaunchSpec.env` already carries `SPECTTY_SESSION_ID`.

```
Claude Code (child) ──fires hook on lifecycle event──▶ spectty-hook sidecar
   │  inherits SPECTTY_SESSION_ID from LaunchSpec.env                │ reads $SPECTTY_SESSION_ID + --event
   │                                                                 ▼ atomic tmp→rename
   │                                              {runtime_dir}/spectty-<id>.state  {event, ts}
   ▼                                                                 │
src-tauri  (composition root)                                        │ polled on QUIESCE(200ms) tick
   │  spawn: ClaudeSettingsProvisioner.inject(scope) BEFORE PTY  ◀───┘
   │  run_signal_loop: hook state-file read ──▶ map event→Observed ──▶ observe_and_diff ──▶ transition()
   │  close: kill → retract hooks + retract mcp → delete .state/.state.tmp → remove
   ▼
crates/adapters
   │  ClaudeSettingsProvisioner (2nd impl ProvisioningPort) over inject/retract_spectty_hooks (pure)
   │  hook_state: HookEvent enum + parse_state_file + event_to_observed (pure) + StateFileReader (consume-once)
   ▼
crates/core  (UNTOUCHED — ProvisioningPort, transition(), Observed all verbatim)

crates/spectty-hook  (NEW binary crate — serde/serde_json only, like spectty-mcp; NOT on core/tauri)
```

---

## 2. Module / File Layout

### `crates/adapters/src/provision/` — grown
| File | Action | Description |
|---|---|---|
| `json_namespace.rs` | Modify | + pure `inject_spectty_hooks` / `retract_spectty_hooks` over the `hooks` key; + `HookCommandEntry` |
| `settings_provisioner.rs` | Create | `ClaudeSettingsProvisioner<F: ConfigFile>` — 2nd `ProvisioningPort` impl |
| `scope.rs` | Modify | + `settings_path_for_scope(scope)` (Global→`~/.claude/settings.json`, Project→`{root}/.claude/settings.json`) |
| `mod.rs` | Modify | export the new provisioner + namespace fns + entry type |

### `crates/adapters/src/hook/` — new (the IPC reader, all pure-testable)
| File | Action | Description |
|---|---|---|
| `mod.rs` | Create | re-exports |
| `state.rs` | Create | `HookEvent` enum, `HookState` struct, `parse_state_file(&str)`, `event_to_observed(HookEvent)` — all PURE |
| `reader.rs` | Create | `StateFileReader` — owns runtime path + last-consumed `ts`; `poll(&ConfigFile-like) -> Option<HookEvent>` (consume-once) |

### `crates/spectty-hook/` — new binary crate (mirrors `crates/spectty-mcp`)
| File | Action | Description |
|---|---|---|
| `Cargo.toml` | Create | serde + serde_json only; NO core, NO tauri; `[[bin]] name = "spectty-hook"` |
| `src/main.rs` | Create | read `$SPECTTY_SESSION_ID` + `--event <Name>`; atomic write `{runtime_dir}/spectty-<id>.state` |
| `src/runtime_dir.rs` | Create | shared runtime-dir resolver (also used by src-tauri via a tiny duplicated fn — see D25) |

### `src-tauri/src/` — grown
| File | Action | Description |
|---|---|---|
| `lib.rs` | Modify | compose `ClaudeSettingsProvisioner` (2nd provisioner state); `spectty_hook_command()` mirroring `spectty_mcp_command()`; resolve `spectty_runtime_dir()` |
| `session_runtime.rs` | Modify | `run_signal_loop` gains a `StateFileReader` param; on each Ingest/Quiesce tick, poll it and feed any `Observed` through `observe_and_diff` BEFORE the PTY observation |
| `commands/session.rs` | Modify | `spawn_session` injects hooks (2nd provisioner) BEFORE PTY; `close_session` retracts hooks + deletes state file; `PtyState` carries a 2nd `ProvisioningHandle` + the state-file path |

### Manifest deltas
| Manifest | Add |
|---|---|
| `crates/spectty-hook/Cargo.toml` | new crate (serde, serde_json) |
| `Cargo.toml` (workspace) | `+ "crates/spectty-hook"` member |
| `src-tauri/tauri.conf.json` | `bundle.externalBin: ["binaries/spectty-mcp", "binaries/spectty-hook"]` (closes M2 L2 for BOTH) |

---

## 3. Interfaces / Contracts

### 3.1 Pure namespace editor (`json_namespace.rs`) — generalizes R7 to a nested key
```rust
/// One hook command Spectty registers. Serializes to the Claude Code shape:
/// { "type": "command", "command": "<spectty-hook path>", "args": ["--event","<Name>"] }
pub struct HookCommandEntry { pub command: String, pub args: Vec<String> }

/// Inject (or replace) ONLY Spectty-owned rows in `hooks.<EventName>`, leaving every
/// foreign hook + foreign top-level key intact. `events` maps EventName → entry. The
/// owned rows are identified by `command == entry.command` (Spectty's hook binary path),
/// so a user's own hook on the same event survives. Idempotent.
pub fn inject_spectty_hooks(current_json: &str, events: &[(String, HookCommandEntry)])
    -> Result<String, ProvisioningError>;
pub fn retract_spectty_hooks(current_json: &str, hook_command: &str)
    -> Result<String, ProvisioningError>;
```
> **R7 GENERALIZED (D21).** `settings.json` `hooks` is `EventName → [ { matcher?, hooks: [ {type,
> command, args} ] } ]` — deeper than `mcpServers` and with more foreign top-level keys
> (`permissions`, `env`, `model`). The owned-key predicate therefore changes: under `mcpServers` we
> owned a NAMED key (`"spectty"`); here we own ROWS whose inner `hooks[].command` equals our sidecar
> path. Retract removes only those rows, leaving foreign rows + empty event arrays' siblings intact.
> Same parse-`Value` / `preserve_order` / re-serialize-pretty machinery; same VALUE+ORDER (not
> byte-identity) contract. This is the headline TDD unit (§7).

### 3.2 State-file schema + pure reader (`hook/state.rs`)
```rust
/// Lifecycle events Spectty registers, in Spectty's vocabulary (NOT Claude's raw names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent { Submit, Stop, Permission, SessionEnd, StopFailure }

/// The atomic state file the sidecar writes. `ts` is a monotonic counter the SIDECAR
/// increments per write (read-modify-write of the prior file), NOT a wall-clock time.
pub struct HookState { pub event: HookEvent, pub ts: u64, pub session_id: String }

pub fn parse_state_file(json: &str) -> Result<HookState, ProvisioningError>; // serde
pub fn event_to_observed(event: HookEvent) -> Observed; // PURE mapping table (§3.4)
```
State-file JSON shape (the sidecar↔reader contract):
```json
{ "event": "Stop", "ts": 7, "session_id": "42" }
```

### 3.3 Consume-once reader (`hook/reader.rs`)
```rust
pub struct StateFileReader { path: String, last_ts: Option<u64> }
impl StateFileReader {
    pub fn new(runtime_dir: &str, session_id: &str) -> Self; // path = {dir}/spectty-{id}.state
    /// Read+parse the file; return Some(event) ONLY if its `ts` is strictly greater than
    /// the last consumed `ts` (then advance last_ts). Absent/unchanged/older → None.
    pub fn poll(&mut self, read: &dyn Fn(&str) -> std::io::Result<Option<String>>) -> Option<HookEvent>;
}
```

### 3.4 Event → Observed mapping (PURE, drives `transition()` unchanged)
| HookEvent | Claude event(+matcher) | → Observed | covers transition |
|---|---|---|---|
| `Submit` | `UserPromptSubmit` | `Working` | Idle/Starting → Running |
| `Stop` | `Stop` | `Ready` | Running → Idle (**PRIMARY FIX**) |
| `Permission` | `Notification` (permission matcher) | `NeedsInput` | Running → AwaitingInput |
| `SessionEnd` | `SessionEnd` | `Finished` | * → Completed |
| `StopFailure` | `Stop` with failure / `SubagentStop` failure | `Failed` | * → Error |

> `transition()` and the `Observed` enum are UNCHANGED — hooks reuse the exact M2 authority (D24).

### 3.5 Injected hooks JSON (the exact shape `inject_spectty_hooks` produces — Slice 1)
```json
{
  "hooks": {
    "UserPromptSubmit": [ { "hooks": [ { "type": "command",
      "command": "/…/spectty-hook", "args": ["--event","Submit"] } ] } ],
    "Stop": [ { "hooks": [ { "type": "command",
      "command": "/…/spectty-hook", "args": ["--event","Stop"] } ] } ]
  }
}
```

### 3.6 spectty-hook CLI contract
`spectty-hook --event <Name>`: reads `$SPECTTY_SESSION_ID` (required; exit 0 silently if unset so a
stray hook never errors Claude), resolves `runtime_dir`, reads the prior state file for its `ts`
(default 0), writes `{event, ts: prior+1, session_id}` to `<path>.tmp` then `rename`s over `<path>`.
Reads (and ignores) Claude's stdin hook JSON. Sub-millisecond; well within the sync-hook timeout.

---

## 4. Data Flow (consume-once)

```
Claude turn ends ─▶ Stop hook ─▶ spectty-hook --event Stop
   reads prior .state (ts=6) ─▶ writes {event:Stop, ts:7} atomically
                                              │
run_signal_loop QUIESCE tick ─▶ reader.poll() ─▶ ts 7 > last 6 ⇒ Some(Stop), last_ts:=7
   ─▶ event_to_observed(Stop)=Ready ─▶ observe_and_diff ─▶ transition(Running,Ready)=Idle ─▶ emit
next tick ─▶ reader.poll() ─▶ ts 7 == last 7 ⇒ None  (consumed once)
```

---

## 5. Architecture Decisions (D21–D25)

| ADR | Choice | Alternatives rejected | Rationale |
|---|---|---|---|
| **D21** | hooks live in `~/.claude/settings.json`; a SECOND `ClaudeSettingsProvisioner` impl, trait UNCHANGED; composition root manages it as a 2nd `Arc<dyn ProvisioningPort>` injected/retracted alongside the mcp one | extend `ClaudeJsonProvisioner` with a 2nd path; one unified provisioner | settings.json is a DIFFERENT file from `~/.claude.json`; a 2nd impl keeps R7 foreign-key preservation scoped per file and independently testable, matching the M2 "one impl per config concern" hexagon. ZERO trait change. |
| **D22** | state file + 200ms QUIESCE poll; consume-once via a **monotonic counter `ts`** the sidecar increments per write | wall-clock timestamp (clock skew, same-tick collisions); file `mtime` (coarse, FS-dependent, no Windows guarantee) | the loop already ticks at 200ms — one `fs::read` per tick is negligible. A sidecar-owned counter gives unambiguous strict-greater consume-once with NO clock dependency and survives sub-200ms event bursts; format is `notify`-upgradeable without changing the sidecar. |
| **D23** | `SPECTTY_SESSION_ID` (already in `LaunchSpec.env`) is the correlation key; the sidecar names the state file by it and NEVER parses Claude's stdin `session_id` | parse Claude's internal hook `session_id` | Claude inherits `LaunchSpec.env` and every hook command inherits Claude's env, so `$SPECTTY_SESSION_ID` flows for free; Claude's internal id is a DIFFERENT identifier and correlating it would need a lookup table. |
| **D24** | hooks AUGMENT scraping: the hook `Observed` is fed through the SAME `observe_and_diff` as the PTY one, hook FIRST per tick; `detect_status` stays pure PTY-only; `transition()` unchanged | route hooks INTO `detect_status` (would make it do file I/O, breaking its pure table-test seam) | one authority (`transition()`); hook precedence means the deterministic signal wins, scraping fills the async gap. Double-emit is impossible because `observe_and_diff` only emits on an ACTUAL status change (a 2nd same-tick observation that doesn't change status returns `None`). |
| **D25** | `spectty-hook` is a STANDALONE binary crate (independent, like `spectty-mcp`), NOT a `spectty-mcp` subcommand; BOTH bundled via `externalBin` | shared workspace crate behind a subcommand flag | mirrors the proven `spectty-mcp` shape (serde-only, no core/tauri); a subcommand would couple two unrelated sidecars' release/versioning. Independent binaries keep each minimal. The runtime-dir resolver is a ~10-line fn duplicated in src-tauri (too small to justify a shared crate; both must agree on the path, asserted by an integration test). |

---

## 6. Lifecycle integration (precedence + cleanup)

- **spawn** (`spawn_session`): resolve scope once; `mcp_provisioner.inject(scope)` AND
  `settings_provisioner.inject(scope)` BEFORE `PtyAdapter::spawn` (same ordering as M2 — hooks must
  exist at session start; settings.json reloads live). `finish_spawn_impl`'s cleanup retracts BOTH
  handles on failure. `PtyState` grows `hooks: Option<ProvisioningHandle>` + `state_file: String`.
- **close** (`close_session_impl`): kill → retract mcp → retract hooks → delete `<state>`/`<state>.tmp`
  → remove. Retracts best-effort (a leaked hook row points at a real harmless binary — D14 carries over).
- **run_signal_loop**: `StateFileReader` polled at the TOP of each Ingest and Quiesce arm; a `Some(event)`
  runs `observe_and_diff(event_to_observed(event))` and emits before the PTY observation that tick.
- **Orphan reconciliation**: M2 R8/L5 widened — leaked settings.json hook rows + orphaned `.state` files;
  full boot sweep STILL DEFERRED to M4. Mitigation: `.spectty.bak` + harmless stale state file +
  opportunistic sweep of the session's own `.state` on next spawn with the same id.

---

## 7. Strict-TDD Plan (RED first)

Test runner: **`cargo test --workspace`**. RED first, one behavior per test, descriptive names (M2 convention).

### 7.1 PURE units (the primary surface — NO fakes)
1. `inject_spectty_hooks` / `retract_spectty_hooks` ROUND-TRIP — start from HAND-FORMATTED
   `settings.json` carrying foreign keys (`permissions`, `env`) AND a foreign user hook on `Stop`;
   inject→retract preserves every foreign value + order; no Spectty row remains; foreign `Stop` hook
   survives (R7 generalized — headline property). Idempotent double-inject; retract-when-absent no-op;
   non-object `hooks` → `Parse` error not data loss (mirror M2 `json_namespace` test set).
2. `parse_state_file` — valid JSON → `HookState`; each `event` string → correct `HookEvent`; malformed
   → `Parse` error.
3. `event_to_observed` — table: all 5 events → the §3.4 `Observed`.
4. `StateFileReader::poll` consume-once — injected `read` closure returns a state with `ts=7`: first
   poll `Some`, second poll (same `ts`) `None`; a newer `ts` re-fires; absent file → `None`; older
   `ts` → `None`.
5. `settings_path_for_scope` — Global → `~/.claude/settings.json`; Project(root) → `{root}/.claude/settings.json`.

### 7.2 Units that NEED fakes / harness
6. `ClaudeSettingsProvisioner` against `FakeConfigFile` — inject writes the right path per scope +
   backs up on first write; retract removes only Spectty rows; absent file retract = `Ok(())`.
7. `run_signal_loop` with a scripted `StateFileReader` (fake `read`) — a hook `Stop` from `Running`
   emits `Idle` and is consumed once; hook precedence over a same-tick scraped observation does not
   double-emit (2nd no-op returns `None`).
8. `close_session_impl` — asserts retract-hooks + state-file-delete are invoked in order (recording
   provisioner + recording deleter closure).

### 7.3 Integration / real-PTY seam (`#[cfg(unix)]`, CI-safe)
9. spectty-hook end-to-end: run the built `spectty-hook --event Stop` with `SPECTTY_SESSION_ID=itest`
   in a temp `runtime_dir`; assert the `.state` file parses to `{Stop, ts:1, "itest"}`; run it again →
   `ts:2` (monotonic). Asserts the sidecar↔reader contract AND that src-tauri's `spectty_runtime_dir()`
   and the sidecar's resolver agree (D25 path agreement).
10. Real-PTY hook→status: write a `.state` file out-of-band, drive `run_signal_loop` over a real PTY
    tee, assert the hook `Observed` reaches the registry (the M2 real-PTY template, hook-sourced).

### 7.4 Manual acceptance (sdd-verify gate)
- [ ] (Slice 1) bypass-permissions Claude session: submit a task → `Running`; turn ends → `Idle`
      (the STUCK-RUNNING REGRESSION FIXED).
- [ ] Inspect `~/.claude/settings.json` → Spectty `Stop`+`UserPromptSubmit` hook rows present; foreign keys intact.
- [ ] Close → hook rows removed; `.state` deleted.
- [ ] (Slice 2) permission prompt → `AwaitingInput`; session end → `Completed`; failure → `Error`.
- [ ] `cargo deny` core-scope green (`crates/core` gained nothing).

---

## 8. Slicing & chained-PR boundaries (stacked-to-main)

**Slice 1 — `Stop` + `UserPromptSubmit` (the primary regression fix).** New `spectty-hook` crate +
`externalBin` (both sidecars); `inject/retract_spectty_hooks` + `ClaudeSettingsProvisioner` +
`settings_path_for_scope`; `hook/{state,reader}` with the 2-event mapping; `run_signal_loop` reader
param; `spawn`/`close` wiring + `PtyState` fields; lib.rs composition. **Project scope included from
Slice 1** (reuses M2 `is_git_tracked`; the path map is a one-line addition — no reason to defer).
Likely **2–3 chained PRs** (exceeds 400-line budget): (PR-1) sidecar crate + externalBin + namespace
fns + provisioner (pure + fake tests); (PR-2) hook reader + run_signal_loop integration; (PR-3) spawn/
close lifecycle wiring + real-PTY integration + manual acceptance.

**Slice 2 — `Notification`/`Permission`, `SessionEnd`, `StopFailure`.** Additive hook rows + 3 mapping
rows + their tests; matcher handling for `Notification`. Likely **1 PR** (additive, low risk).

> Each PR has a clear start/finish, autonomous scope, its own `cargo test --workspace` gate, and a clean
> rollback (revert the PR; M2 behavior — scraping-only — is the floor, so reverting never regresses past M2).

---

## 9. Open Questions — RESOLVED

- **(1) shared crate vs independent binaries** → RESOLVED (D25): independent `spectty-hook` binary
  crate, mirroring `spectty-mcp`; runtime-dir resolver duplicated (~10 lines), agreement asserted by
  integration test 9.
- **(2) event identity for consume-once** → RESOLVED (D22): sidecar-owned MONOTONIC COUNTER `ts`
  (not wall-clock, not mtime); strict-greater-than is the consume-once predicate.
- **(3) Slice 1 Project scope vs Global-first** → RESOLVED (§8): Project scope INCLUDED in Slice 1
  (trivial reuse of M2 scope machinery).
- **(4) runtime-dir + cleanup ownership** → RESOLVED (§6): runtime dir is a Spectty app runtime dir
  (`spectty_runtime_dir()` from Tauri app-local data, NOT bare `/tmp`); `close_session` owns deletion;
  next-spawn opportunistic sweep of the same id's stale `.state`; full boot sweep DEFERRED to M4.

## 10. Risks
- **R-Settings**: `hooks` is more deeply nested than `mcpServers` and the owned-key predicate shifts
  from a named key to a `command`-path match; the round-trip test (7.1) is the pin. Foreign hook on the
  SAME event must survive — explicitly tested.
- **R-Async-gap**: hooks fire asynchronously; a hook may land between two QUIESCE ticks — acceptable
  (≤200ms latency) and scraping covers the gap. Consume-once prevents replay.
- **R-PathAgreement**: src-tauri and the sidecar must resolve the SAME runtime dir; pinned by
  integration test 9. If they diverge, status never updates (silent) — the test is load-bearing.
- **R-WindowsBestEffort**: atomic rename + counter file work on Windows, but the hook command is a
  native binary (not a shell snippet), so no shell-quoting issue; not CI-gated (M2 macOS-gating holds).
