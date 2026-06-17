# Capability: agent-runner

> Living baseline spec. Established by change `M2-spawn-agent-provisioner` (archived 2026-06-08).
> Confirmed by change `M4-triad-spec-vibelens` (archived 2026-06-17) — W1 doc-only correction verified baseline correctness.
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The `AgentRunner` port (Core trait) abstracts every supported AI CLI agent behind ONE
interface. M2 ships two adapters in `crates/adapters` — `ClaudeCodeRunner` (Cooperative tier)
and `GenericRunner` (Generic tier). Agent names, config formats, and ANSI/regex parsing live
ONLY in adapters; the Core port carries none of them. The pure Core `AgentStatus` state
machine (the `transition` function) is part of this capability.

## Requirement: AgentRunner is a Core port with the M2 method subset
`spectty-core` MUST define an `AgentRunner` trait whose M2 subset — `launch_spec`,
`detect_status`, `descriptor`, `tier` — is fully implementable, with `parse_cost` and
`quick_actions` present as honest, tested skeletons. `detect_status` MUST have the signature
`detect_status(&OutputSignal) -> Option<Observed>` (it returns an observation, not a final
`AgentStatus`; the pure `transition` function folds that observation into the next status).
The trait MUST NOT expose a `provisioner()` method (provisioning is the separate
`ProvisioningPort`; this supersedes the ADR-0004 method shape for M2).

### Scenario: AgentRunner exposes the four full methods plus two skeleton methods
- **Given** the `spectty-core` `AgentRunner` trait after M2
- **When** its method set is inspected
- **Then** `launch_spec`, `detect_status`, `descriptor`, and `tier` MUST each be present and
  fully specified, `parse_cost` and `quick_actions` MUST each be present as typed skeletons,
  AND there MUST be NO `provisioner()` method on the trait

## Requirement: AgentSpec and descriptor types are Core, agent-agnostic value types
`spectty-core` MUST define `AgentSpec`, `AgentTier` (at least `Cooperative` and `Generic`),
and `AgentDescriptor` as plain serde value types with no behavior branching on a hard-coded
agent name.

### Scenario: AgentSpec round-trips through serde
- **Given** an `AgentSpec` value
- **When** it is serialized and deserialized
- **Then** the round-tripped value MUST equal the original

### Scenario: AgentTier distinguishes Cooperative from Generic
- **Given** the `AgentTier` enum
- **When** a runner reports its tier via `tier()`
- **Then** `ClaudeCodeRunner` MUST report `Cooperative` AND `GenericRunner` MUST report
  `Generic`, asserted without spawning either agent

## Requirement: ClaudeCodeRunner produces a correct LaunchSpec
`ClaudeCodeRunner` MUST map a launch context to a `LaunchSpec` invoking Claude Code in the
given workspace directory, as a pure testable function.

### Scenario: launch_spec maps context to program, cwd, and env
- **Given** a launch context carrying a workspace directory and a session id
- **When** `ClaudeCodeRunner::launch_spec` runs
- **Then** the resulting `LaunchSpec` MUST name the Claude Code program, set `cwd` to the
  workspace directory, AND carry any session-identifying env, asserted on the value with no
  real process spawned

## Requirement: ClaudeCodeRunner detects status by scraping known patterns
`ClaudeCodeRunner::detect_status` MUST map an `OutputSignal` to an observed status via
an EMPIRICAL pattern table held as DATA in the adapter (not Core logic), returning `None` when
nothing matches.

### Scenario: Permission-prompt output is observed as AwaitingInput
- **Given** an `OutputSignal` whose text window matches a permission-prompt pattern
- **When** `ClaudeCodeRunner::detect_status` runs
- **Then** it MUST return the AwaitingInput observation, asserted as a pure function with no
  real PTY

### Scenario: Unrecognized output yields no observation
- **Given** an `OutputSignal` matching none of the runner's patterns
- **When** `ClaudeCodeRunner::detect_status` runs
- **Then** it MUST return `None`

