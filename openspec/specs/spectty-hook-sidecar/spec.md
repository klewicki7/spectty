# Capability: spectty-hook-sidecar

> Living baseline spec. Established by change `M3-hook-status-detection` (archived 2026-06-10).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

`spectty-hook` is a standalone binary sidecar that fires synchronously when Claude Code's hook
subsystem triggers a lifecycle event. It writes a per-session JSON state file to the Spectty
runtime directory, correlating events back to the running session via `SPECTTY_SESSION_ID`.

## Requirement: spectty-hook atomically writes a per-session state file  [unit]

The `spectty-hook` binary MUST accept `--status <STATUS>` as a CLI argument. It MUST read
`SPECTTY_SESSION_ID` from its inherited environment. It MUST atomically write
`{runtime_dir}/spectty-{SPECTTY_SESSION_ID}.state` with JSON content
`{"status": "<STATUS>", "ts": <unix_epoch_seconds>}`, where the write uses a `.tmp` → rename
sequence (same atomic pattern as the provisioner). The binary MUST return exit code 0 on
success and a non-zero exit code on failure.

### Scenario: spectty-hook writes a valid state file from env + args  [unit] (#[cfg(unix)])
- **Given** `SPECTTY_SESSION_ID=abc123` in the environment, the runtime dir exists, and
  `spectty-hook --status Ready` is invoked
- **When** the binary runs
- **Then** `{runtime_dir}/spectty-abc123.state` MUST exist, contain valid JSON with
  `{"status": "Ready", "ts": <a reasonable unix epoch>}`, and the file MUST have been written
  via `.tmp` → rename (no partial-write observable), asserted in an integration test

### Scenario: spectty-hook with unknown status arg exits non-zero  [unit]
- **Given** `spectty-hook --status BOGUS_VALUE` invoked with a valid env
- **When** the binary runs
- **Then** it MUST exit with a non-zero exit code and MUST NOT write a state file
  (malformed writes are not observable to the watcher)

### Scenario: spectty-hook exits non-zero when SPECTTY_SESSION_ID is absent  [unit]
- **Given** `SPECTTY_SESSION_ID` is NOT set in the environment
- **When** `spectty-hook --status Ready` is invoked
- **Then** it MUST exit with a non-zero exit code and MUST NOT write a state file

### Scenario: spectty-hook exits non-zero when the runtime dir does not exist  [unit]
- **Given** a non-existent runtime dir and `SPECTTY_SESSION_ID` is set
- **When** `spectty-hook --status Ready` is invoked
- **Then** it MUST exit with a non-zero exit code (the binary does NOT create the dir — that
  is the responsibility of the host process at session spawn)

## Requirement: spectty-hook accepts all five mapped status values  [unit]

The binary MUST accept the following STATUS values as valid arguments (mapping to the five
locked hook events): `Working`, `Ready`, `NeedsInput`, `Finished`, `Failed`. These are the
STATUS strings the watcher maps back to `Observed` variants. No other values MUST be accepted.

### Scenario: Each valid status value writes a state file  [unit]
- **Given** a valid environment and runtime dir
- **When** `spectty-hook --status <VALUE>` is invoked for each of `Working`, `Ready`,
  `NeedsInput`, `Finished`, `Failed`
- **Then** each invocation MUST produce a valid state file containing the matching status string
