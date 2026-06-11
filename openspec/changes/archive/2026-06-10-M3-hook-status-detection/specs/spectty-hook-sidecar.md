# Capability: spectty-hook-sidecar

> M3 delta spec. New capability established by change `M3-hook-status-detection`.
> RFC 2119 keywords are normative. Full prose + verification-class tags live in the
> change-level `spec.md`.

`spectty-hook` is a NEW standalone binary crate. Claude Code's hook subsystem invokes it on
lifecycle events. It reads `$SPECTTY_SESSION_ID` from its inherited environment and
`--status <STATUS>` from CLI args, then atomically writes a per-session JSON state file under
the Spectty runtime dir. Statically compiled, no shell dependency, no JSON-RPC boot overhead.
Bundled as a Tauri sidecar alongside `spectty-mcp`.

## ADDED Requirements

### Requirement: spectty-hook atomically writes a per-session state file
The `spectty-hook` binary MUST accept `--status <STATUS>` as a CLI argument, read
`SPECTTY_SESSION_ID` from its inherited environment, and atomically write
`{runtime_dir}/spectty-{SPECTTY_SESSION_ID}.state` with JSON content
`{"status": "<STATUS>", "ts": <unix_epoch_seconds>}` using a `.tmp` → rename sequence.
Exit code 0 on success, non-zero on failure.

#### Scenario: spectty-hook writes a valid state file from env + args
_#[cfg(unix)] integration test_
- **Given** `SPECTTY_SESSION_ID=abc123` in the environment and the runtime dir exists
- **When** `spectty-hook --status Ready` is invoked
- **Then** `{runtime_dir}/spectty-abc123.state` MUST exist, contain valid JSON with
  `{"status": "Ready", "ts": <reasonable epoch>}`, written via `.tmp` → rename

#### Scenario: spectty-hook with an unknown status arg exits non-zero
- **Given** `spectty-hook --status BOGUS_VALUE` with a valid env
- **When** the binary runs
- **Then** it MUST exit with a non-zero exit code and MUST NOT write a state file

#### Scenario: spectty-hook exits non-zero when SPECTTY_SESSION_ID is absent
- **Given** `SPECTTY_SESSION_ID` is NOT set in the environment
- **When** `spectty-hook --status Ready` is invoked
- **Then** it MUST exit with a non-zero exit code and MUST NOT write a state file

#### Scenario: spectty-hook exits non-zero when the runtime dir does not exist
- **Given** a non-existent runtime dir and `SPECTTY_SESSION_ID` is set
- **When** `spectty-hook --status Ready` is invoked
- **Then** it MUST exit with a non-zero exit code — the binary does NOT create the dir

### Requirement: spectty-hook accepts all five mapped status values
The binary MUST accept exactly `Working`, `Ready`, `NeedsInput`, `Finished`, `Failed` as valid
`--status` values. No other values MUST be accepted.

#### Scenario: Each valid status value writes a state file
- **Given** a valid environment and runtime dir
- **When** `spectty-hook --status <VALUE>` is invoked for each of the five values
- **Then** each invocation MUST produce a valid state file containing the matching status string