## Requirement: GenericRunner detects Idle and idle-timeout Completed via injected time
`GenericRunner::detect_status` MUST implement first-activity → `Idle`, ongoing activity →
`Running`, and a CONFIGURABLE inactivity window → `Completed`, driven by an INJECTED time seam
(not a wall clock). The Generic adapter advertises no `spectty_*` tools and does no injection.

### Scenario: GenericRunner transitions to Completed after the configured idle window
- **Given** a `GenericRunner` with a configured idle-timeout and an injected fake clock
- **When** an `OutputSignal` reports no new activity for at least the configured window
- **Then** `detect_status` MUST yield the Completed observation, asserted deterministically

### Scenario: GenericRunner reaches Idle on first output
- **Given** a `GenericRunner` spawning `bash`
- **When** the shell starts and produces its first prompt output
- **Then** status MUST reach `Idle`

## Requirement: parse_cost and quick_actions ship as honest, tested skeletons
For M2, both runners' `parse_cost` MUST return an empty/zero cost delta and `quick_actions`
MUST return an empty/static set, each with a test asserting the skeleton contract.

### Scenario: parse_cost returns a zero/empty delta in M2
- **Given** any `OutputSignal`
- **When** a runner's `parse_cost` is called in M2
- **Then** it MUST return an empty or zero cost delta, NOT a parsed value, AND a test MUST
  assert this skeleton behavior

## Requirement: A pure Core transition function enforces legal AgentStatus transitions
`spectty-core` MUST define `AgentStatus` with `Starting`, `Idle`, `Running`, `AwaitingInput`,
`Completed`, `Error`, and a PURE total `transition(current, observed) -> AgentStatus`
enforcing: `Starting→Idle`; `Idle→Running`; `Running→{AwaitingInput,Completed,Error}`;
`AwaitingInput→Running`; any state `→Error`. Illegal observations MUST leave `current`
unchanged. The function MUST contain no agent name, no I/O, no time access.

### Scenario: Running → AwaitingInput → Running is legal
- **Given** `current = Running`
- **When** `transition(Running, AwaitingInput)` then `transition(AwaitingInput, Running)` run
- **Then** the first MUST yield `AwaitingInput` AND the second MUST yield `Running`

### Scenario: An illegal jump is rejected and leaves current unchanged
- **Given** `current = Starting`
- **When** `transition(Starting, Completed)` runs
- **Then** it MUST return `Starting` unchanged

### Scenario: Any state may transition to Error
- **Given** any `current` status
- **When** `transition(current, Error)` runs
- **Then** it MUST return `Error`

---

## M3 capability: hook-status-mapping

The mapping from Claude Code hook event names to `Observed` variants is DATA (a table in the
adapter), not Core logic. The watcher in `run_signal_loop` reads the state file's `status`
string and maps it to an `Observed` variant. This table is the M3 locked mapping.

## Requirement: The five hook events map to Observed variants via a pure table  [unit]

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

### Scenario: "Ready" maps to Observed::Ready  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"Ready"`
- **Then** it MUST return `Some(Observed::Ready)`

### Scenario: "Working" maps to Observed::Working  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"Working"`
- **Then** it MUST return `Some(Observed::Working)`

### Scenario: "NeedsInput" maps to Observed::NeedsInput  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"NeedsInput"`
- **Then** it MUST return `Some(Observed::NeedsInput)`

### Scenario: "Finished" maps to Observed::Finished  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"Finished"`
- **Then** it MUST return `Some(Observed::Finished)`

### Scenario: "Failed" maps to Observed::Failed  [unit]
- **Given** the hook-status mapping function
- **When** it is called with `"Failed"`
- **Then** it MUST return `Some(Observed::Failed)`

### Scenario: An unrecognized status string maps to None  [unit]
- **Given** the hook-status mapping function
- **When** it is called with any string not in the five locked values (e.g. `"UNKNOWN"`, `""`)
- **Then** it MUST return `None` (the watcher silently ignores the event, scraping fallback
  continues)

