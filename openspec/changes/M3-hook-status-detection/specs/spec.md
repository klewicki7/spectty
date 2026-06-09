# M3 — Hook-Based Status Detection — Delta Spec

> SDD spec phase. Consumes `sdd/M3-hook-status-detection/proposal` (obs #828),
> `openspec/changes/M3-hook-status-detection/proposal.md`, and the M2 archived spec
> (`openspec/changes/archive/2026-06-08-M2-spawn-agent-provisioner/specs/`).
> Drives `sdd-tasks` (with `sdd-design`). Artifact store: HYBRID
> (engram `sdd/M3-hook-status-detection/spec` + this file + per-capability files under
> this directory).
>
> This is a DELTA spec: it states WHAT MUST be true after M3 is applied, on top of the
> M2 baseline (`agent-runner`, `agent-status-machine`, `output-signal`, `session-registry`,
> `provisioning-port`, `agent-session-ui`, `hexagonal-core`). It describes outcomes,
> NOT implementation. RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.
>
> Each requirement is tagged with its verification class:
> - **[unit]** — assertable under Strict TDD (`cargo test --workspace` / `pnpm -C ui test`)
>   without a real PTY, a real agent, or a running app. These seed the Strict-TDD task list.
> - **[manual]** — real-app / real-Claude-Code manual acceptance check; the `sdd-verify`
>   pass/fail gate, on top of the strict-TDD unit gate.
> - **[ci]** — enforced by existing CI gates (cargo build, clippy, cargo-deny).

M2 shipped the provisioner pattern (atomic write + `.spectty.bak` + foreign-key preservation
R7) and established the `ProvisioningPort` trait + `ClaudeJsonProvisioner` adapter managing
`~/.claude.json` `mcpServers`. M2 also shipped the TUI-scraping status pipeline (pure
`detect_status` → `observe_and_diff` → `transition`), with quiescence as the fallback for
`Running → Idle`. This fallback is unreliable when bypass-permissions mode suppresses the
TUI rendering the scraper keys on — the known "stuck Running" regression.

M3 MUST augment the status pipeline with Claude Code's official, structured hooks mechanism.
Hooks fire deterministic lifecycle events (`UserPromptSubmit`, `Stop`, `Notification`,
`SessionEnd`, `StopFailure`) independent of TUI rendering. M3 establishes hooks as the
AUTHORITATIVE status source while keeping scraping as the fallback (D24). Both sources
converge on the SAME `transition()` authority in Core — hooks become a second `Observed`
input path, not a replacement architecture.

M3 also MUST retroactively bundle `spectty-mcp` and the new `spectty-hook` sidecar as Tauri
`externalBin`, closing the M2 L2 bundling gap.

This delta spec is organized by capability. The full text lives here; the per-capability
files in this directory (`hook-provisioning.md`, `spectty-hook-sidecar.md`,
`hook-status-mapping.md`, `pipeline-augmentation.md`, `lifecycle.md`, `bundling.md`) mirror
these sections.

---

## Capability: hook-provisioning

The `ClaudeSettingsProvisioner` is a SECOND `ProvisioningPort` impl, operating on
`~/.claude/settings.json` (Global) or `{project}/.claude/settings.json` (Project). It manages
ONLY the `hooks` key using the same M2 `ConfigFile` atomic-write seam, `.spectty.bak` backup,
and foreign-key preservation invariant (R7). The Core `ProvisioningPort` trait is UNCHANGED.

### Requirement: ClaudeSettingsProvisioner manages the hooks section of settings.json  [unit]

`crates/adapters` MUST provide a `ClaudeSettingsProvisioner` that implements `ProvisioningPort`
(the existing M2 Core trait, UNCHANGED). It MUST manage ONLY the `hooks` top-level key in
`~/.claude/settings.json` (Global) or `{project}/.claude/settings.json` (Project). It MUST
NOT touch `mcpServers`, `permissions`, `env`, `model`, or any other key in settings.json.
The managed key is `hooks` and the managed sub-entries are keyed by hook event name in the
`spectty_*` namespace.

#### Scenario: ClaudeSettingsProvisioner implements ProvisioningPort without trait change  [unit]
- **Given** the `ProvisioningPort` trait after M2 (unchanged)
- **When** `ClaudeSettingsProvisioner` is inspected for trait conformance
- **Then** it MUST implement `inject(scope)` and `retract(scope)` matching the existing trait
  signature with no new methods required on the Core trait

#### Scenario: inject adds managed hook entries and leaves foreign keys untouched  [unit]
- **Given** a settings.json string containing user-authored `hooks`, a `permissions` key, and
  a `model` key (diverse foreign content)
- **When** `inject_spectty_hooks` is called on that string (the pure namespace editor)
- **Then** the output MUST contain the new Spectty-managed hook entries for each configured
  event AND every foreign key (`permissions`, `model`, and any existing user `hooks` sub-entries
  not managed by Spectty) MUST be present and structurally unchanged — asserted as a pure
  `String -> String` unit with no file-IO

#### Scenario: retract removes only Spectty-managed hook entries  [unit]
- **Given** a settings.json string containing both Spectty-managed hook entries and user-authored
  hook entries under the same or different event names
- **When** `retract_spectty_hooks` is called on that string
- **Then** all Spectty-managed entries MUST be absent AND every user-authored hook entry MUST
  be present and structurally unchanged — asserted as a pure unit

#### Scenario: Editing absent or empty hooks section creates valid output  [unit]
- **Given** a settings.json string with no `hooks` key (or an empty document `{}`)
- **When** `inject_spectty_hooks` is called
- **Then** the output MUST be valid JSON containing a `hooks` object with the Spectty-managed
  entries AND all other absent keys MUST remain absent (no key creation side-effects)

#### Scenario: retract on a settings.json that has no Spectty hooks is idempotent  [unit]
- **Given** a settings.json with no Spectty-managed hook entries (fresh file or already retracted)
- **When** `retract_spectty_hooks` is called
- **Then** the output MUST equal the input structurally (no keys added or removed) AND MUST
  remain valid JSON — asserted as a pure unit

### Requirement: Settings.json scope path resolves correctly for Global and Project  [unit]

The `ClaudeSettingsProvisioner` MUST resolve `ProvisioningScope::Global` to
`~/.claude/settings.json` and `ProvisioningScope::Project(root)` to
`{root}/.claude/settings.json`. This is a DISTINCT path mapping from the M2 `ClaudeJsonProvisioner`
(which resolves Global to `~/.claude.json` and Project to `{root}/.mcp.json`). The path
resolution MUST be a pure function asserted without touching the filesystem. The existing
injected `is_git_tracked` predicate (M2) governs scope selection upstream; the settings path
resolver just maps the chosen scope to its file path.

#### Scenario: Global scope resolves to ~/.claude/settings.json  [unit]
- **Given** the settings path resolver with `ProvisioningScope::Global`
- **When** the resolver runs
- **Then** it MUST return the path `~/.claude/settings.json` (expanded) with no filesystem access

#### Scenario: Project scope resolves to {root}/.claude/settings.json  [unit]
- **Given** the settings path resolver with `ProvisioningScope::Project("/some/repo")`
- **When** the resolver runs
- **Then** it MUST return `/some/repo/.claude/settings.json` with no filesystem access

### Requirement: Settings.json writes are atomic with a one-time .spectty.bak backup  [unit]

`ClaudeSettingsProvisioner` MUST use the same M2 `ConfigFile` atomic-write seam (temp file →
fsync → atomic rename) for all writes to settings.json. Before the FIRST write to a given
settings.json path, it MUST copy the existing file to `<path>.spectty.bak`. The seam MUST be
injectable so backup + atomic-write behavior is testable with a fake filesystem, matching the
M2 `ClaudeJsonProvisioner` contract exactly.

#### Scenario: First write creates a .spectty.bak backup of the original settings.json  [unit]
- **Given** an existing settings.json with user content and the atomic-write seam backed by a
  fake filesystem
- **When** `ClaudeSettingsProvisioner` performs its first write (inject call)
- **Then** a `<settings-path>.spectty.bak` copy of the ORIGINAL contents MUST exist AND the
  written file MUST land via temp-file-then-rename — asserted on the fake filesystem operations

#### Scenario: Subsequent writes do not overwrite an existing .spectty.bak  [unit]
- **Given** a settings.json where a `.spectty.bak` already exists (from a prior inject)
- **When** `ClaudeSettingsProvisioner` performs a second write (e.g. retract then re-inject)
- **Then** the `.spectty.bak` MUST NOT be overwritten — the original pre-Spectty state is
  preserved as the escape hatch

---

## Capability: spectty-hook-sidecar

`spectty-hook` is a NEW standalone binary crate. When invoked by Claude Code's hook subsystem,
it reads `$SPECTTY_SESSION_ID` from its inherited environment and `--status <STATUS>` from
its CLI args, then atomically writes a per-session JSON state file under the Spectty runtime
dir. It is statically compiled with no shell dependency.

### Requirement: spectty-hook atomically writes a per-session state file  [unit]

The `spectty-hook` binary MUST accept `--status <STATUS>` as a CLI argument. It MUST read
`SPECTTY_SESSION_ID` from its inherited environment. It MUST atomically write
`{runtime_dir}/spectty-{SPECTTY_SESSION_ID}.state` with JSON content
`{"status": "<STATUS>", "ts": <unix_epoch_seconds>}`, where the write uses a `.tmp` → rename
sequence (same atomic pattern as the provisioner). The binary MUST return exit code 0 on
success and a non-zero exit code on failure.

#### Scenario: spectty-hook writes a valid state file from env + args  [unit] (#[cfg(unix)])
- **Given** `SPECTTY_SESSION_ID=abc123` in the environment, the runtime dir exists, and
  `spectty-hook --status Ready` is invoked
- **When** the binary runs
- **Then** `{runtime_dir}/spectty-abc123.state` MUST exist, contain valid JSON with
  `{"status": "Ready", "ts": <a reasonable unix epoch>}`, and the file MUST have been written
  via `.tmp` → rename (no partial-write observable), asserted in an integration test

#### Scenario: spectty-hook with unknown status arg exits non-zero  [unit]
- **Given** `spectty-hook --status BOGUS_VALUE` invoked with a valid env
- **When** the binary runs
- **Then** it MUST exit with a non-zero exit code and MUST NOT write a state file
  (malformed writes are not observable to the watcher)

#### Scenario: spectty-hook exits non-zero when SPECTTY_SESSION_ID is absent  [unit]
- **Given** `SPECTTY_SESSION_ID` is NOT set in the environment
- **When** `spectty-hook --status Ready` is invoked
- **Then** it MUST exit with a non-zero exit code and MUST NOT write a state file

#### Scenario: spectty-hook exits non-zero when the runtime dir does not exist  [unit]
- **Given** a non-existent runtime dir and `SPECTTY_SESSION_ID` is set
- **When** `spectty-hook --status Ready` is invoked
- **Then** it MUST exit with a non-zero exit code (the binary does NOT create the dir — that
  is the responsibility of the host process at session spawn)

### Requirement: spectty-hook accepts all five mapped status values  [unit]

The binary MUST accept the following STATUS values as valid arguments (mapping to the five
locked hook events): `Working`, `Ready`, `NeedsInput`, `Finished`, `Failed`. These are the
STATUS strings the watcher maps back to `Observed` variants. No other values MUST be accepted.

#### Scenario: Each valid status value writes a state file  [unit]
- **Given** a valid environment and runtime dir
- **When** `spectty-hook --status <VALUE>` is invoked for each of `Working`, `Ready`,
  `NeedsInput`, `Finished`, `Failed`
- **Then** each invocation MUST produce a valid state file containing the matching status string

---

## Capability: hook-status-mapping

The mapping from Claude Code hook event names to `Observed` variants is DATA (a table in the
adapter), not Core logic. The watcher in `run_signal_loop` reads the state file's `status`
string and maps it to an `Observed` variant. This table is the M3 locked mapping.

### Requirement: The five hook events map to Observed variants via a pure table  [unit]

`crates/adapters` MUST define a PURE function (or const lookup table) that maps the five
status strings written by `spectty-hook` to `Observed` variants:

| State file `status` | `Observed` variant | Hook event that writes it |
|---|---|---|
| `"Working"` | `Observed::Working` | `UserPromptSubmit` (no matcher) |
| `"Ready"` | `Observed::Ready` | `Stop` (no matcher) |
| `"NeedsInput"` | `Observed::NeedsInput` | `Notification` (permission_prompt matcher) |
| `"Finished"` | `Observed::Finished` | `SessionEnd` (no matcher) |
| `"Failed"` | `Observed::Failed` | `StopFailure` (no matcher) |

Unrecognized status strings MUST map to `None` (ignored, not an error). The mapping MUST be
a pure function with no I/O, tested as a unit table test.

#### Scenario: "Ready" maps to Observed::Ready  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"Ready"`
- **Then** it MUST return `Some(Observed::Ready)`

#### Scenario: "Working" maps to Observed::Working  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"Working"`
- **Then** it MUST return `Some(Observed::Working)`

#### Scenario: "NeedsInput" maps to Observed::NeedsInput  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"NeedsInput"`
- **Then** it MUST return `Some(Observed::NeedsInput)`

#### Scenario: "Finished" maps to Observed::Finished  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"Finished"`
- **Then** it MUST return `Some(Observed::Finished)`

#### Scenario: "Failed" maps to Observed::Failed  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"Failed"`
- **Then** it MUST return `Some(Observed::Failed)`

#### Scenario: An unrecognized status string maps to None  [unit]
- **Given** the hook-status mapping function
- **When** it is called with any string not in the five locked values (e.g. `"UNKNOWN"`, `""`)
- **Then** it MUST return `None` (the watcher silently ignores the event, scraping fallback
  continues)

### Requirement: The hook event settings.json shape is DATA in the adapter  [unit]

The settings.json `hooks` value shape injected by `inject_spectty_hooks` MUST embed the hook
event configuration as DATA in the adapter, not as Core logic. The managed entries MUST follow
this structure for each event:

```json
"<EventName>": [
  {
    "matcher": "<optional-matcher-string>",
    "hooks": [
      {
        "type": "command",
        "command": "<spectty-hook-binary-path>",
        "args": ["--status", "<STATUS>"]
      }
    ]
  }
]
```

For `Stop` and `UserPromptSubmit` (no-matcher events), the `matcher` field MUST be absent.
For `Notification` (permission-prompt), the `matcher` MUST be present with the empirical
permission-prompt matcher string. For `StopFailure` and `SessionEnd`, the `matcher` MUST be
absent. The exact matcher strings are empirical (sourced from Claude Code docs/observation)
and MUST live as constants in the adapter, NOT in Core.

#### Scenario: No-matcher events have no matcher field in the injected JSON  [unit]
- **Given** the output of `inject_spectty_hooks` for a session
- **When** the `Stop` and `UserPromptSubmit` hook entries are inspected
- **Then** neither MUST contain a `matcher` field (absent, not null)

#### Scenario: Notification event has a permission-prompt matcher  [unit]
- **Given** the output of `inject_spectty_hooks` for a session
- **When** the `Notification` hook entry is inspected
- **Then** it MUST contain a `matcher` field with a non-empty string (the empirical
  permission-prompt matcher, asserted on the constant value in the adapter)

---

## Capability: pipeline-augmentation

Hook-sourced `Observed` events flow through the SAME `observe_and_diff → transition()`
pipeline as PTY-scraped observations. The watcher in `run_signal_loop` reads the state file
on the existing QUIESCE(200ms) tick. `detect_status` stays a pure PTY-only function (D24).
Each event is consumed exactly once.

### Requirement: run_signal_loop reads the state file on QUIESCE ticks and emits Observed  [unit]

`src-tauri/src/session_runtime.rs` MUST augment `run_signal_loop` so that on each QUIESCE
(200ms) tick, it reads the per-session state file (keyed by `SPECTTY_SESSION_ID`). If the
file contains an event with a `ts` STRICTLY GREATER than the last consumed `ts` (initialized
to 0 at loop start), the loop MUST map the status string to `Observed` via the hook-status
mapping table and feed it into `observe_and_diff` — the SAME path as PTY-scraped observations.
After feeding the event, the loop MUST record the consumed `ts` and MUST NOT re-emit the same
event on subsequent ticks. `detect_status` MUST NOT be modified (it stays pure PTY-only).

#### Scenario: A new state file event triggers one Observed emission  [unit]
- **Given** the watcher with a fake state-file reader returning `{"status":"Ready","ts":1000}`
  and last-consumed-ts = 0
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST receive EXACTLY ONE `Observed::Ready` AND the consumed-ts
  MUST be updated to 1000 — asserted with the fake reader and a fake `observe_and_diff` sink

#### Scenario: Same ts is not re-emitted on a subsequent tick  [unit]
- **Given** the watcher after consuming a `ts=1000` event
- **When** the next QUIESCE tick fires and the state file still reads `{"status":"Ready","ts":1000}`
- **Then** `observe_and_diff` MUST NOT receive a second emission — the event is consumed once

#### Scenario: A newer ts supersedes without re-emitting the old one  [unit]
- **Given** the watcher after consuming `ts=1000` and the state file now reads
  `{"status":"Working","ts":2000}`
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST receive `Observed::Working` (ts 2000) and consumed-ts MUST
  be 2000 — the Working event is emitted once

#### Scenario: A malformed state file is silently ignored  [unit]
- **Given** the watcher and a state file containing malformed JSON or a missing `status` field
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST NOT receive any emission AND the consumed-ts MUST remain
  unchanged — asserted with a fake reader returning bad JSON

#### Scenario: An absent state file on a tick is silently ignored  [unit]
- **Given** the watcher and no state file present at the expected path
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST NOT receive any emission AND no error is returned — the
  absence of the file is a normal condition (no hook fired yet)

### Requirement: Hook-sourced Observed events go through the same transition() authority  [unit]

The `transition()` function (M2 Core, UNCHANGED) MUST remain the sole authority for
`AgentStatus` advancement. An `Observed` derived from a hook event MUST be processed by
`transition(current, observed)` identically to a scrape-derived `Observed`. No hook-specific
bypass or short-circuit of the transition table is permitted.

#### Scenario: Hook-derived Ready observation is rejected by transition if current is Starting  [unit]
- **Given** `current = Starting` and the watcher emits `Observed::Ready` (from a hook event)
- **When** `transition(Starting, Ready)` runs
- **Then** it MUST return `Starting` unchanged (the M2 rule: Starting → Idle is the only legal
  first step; the transition table is unmodified by M3)

#### Scenario: Hook-derived Working observation advances Running-ish states correctly  [unit]
- **Given** `current = Idle` and the watcher emits `Observed::Working`
- **When** `transition(Idle, Working)` runs
- **Then** it MUST return `Running` (the legal Idle → Running transition)

### Requirement: detect_status stays pure PTY-only and is not modified by M3  [unit]

`ClaudeCodeRunner::detect_status` MUST NOT be modified in M3 to read files, check state, or
incorporate hook data. It MUST remain a pure function over `OutputSignal` only (D24 lock).
Scraping-based detection remains the fallback path when no hook event has fired within the
QUIESCE window.

#### Scenario: detect_status signature and purity are unchanged after M3  [unit]
- **Given** `ClaudeCodeRunner::detect_status` after M3 is applied
- **When** its signature and body are inspected
- **Then** it MUST accept only `&self` and `&OutputSignal` and MUST NOT call any filesystem
  function, read any file, or access any session-specific state beyond the signal

---

## Capability: lifecycle

Injection and retraction of settings.json hooks follow the same ordering established for
`mcpServers` in M2. The state file is created by `spectty-hook` at runtime and cleaned up
by `close_session_impl`. The runtime dir is created by `spawn_session_impl` before the PTY
spawns.

### Requirement: spawn_session_impl injects hooks before PTY spawn  [unit]

`spawn_session_impl` MUST call `ClaudeSettingsProvisioner::inject` for the resolved scope
BEFORE calling `PtyAdapter::spawn`. This ordering ensures that when Claude Code starts, the
hooks are already present in settings.json and are loaded at agent startup. The existing
`ClaudeJsonProvisioner::inject` call (for `mcpServers`) MUST NOT be moved; both inject calls
MUST precede `PtyAdapter::spawn`.

#### Scenario: Both provisioners inject before PTY spawn  [unit]
- **Given** `spawn_session_impl` wired to a fake `ClaudeJsonProvisioner`, a fake
  `ClaudeSettingsProvisioner`, and a fake `PtyAdapter`
- **When** `spawn_session_impl` runs
- **Then** `ClaudeJsonProvisioner::inject` MUST be called before the PTY spawns AND
  `ClaudeSettingsProvisioner::inject` MUST ALSO be called before the PTY spawns —
  asserted on invocation order with the fakes

#### Scenario: spawn_session_impl creates the runtime dir before injection  [unit]
- **Given** `spawn_session_impl` with a fake filesystem
- **When** it runs for a new session
- **Then** the Spectty runtime dir MUST be created (if absent) BEFORE either provisioner injects
  (the hook binary will write there immediately upon first hook fire)

### Requirement: close_session_impl retracts hooks and deletes the state file  [unit]

`close_session_impl` MUST, after killing the PTY (M1 path), retract BOTH provisioners
(`ClaudeJsonProvisioner::retract` AND `ClaudeSettingsProvisioner::retract`) and delete the
per-session state files (`{runtime_dir}/spectty-{id}.state` and `{runtime_dir}/spectty-{id}.state.tmp`
if present). This MUST follow the existing kill-then-retract-then-remove order. A missing state
file at close time MUST be tolerated (not an error).

#### Scenario: Close retracts both provisioners after killing the PTY  [unit]
- **Given** `close_session_impl` wired to fake provisioners and a fake state-file deleter
- **When** `close_session_impl` runs
- **Then** PTY kill MUST occur first, then BOTH `retract` calls MUST occur, then state file
  deletion — asserted on invocation order with the fakes

#### Scenario: Close tolerates an absent state file  [unit]
- **Given** `close_session_impl` and no `.state` file exists for the session
- **When** `close_session_impl` runs
- **Then** it MUST complete successfully without error — a missing file at close is normal
  (no hook fired before close)

### Requirement: SPECTTY_SESSION_ID is the correlation key between spawn context and hook binary  [unit]

The `SPECTTY_SESSION_ID` env var (already injected into `LaunchSpec.env` by M2's
`ClaudeCodeRunner::launch_spec`) MUST be the sole key correlating the spawned Claude Code
process's hook commands to the Spectty session's state file. NO parsing of Claude's internal
`session_id` field from hook stdin JSON is required or permitted (D23).

#### Scenario: LaunchSpec.env carries SPECTTY_SESSION_ID for hook correlation  [unit]
- **Given** `ClaudeCodeRunner::launch_spec` after M3 (unchanged from M2 on this point)
- **When** the resulting `LaunchSpec` is inspected
- **Then** `SPECTTY_SESSION_ID` MUST be present in the env — this is the key the hook binary
  uses to name the state file, asserted on the `LaunchSpec` value with no process spawned

### Requirement: Orphaned settings.json hooks and state files are best-effort mitigated  [unit]

M3 does NOT build full boot-time orphan reconciliation (that is deferred to M4, as in M2 R8
/ L5). The M3 concrete mitigations are: (a) `.spectty.bak` is the manual escape hatch for
settings.json, (b) orphaned `.state` files are harmless (a stale `.state` is never read once
its session id is retired — the watcher keyed to that id is gone), (c) opportunistic sweep
at spawn time: if a `.state` file for the session id already exists at spawn (leftover from a
crashed prior run with the same id), it MUST be deleted before the loop starts so stale events
are not replayed.

#### Scenario: Stale state file from a crashed prior run is deleted at spawn  [unit]
- **Given** a session being spawned whose `SPECTTY_SESSION_ID` has a leftover `.state` file
  from a prior run
- **When** `spawn_session_impl` sets up the watcher loop for the new session
- **Then** the leftover `.state` file MUST be deleted before the watcher loop starts, asserted
  with a fake filesystem

---

## Capability: bundling

`spectty-hook` and `spectty-mcp` MUST BOTH be configured as Tauri `externalBin` sidecars.
This closes the M2 L2 bundling gap for `spectty-mcp` and establishes the bundling pattern
for `spectty-hook`. Both binaries MUST be resolvable at runtime in a packaged Tauri build via
the `spectty_hook_command()` / `spectty_mcp_command()` pattern in `src-tauri/src/lib.rs`.

### Requirement: Both sidecars are declared as externalBin in tauri.conf.json  [ci]

`src-tauri/tauri.conf.json` MUST declare BOTH `spectty-mcp` AND `spectty-hook` under
`bundle.externalBin` with target-triple-suffixed binary names (matching the Tauri sidecar
convention). A missing `externalBin` entry causes silent failure in packaged builds (the
binary is not shipped).

#### Scenario: tauri.conf.json contains both sidecar entries  [ci]
- **Given** `src-tauri/tauri.conf.json` after M3
- **When** `bundle.externalBin` is inspected
- **Then** it MUST contain entries for `spectty-mcp` AND `spectty-hook` (with appropriate
  target-triple suffix patterns), assertable by cargo build + manifest inspection

### Requirement: Runtime path resolution works for both sidecars  [unit]

`src-tauri/src/lib.rs` MUST provide a `spectty_hook_command()` function mirroring the existing
`spectty_mcp_command()` pattern: it resolves the sidecar binary path using `app.path()` (or
the equivalent Tauri v2 resolver) so that it works in both `cargo run` (dev, from the local
`target/` dir) and in a packaged Tauri build (from the bundle resources dir). `ClaudeSettingsProvisioner`
MUST use this resolved path (not a hardcoded path) as the `command` in each injected hook entry.

#### Scenario: spectty_hook_command() resolves without panic in dev mode  [unit]
- **Given** the Tauri app handle in a test/dev context
- **When** `spectty_hook_command()` is called
- **Then** it MUST return a non-empty path string without panicking — the resolved path is what
  gets embedded in settings.json hook entries

#### Scenario: The injected hook command path matches spectty_hook_command() output  [unit]
- **Given** the output of `inject_spectty_hooks` for a session (with the path injected at
  provision time)
- **When** the `command` field in each managed hook entry is inspected
- **Then** it MUST equal the path returned by `spectty_hook_command()`, not a hardcoded literal

---

## Acceptance gate (M3 exit criteria)  [manual]

These checks are the `sdd-verify` pass/fail gate, on top of the strict-TDD unit gate
(`cargo test --workspace`; `pnpm -C ui test`). All manual scenarios require a real Claude Code
session. macOS is the gating platform; Windows is best-effort (M2 ADR holds).

### Requirement: M3 satisfies all five acceptance criteria  [manual]

#### Scenario: (1) Bypass-permissions session — Stop drives badge to Idle without scraping
- **Given** a Claude Code session spawned with `--dangerously-skip-permissions` (bypass mode)
- **When** the user submits a task (badge → `Running`) and the turn ends
- **Then** the badge MUST return to `Idle` within one QUIESCE tick (200ms) after the hook fires,
  WITHOUT depending on any scraped TUI text — this is the primary regression fix

#### Scenario: (2) settings.json contains managed hooks with foreign keys intact
- **Given** a spawned Claude Code session
- **When** the user inspects `~/.claude/settings.json` (Global) or `{project}/.claude/settings.json`
  (Project)
- **Then** Spectty's managed `hooks` entries MUST be present for Stop and UserPromptSubmit
  AND every user-authored key/hook in the file MUST be structurally unchanged (R7 for settings.json)

#### Scenario: (3) Full lifecycle — Notification → AwaitingInput; SessionEnd → Completed; StopFailure → Error
- **Given** an active Claude Code session
- **When** a permission prompt is hit → status MUST reach `AwaitingInput`; when the session
  ends cleanly → status MUST reach `Completed`; when an API failure occurs → status MUST reach
  `Error` — each driven by the corresponding hook event, not by scraping

#### Scenario: (4) Close removes hooks, state file is deleted, badge shows session ended
- **Given** a running Claude Code session with managed hooks injected
- **When** the session is closed
- **Then** the PTY MUST terminate, `hooks` managed entries MUST be absent from settings.json,
  the per-session `.state` file MUST be deleted, and the foreign hooks/keys in settings.json
  MUST remain intact

#### Scenario: (5) Both sidecars resolve and Claude Code starts in a packaged build
- **Given** a packaged Tauri build (not `cargo run`)
- **When** a Claude Code session is spawned
- **Then** BOTH `spectty-mcp` AND `spectty-hook` MUST be resolvable from the bundle resources
  AND Claude Code MUST start successfully with both registered

---

## Cross-platform stance  [manual]

### Requirement: macOS MUST pass; Windows hook binary is best-effort  [manual]

M3 acceptance MUST pass on macOS (inheriting M2's stance). The real-PTY integration tests
MUST be `#[cfg(unix)]`. The `spectty-hook` binary + atomic-rename state file work on Windows
by design (no FIFO/socket dependency), but Windows is NOT CI-gated. A Windows hook-binary
failure MUST NOT block M3 acceptance.

#### Scenario: macOS acceptance is gating, Windows is best-effort
- **Given** the M3 acceptance run
- **When** evaluated per platform
- **Then** all five exit criteria MUST pass on macOS AND a Windows `spectty-hook` failure MUST
  NOT block M3

---

## Out of scope (NO requirements in M3 — M4/M5)

The following carry NO M3 requirements and MUST NOT be built in M3:

- **`notify`-crate filesystem watching** (kqueue/FSEvents/inotify) — the QUIESCE(200ms) poll
  is M3; `notify` is post-M3. The state-file format is chosen so a `notify` upgrade requires
  no hook-command or provisioner change.
- **HTTP callback IPC to `spectty-mcp`** as the transport — requires port negotiation; M4.
- **`SessionStart` → Starting** and **`Notification(idle_prompt)` → Idle** — not in the five
  locked transitions; M4 or later.
- **Removing or replacing the TUI scraping path** — `detect_status` + quiescence stay as the
  fallback; M3 augments, does not replace.
- **Boot-time orphan reconciliation** (full settings.json + state-file sweep) — M3's mitigation
  is `.spectty.bak` + harmless stale state files + opportunistic pre-spawn sweep. Full boot
  sweep is M4.
- **Windows CI-gating** of the hook binary — best-effort only; M2 ADR holds.
- **`parse_cost` real implementation** and **`quick_actions` real answering** — M3 skeleton
  carries forward; real behavior is M4/M5.
- **Living Spec pane** (`Spec` aggregate, plan-approval gate, structured task progress) — M4.
- **Multi-session UI** (tabs, panes, switcher) — M4.
