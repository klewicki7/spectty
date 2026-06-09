# Capability: lifecycle

> M3 delta spec. MODIFIED capability (extends `session-commands` from M2) established by
> change `M3-hook-status-detection`. RFC 2119 keywords are normative. Full prose +
> verification-class tags live in the change-level `spec.md`.

Injection/retraction ordering for the hook provisioner mirrors the M2 `mcpServers` provisioner.
The runtime dir is created at spawn. The state file is deleted at close. `SPECTTY_SESSION_ID`
(already in `LaunchSpec.env`) is the correlation key between spawn context and hook binary.

## MODIFIED Requirements

### Requirement: spawn_session_impl injects hooks before PTY spawn
`spawn_session_impl` MUST call `ClaudeSettingsProvisioner::inject` for the resolved scope
BEFORE `PtyAdapter::spawn`, alongside the existing `ClaudeJsonProvisioner::inject` call.
The runtime dir MUST be created (if absent) before either inject call.

#### Scenario: Both provisioners inject before PTY spawn
- **Given** `spawn_session_impl` wired to fake provisioners and a fake `PtyAdapter`
- **When** `spawn_session_impl` runs
- **Then** BOTH inject calls MUST complete BEFORE the PTY spawn — asserted on invocation order
  with the fakes

#### Scenario: spawn_session_impl creates the runtime dir before injection
- **Given** `spawn_session_impl` with a fake filesystem
- **When** it runs for a new session
- **Then** the Spectty runtime dir MUST be created (if absent) BEFORE either provisioner injects

### Requirement: close_session_impl retracts hooks and deletes the state file
`close_session_impl` MUST, after killing the PTY: retract BOTH provisioners, then delete
`{runtime_dir}/spectty-{id}.state` and `spectty-{id}.state.tmp` if present. Kill-then-retract-then-remove
order is enforced. A missing state file at close MUST be tolerated.

#### Scenario: Close retracts both provisioners after killing the PTY
- **Given** `close_session_impl` wired to fake provisioners and a fake state-file deleter
- **When** `close_session_impl` runs
- **Then** PTY kill MUST occur first, then BOTH `retract` calls, then state file deletion

#### Scenario: Close tolerates an absent state file
- **Given** `close_session_impl` and no `.state` file exists for the session
- **When** `close_session_impl` runs
- **Then** it MUST complete successfully without error

### Requirement: SPECTTY_SESSION_ID is the correlation key between spawn context and hook binary
`SPECTTY_SESSION_ID` (already in `LaunchSpec.env`) is the sole key correlating hook commands
to the session state file. No parsing of Claude's internal `session_id` from hook stdin is
required or permitted (D23).

#### Scenario: LaunchSpec.env carries SPECTTY_SESSION_ID for hook correlation
- **Given** `ClaudeCodeRunner::launch_spec` after M3 (unchanged from M2 on this point)
- **When** the resulting `LaunchSpec` is inspected
- **Then** `SPECTTY_SESSION_ID` MUST be present in the env — asserted on the value with no
  process spawned

### Requirement: Orphaned state files from crashed prior runs are swept at spawn
M3 does NOT implement full boot-time orphan reconciliation. Mitigation: if a `.state` file
for the session id already exists at spawn (leftover from a crashed run), it MUST be deleted
before the watcher loop starts.

#### Scenario: Stale state file from a crashed prior run is deleted at spawn
- **Given** a session being spawned whose id has a leftover `.state` file
- **When** `spawn_session_impl` sets up the watcher loop
- **Then** the leftover `.state` file MUST be deleted before the watcher loop starts —
  asserted with a fake filesystem