## M3 capability: pipeline-augmentation

Hook-sourced `Observed` events flow through the SAME `observe_and_diff → transition()`
pipeline as PTY-scraped observations. The watcher in `run_signal_loop` reads the state file
on the existing QUIESCE(200ms) tick. `detect_status` stays a pure PTY-only function.
Each event is consumed exactly once.

## Requirement: run_signal_loop reads the state file on QUIESCE ticks and emits Observed  [unit]

`src-tauri/src/session_runtime.rs` MUST augment `run_signal_loop` so that on each QUIESCE
(200ms) tick, it reads the per-session state file (keyed by `SPECTTY_SESSION_ID`). If the
file contains an event with a `ts` STRICTLY GREATER than the last consumed `ts` (initialized
to 0 at loop start), the loop MUST map the status string to `Observed` via the hook-status
mapping table and feed it into `observe_and_diff` — the SAME path as PTY-scraped observations.
After feeding the event, the loop MUST record the consumed `ts` and MUST NOT re-emit the same
event on subsequent ticks. `detect_status` MUST NOT be modified (it stays pure PTY-only).

### Scenario: A new state file event triggers one Observed emission  [unit]
- **Given** the watcher with a fake state-file reader returning `{"status":"Ready","ts":1000}`
  and last-consumed-ts = 0
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST receive EXACTLY ONE `Observed::Ready` AND the consumed-ts
  MUST be updated to 1000 — asserted with the fake reader and a fake `observe_and_diff` sink

### Scenario: Same ts is not re-emitted on a subsequent tick  [unit]
- **Given** the watcher after consuming a `ts=1000` event
- **When** the next QUIESCE tick fires and the state file still reads `{"status":"Ready","ts":1000}`
- **Then** `observe_and_diff` MUST NOT receive a second emission — the event is consumed once

### Scenario: A newer ts supersedes without re-emitting the old one  [unit]
- **Given** the watcher after consuming `ts=1000` and the state file now reads
  `{"status":"Working","ts":2000}`
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST receive `Observed::Working` (ts 2000) and consumed-ts MUST
  be 2000 — the Working event is emitted once

### Scenario: A malformed state file is silently ignored  [unit]
- **Given** the watcher and a state file containing malformed JSON or a missing `status` field
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST NOT receive any emission AND the consumed-ts MUST remain
  unchanged — asserted with a fake reader returning bad JSON

### Scenario: An absent state file on a tick is silently ignored  [unit]
- **Given** the watcher and no state file present at the expected path
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST NOT receive any emission AND no error is returned — the
  absence of the file is a normal condition (no hook fired yet)

## Requirement: Hook-sourced Observed events go through the same transition() authority  [unit]

The `transition()` function MUST remain the sole authority for `AgentStatus`
advancement. An `Observed` derived from a hook event MUST be processed by
`transition(current, observed)` identically to a scrape-derived `Observed`. No hook-specific
bypass or short-circuit of the transition table is permitted.

> **Amendment (M3, 2026-06-10)**: `(AwaitingInput, Ready) => Idle` (updated from the original
> M2 rule `(Running, Ready) => Idle`). Rationale: `Ready` means quiet-at-prompt; resumption
> is always observed as `Working`, never `Ready`. See acceptance.md and 0004-agent-agnostic-core.md.

### Scenario: Hook-derived Ready observation advances Starting to Idle  [unit]
- **Given** `current = Starting` and the watcher emits `Observed::Ready` (from a hook event)
- **When** `transition(Starting, Ready)` runs
- **Then** it MUST return `Idle` (the transition rule `(Starting, Ready) => Idle`)

### Scenario: Hook-derived Working observation advances Running-ish states correctly  [unit]
- **Given** `current = Idle` and the watcher emits `Observed::Working`
- **When** `transition(Idle, Working)` runs
- **Then** it MUST return `Running` (the legal Idle → Running transition)

