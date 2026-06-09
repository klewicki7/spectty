# Capability: hook-provisioning

> M3 delta spec. New capability established by change `M3-hook-status-detection`.
> RFC 2119 keywords are normative. Full prose + verification-class tags live in the
> change-level `spec.md`.

`ClaudeSettingsProvisioner` is a SECOND `ProvisioningPort` impl managing the `hooks` section
of `~/.claude/settings.json` (Global) or `{project}/.claude/settings.json` (Project). It reuses
the M2 `ConfigFile` atomic-write seam, `.spectty.bak` backup, and foreign-key preservation (R7),
operating on a DIFFERENT file from `ClaudeJsonProvisioner`. The Core `ProvisioningPort` trait
is UNCHANGED.

## ADDED Requirements

### Requirement: ClaudeSettingsProvisioner manages the hooks section of settings.json
`crates/adapters` MUST provide `ClaudeSettingsProvisioner` implementing the existing
`ProvisioningPort` trait (UNCHANGED) to manage ONLY the `hooks` top-level key in settings.json.
It MUST NOT touch `mcpServers`, `permissions`, `env`, `model`, or any other key. The managed
sub-entries live in the `spectty_*` namespace of the `hooks` object.

#### Scenario: ClaudeSettingsProvisioner implements ProvisioningPort without trait change
- **Given** the `ProvisioningPort` trait after M2 (unchanged)
- **When** `ClaudeSettingsProvisioner` is inspected for trait conformance
- **Then** it MUST implement `inject(scope)` and `retract(scope)` matching the existing trait
  signature with no new methods required on the Core trait

#### Scenario: inject adds managed hook entries and leaves foreign keys untouched
- **Given** a settings.json string containing user-authored hooks, a `permissions` key, and
  a `model` key
- **When** `inject_spectty_hooks` is called on that string
- **Then** the output MUST contain the Spectty-managed hook entries AND every foreign key and
  user hook MUST be present and structurally unchanged — asserted as a pure `String -> String`
  unit with no file-IO

#### Scenario: retract removes only Spectty-managed hook entries
- **Given** a settings.json string containing both Spectty-managed and user-authored hook entries
- **When** `retract_spectty_hooks` is called
- **Then** all Spectty-managed entries MUST be absent AND every user-authored entry MUST remain
  structurally unchanged — asserted as a pure unit

#### Scenario: Editing absent or empty hooks section creates valid output
- **Given** a settings.json string with no `hooks` key (or `{}`)
- **When** `inject_spectty_hooks` is called
- **Then** the output MUST be valid JSON containing a `hooks` object with the Spectty entries
  AND all other absent keys MUST remain absent

#### Scenario: retract on a settings.json with no Spectty hooks is idempotent
- **Given** a settings.json with no Spectty-managed hook entries
- **When** `retract_spectty_hooks` is called
- **Then** the output MUST equal the input structurally AND MUST remain valid JSON

### Requirement: Settings.json scope path resolves correctly for Global and Project
`ClaudeSettingsProvisioner` MUST resolve `ProvisioningScope::Global` to
`~/.claude/settings.json` and `ProvisioningScope::Project(root)` to
`{root}/.claude/settings.json`. This path mapping is DISTINCT from the M2
`ClaudeJsonProvisioner` paths. Resolution MUST be a pure function asserted without filesystem access.

#### Scenario: Global scope resolves to ~/.claude/settings.json
- **Given** the settings path resolver with `ProvisioningScope::Global`
- **When** the resolver runs
- **Then** it MUST return the expanded `~/.claude/settings.json` path with no filesystem access

#### Scenario: Project scope resolves to {root}/.claude/settings.json
- **Given** the settings path resolver with `ProvisioningScope::Project("/some/repo")`
- **When** the resolver runs
- **Then** it MUST return `/some/repo/.claude/settings.json` with no filesystem access

### Requirement: Settings.json writes are atomic with a one-time .spectty.bak backup
`ClaudeSettingsProvisioner` MUST use the M2 `ConfigFile` atomic-write seam (temp → fsync →
rename). Before the FIRST write to a given path, it MUST copy the original to
`<path>.spectty.bak`. Injectable seam required for fake-filesystem unit testing.

#### Scenario: First write creates a .spectty.bak backup of the original settings.json
- **Given** an existing settings.json and the atomic-write seam backed by a fake filesystem
- **When** `ClaudeSettingsProvisioner` performs its first write
- **Then** `<settings-path>.spectty.bak` MUST hold the ORIGINAL contents AND the write MUST
  land via temp-file-then-rename — asserted on the fake filesystem operations

#### Scenario: Subsequent writes do not overwrite an existing .spectty.bak
- **Given** a settings.json where `.spectty.bak` already exists
- **When** `ClaudeSettingsProvisioner` performs a second write
- **Then** the `.spectty.bak` MUST NOT be overwritten — the original pre-Spectty state is
  preserved as the escape hatch
