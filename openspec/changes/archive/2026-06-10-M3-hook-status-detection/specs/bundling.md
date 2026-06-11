# Capability: bundling

> M3 delta spec. MODIFIED capability (extends `provisioning-port` from M2, closes M2 L2)
> established by change `M3-hook-status-detection`. RFC 2119 keywords are normative.
> Full prose + verification-class tags live in the change-level `spec.md`.

Both `spectty-mcp` and `spectty-hook` MUST be configured as Tauri `externalBin` sidecars.
This closes the M2 L2 bundling gap retroactively for `spectty-mcp` and establishes the
`externalBin` pattern for `spectty-hook`. Runtime path resolution uses `spectty_hook_command()`
mirroring the `spectty_mcp_command()` pattern already in `src-tauri/src/lib.rs`.

## MODIFIED Requirements

### Requirement: Both sidecars are declared as externalBin in tauri.conf.json
`src-tauri/tauri.conf.json` MUST declare BOTH `spectty-mcp` AND `spectty-hook` under
`bundle.externalBin` with target-triple-suffixed binary names (Tauri sidecar convention).
A missing entry silently fails in packaged builds.

#### Scenario: tauri.conf.json contains both sidecar entries
- **Given** `src-tauri/tauri.conf.json` after M3
- **When** `bundle.externalBin` is inspected
- **Then** it MUST contain entries for both `spectty-mcp` AND `spectty-hook` with appropriate
  target-triple suffix patterns

### Requirement: Runtime path resolution works for both sidecars
`src-tauri/src/lib.rs` MUST provide `spectty_hook_command()` mirroring `spectty_mcp_command()`,
resolving the sidecar path via the Tauri v2 resolver so it works in both dev (`cargo run`) and
packaged builds. `ClaudeSettingsProvisioner` MUST use this resolved path in injected hook entries.

#### Scenario: spectty_hook_command() resolves without panic in dev mode
- **Given** the Tauri app handle in a test/dev context
- **When** `spectty_hook_command()` is called
- **Then** it MUST return a non-empty path string without panicking

#### Scenario: The injected hook command path matches spectty_hook_command() output
- **Given** the output of `inject_spectty_hooks` for a session
- **When** the `command` field in each managed hook entry is inspected
- **Then** it MUST equal the path returned by `spectty_hook_command()`, not a hardcoded literal