## Requirement: detect_status stays pure PTY-only and is not modified by M3  [unit]

`ClaudeCodeRunner::detect_status` MUST NOT be modified in M3 to read files, check state, or
incorporate hook data. It MUST remain a pure function over `OutputSignal` only.
Scraping-based detection remains the fallback path when no hook event has fired within the
QUIESCE window.

### Scenario: detect_status signature and purity are unchanged after M3  [unit]
- **Given** `ClaudeCodeRunner::detect_status` after M3 is applied
- **When** its signature and body are inspected
- **Then** it MUST accept only `&self` and `&OutputSignal` and MUST NOT call any filesystem
  function, read any file, or access any session-specific state beyond the signal

## M3 capability: lifecycle

Injection and retraction of settings.json hooks follow the same ordering established for
`mcpServers` in M2. The state file is created by `spectty-hook` at runtime and cleaned up
by `close_session_impl`. The runtime dir is created by `spawn_session_impl` before the PTY
spawns.

## Requirement: spawn_session_impl injects hooks before PTY spawn  [unit]

`spawn_session_impl` MUST call `ClaudeSettingsProvisioner::inject` for the resolved scope
BEFORE calling `PtyAdapter::spawn`. This ordering ensures that when Claude Code starts, the
hooks are already present in settings.json and are loaded at agent startup. The existing
`ClaudeJsonProvisioner::inject` call (for `mcpServers`) MUST NOT be moved; both inject calls
MUST precede `PtyAdapter::spawn`.

### Scenario: Both provisioners inject before PTY spawn  [unit]
- **Given** `spawn_session_impl` wired to a fake `ClaudeJsonProvisioner`, a fake
  `ClaudeSettingsProvisioner`, and a fake `PtyAdapter`
- **When** `spawn_session_impl` runs
- **Then** `ClaudeJsonProvisioner::inject` MUST be called before the PTY spawns AND
  `ClaudeSettingsProvisioner::inject` MUST ALSO be called before the PTY spawns —
  asserted on invocation order with the fakes

### Scenario: spawn_session_impl creates the runtime dir before injection  [unit]
- **Given** `spawn_session_impl` with a fake filesystem
- **When** it runs for a new session
- **Then** the Spectty runtime dir MUST be created (if absent) BEFORE either provisioner injects
  (the hook binary will write there immediately upon first hook fire)

## Requirement: close_session_impl retracts hooks and deletes the state file  [unit]

`close_session_impl` MUST, after killing the PTY (M1 path), retract BOTH provisioners
(`ClaudeJsonProvisioner::retract` AND `ClaudeSettingsProvisioner::retract`) and delete the
per-session state files (`{runtime_dir}/spectty-{id}.state` and `{runtime_dir}/spectty-{id}.state.tmp`
if present). This MUST follow the existing kill-then-retract-then-remove order. A missing state
file at close time MUST be tolerated (not an error).

### Scenario: Close retracts both provisioners after killing the PTY  [unit]
- **Given** `close_session_impl` wired to fake provisioners and a fake state-file deleter
- **When** `close_session_impl` runs
- **Then** PTY kill MUST occur first, then BOTH `retract` calls MUST occur, then state file
  deletion — asserted on invocation order with the fakes

### Scenario: Close tolerates an absent state file  [unit]
- **Given** `close_session_impl` and no `.state` file exists for the session
- **When** `close_session_impl` runs
- **Then** it MUST complete successfully without error — a missing file at close is normal
  (no hook fired before close)

## Requirement: SPECTTY_SESSION_ID is the correlation key between spawn context and hook binary  [unit]

The `SPECTTY_SESSION_ID` env var (already injected into `LaunchSpec.env` by M2's
`ClaudeCodeRunner::launch_spec`) MUST be the sole key correlating the spawned Claude Code
process's hook commands to the Spectty session's state file. NO parsing of Claude's internal
`session_id` field from hook stdin JSON is required or permitted.

### Scenario: LaunchSpec.env carries SPECTTY_SESSION_ID for hook correlation  [unit]
- **Given** `ClaudeCodeRunner::launch_spec` after M3 (unchanged from M2 on this point)
- **When** the resulting `LaunchSpec` is inspected
- **Then** `SPECTTY_SESSION_ID` MUST be present in the env — this is the key the hook binary
  uses to name the state file, asserted on the `LaunchSpec` value with no process spawned

## Requirement: Orphaned settings.json hooks and state files are best-effort mitigated  [unit]

M3 does NOT build full boot-time orphan reconciliation (that is deferred to M4). The M3 concrete
mitigations are: (a) `.spectty.bak` is the manual escape hatch for settings.json, (b) orphaned
`.state` files are harmless (a stale `.state` is never read once its session id is retired — the
watcher keyed to that id is gone), (c) opportunistic sweep at spawn time: if a `.state` file for
the session id already exists at spawn (leftover from a crashed prior run with the same id), it
MUST be deleted before the loop starts so stale events are not replayed.

### Scenario: Stale state file from a crashed prior run is deleted at spawn  [unit]
- **Given** a session being spawned whose `SPECTTY_SESSION_ID` has a leftover `.state` file
  from a prior run
- **When** `spawn_session_impl` sets up the watcher loop for the new session
- **Then** the leftover `.state` file MUST be deleted before the watcher loop starts, asserted
  with a fake filesystem

## M3 capability: bundling

`spectty-hook` and `spectty-mcp` MUST BOTH be configured as Tauri `externalBin` sidecars.
This closes the M2 L2 bundling gap for `spectty-mcp` and establishes the bundling pattern
for `spectty-hook`. Both binaries MUST be resolvable at runtime in a packaged Tauri build via
the `spectty_hook_command()` / `spectty_mcp_command()` pattern in `src-tauri/src/lib.rs`.

## Requirement: Both sidecars are declared as externalBin in tauri.conf.json  [ci]

`src-tauri/tauri.conf.json` MUST declare BOTH `spectty-mcp` AND `spectty-hook` under
`bundle.externalBin` with target-triple-suffixed binary names (matching the Tauri sidecar
convention). A missing `externalBin` entry causes silent failure in packaged builds (the
binary is not shipped).

### Scenario: tauri.conf.json contains both sidecar entries  [ci]
- **Given** `src-tauri/tauri.conf.json` after M3
- **When** `bundle.externalBin` is inspected
- **Then** it MUST contain entries for `spectty-mcp` AND `spectty-hook` (with appropriate
  target-triple suffix patterns), assertable by cargo build + manifest inspection

## Requirement: Runtime path resolution works for both sidecars  [unit]

`src-tauri/src/lib.rs` MUST provide a `spectty_hook_command()` function mirroring the existing
`spectty_mcp_command()` pattern: it resolves the sidecar binary path using `app.path()` (or
the equivalent Tauri v2 resolver) so that it works in both `cargo run` (dev, from the local
`target/` dir) and in a packaged Tauri build (from the bundle resources dir). `ClaudeSettingsProvisioner`
MUST use this resolved path (not a hardcoded path) as the `command` in each injected hook entry.

### Scenario: spectty_hook_command() resolves without panic in dev mode  [unit]
- **Given** the Tauri app handle in a test/dev context
- **When** `spectty_hook_command()` is called
- **Then** it MUST return a non-empty path string without panicking — the resolved path is what
  gets embedded in settings.json hook entries

### Scenario: The injected hook command path matches spectty_hook_command() output  [unit]
- **Given** the output of `inject_spectty_hooks` for a session (with the path injected at
  provision time)
- **When** the `command` field in each managed hook entry is inspected
- **Then** it MUST equal the path returned by `spectty_hook_command()`, not a hardcoded literal
