# Capability: provisioning-port

> Living baseline spec. Established by change `M2-spawn-agent-provisioner` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The `ProvisioningPort` (Core trait) + `ProvisionerAdapter` implement M2 Layer-1 ONLY: register
the `spectty_*` MCP tools in the agent's config on session create, retract them on close,
backed by a registered-but-stubbed `spectty-mcp` server. The port is SEPARATE from
`AgentRunner` (Lock 1). All config-format/file-IO/agent-name knowledge lives in the adapter.

## Requirement: ProvisioningPort is a Core trait separate from AgentRunner
`spectty-core` MUST define a `ProvisioningPort` trait (distinct from `AgentRunner`) exposing
at minimum `inject(scope)` and `retract(scope)`, agent-agnostic (no agent name, no config
path in Core). Generic-tier sessions are simply not wired to a provisioner.

### Scenario: ProvisioningPort exposes inject and retract and is its own port
- **Given** the `spectty-core` `ProvisioningPort` trait after M2
- **When** its methods are inspected
- **Then** `inject` and `retract` MUST be present AND `ProvisioningPort` MUST be a DISTINCT
  trait from `AgentRunner`

## Requirement: The JSON managed-namespace editor owns only spectty_* keys
The Provisioner adapter MUST implement a PURE `String -> String` JSON editor that registers
`spectty_*` entries under `mcpServers` and owns ONLY the `spectty_*` namespace. It MUST NOT
use text markers and MUST NOT shell out to `claude mcp add`. Foreign keys MUST round-trip
untouched; `retract` removes only `spectty_*` keys.

### Scenario: inject adds spectty_* keys and leaves foreign keys untouched
- **Given** a JSON config string containing a user `mcpServers` entry and a `gentle-ai` entry
- **When** the pure editor injects the `spectty_*` registration
- **Then** the output MUST contain the new `spectty_*` keys AND the user and `gentle-ai`
  entries MUST be structurally preserved, asserted as a pure `String -> String` unit

### Scenario: retract removes only spectty_* keys
- **Given** a JSON config string with `spectty_*` keys plus foreign keys
- **When** the editor retracts the `spectty_*` namespace
- **Then** all `spectty_*` keys MUST be gone AND every foreign key MUST remain

### Scenario: Editing malformed or missing mcpServers is handled, not corrupting
- **Given** a config string with no `mcpServers` object
- **When** the editor injects the `spectty_*` registration
- **Then** it MUST create a valid `mcpServers` object containing the `spectty_*` keys AND
  produce valid JSON

## Requirement: Config writes are atomic with a backup
The Provisioner MUST write behind an atomic-write file-IO seam (temp → fsync → rename) and
copy the original to `<file>.spectty.bak` before the first write. The seam MUST be injectable
so the behavior is testable with a fake filesystem.

### Scenario: First write creates a .spectty.bak backup
- **Given** an existing config file and the atomic-write seam backed by a fake filesystem
- **When** the Provisioner performs its first write to that file
- **Then** a `<file>.spectty.bak` copy of the ORIGINAL contents MUST exist AND the final write
  MUST land via temp-file-then-rename

### Scenario: A crash mid-write never leaves a partial config
- **Given** the atomic-write seam
- **When** a write is interrupted before the rename completes (simulated)
- **Then** the original config file MUST remain intact, so the agent's startup is never broken

## Requirement: Scope resolves to GLOBAL by default, PROJECT when git-tracked
The Provisioner MUST resolve scope via an INJECTED `is_git_tracked(path) -> bool` predicate
(not a full `GitPort`), defaulting to GLOBAL (`~/.claude.json` top-level `mcpServers`) and
resolving to PROJECT (`.mcp.json` at repo root) when the config file is git-tracked. Pure
function over the injected predicate.

### Scenario: Git-tracked config resolves to PROJECT scope
- **Given** the scope resolver with a fake `is_git_tracked` predicate returning true
- **When** scope is resolved for an agent config path
- **Then** it MUST resolve to PROJECT scope targeting `.mcp.json` at the repo root

### Scenario: Untracked or unknown config resolves to GLOBAL scope
- **Given** the scope resolver with a fake predicate returning false (or unavailable)
- **When** scope is resolved
- **Then** it MUST default to GLOBAL scope targeting `~/.claude.json` top-level `mcpServers`

## Requirement: The spectty-mcp server ships registered-but-stubbed advertising five tool schemas
M2 MUST ship a real `spectty-mcp` binary that exists, starts over stdio, and advertises the
five protocol tool schemas — `spectty_spec`, `spectty_diff`, `spectty_approval`,
`spectty_status`, `spectty_cost` — with the registered entry pointing at it. Tool EFFECTS are
M3: in M2 each call returns a benign acknowledgement with no side effect. The advertised
schema is the forward-compatible contract.

### Scenario: The stub server advertises exactly the five tool schemas
- **Given** the `spectty-mcp` stub server started over stdio
- **When** its advertised tool list is requested
- **Then** it MUST advertise `spectty_spec`, `spectty_diff`, `spectty_approval`,
  `spectty_status`, and `spectty_cost` with their declared input schemas

### Scenario: A stub tool call returns an acknowledgement with no side effect
- **Given** the stub server
- **When** any `spectty_*` tool is invoked
- **Then** it MUST return a benign acknowledgement AND MUST NOT persist a spec, trigger a
  diff, resolve an approval, or mutate any session state

### Scenario: The injected config entry points at the existing stub binary
- **Given** a spawned Claude Code session with the managed `spectty_*` registration injected
- **When** the Claude Code config is inspected and the agent starts
- **Then** the managed section with the MCP tools MUST be present AND Claude Code MUST start
  successfully (the registered binary exists and speaks stdio MCP)

## Requirement: Provisioning is injected on create and retracted on close
On session CREATE for a Cooperative agent, `src-tauri` MUST call `inject` for the resolved
scope around the PTY spawn; on CLOSE it MUST kill the PTY (M1 path) AND call `retract`. The
wiring MUST be testable against a fake Provisioner.

### Scenario: Close retracts the managed section after killing the PTY
- **Given** a created Cooperative session wired to a fake Provisioner
- **When** the session is closed
- **Then** the PTY MUST be killed AND `retract` MUST be invoked for the resolved scope,
  asserted against the fake without a real config file

---

## M3 capability: hook-provisioning

The `ClaudeSettingsProvisioner` is a SECOND `ProvisioningPort` impl, operating on
`~/.claude/settings.json` (Global) or `{project}/.claude/settings.json` (Project). It manages
ONLY the `hooks` key using the same M2 `ConfigFile` atomic-write seam, `.spectty.bak` backup,
and foreign-key preservation invariant (R7). The Core `ProvisioningPort` trait is UNCHANGED.

## Requirement: ClaudeSettingsProvisioner manages the hooks section of settings.json  [unit]

`crates/adapters` MUST provide a `ClaudeSettingsProvisioner` that implements `ProvisioningPort`
(the existing M2 Core trait, UNCHANGED). It MUST manage ONLY the `hooks` top-level key in
`~/.claude/settings.json` (Global) or `{project}/.claude/settings.json` (Project). It MUST
NOT touch `mcpServers`, `permissions`, `env`, `model`, or any other key in settings.json.
The managed key is `hooks` and the managed sub-entries are keyed by hook event name in the
`spectty_*` namespace.

### Scenario: ClaudeSettingsProvisioner implements ProvisioningPort without trait change  [unit]
- **Given** the `ProvisioningPort` trait after M2 (unchanged)
- **When** `ClaudeSettingsProvisioner` is inspected for trait conformance
- **Then** it MUST implement `inject(scope)` and `retract(scope)` matching the existing trait
  signature with no new methods required on the Core trait

### Scenario: inject adds managed hook entries and leaves foreign keys untouched  [unit]
- **Given** a settings.json string containing user-authored `hooks`, a `permissions` key, and
  a `model` key (diverse foreign content)
- **When** `inject_spectty_hooks` is called on that string (the pure namespace editor)
- **Then** the output MUST contain the new Spectty-managed hook entries for each configured
  event AND every foreign key (`permissions`, `model`, and any existing user `hooks` sub-entries
  not managed by Spectty) MUST be present and structurally unchanged — asserted as a pure
  `String -> String` unit with no file-IO

### Scenario: retract removes only Spectty-managed hook entries  [unit]
- **Given** a settings.json string containing both Spectty-managed hook entries and user-authored
  hook entries under the same or different event names
- **When** `retract_spectty_hooks` is called on that string
- **Then** all Spectty-managed entries MUST be absent AND every user-authored hook entry MUST
  be present and structurally unchanged — asserted as a pure unit

### Scenario: Editing absent or empty hooks section creates valid output  [unit]
- **Given** a settings.json string with no `hooks` key (or an empty document `{}`)
- **When** `inject_spectty_hooks` is called
- **Then** the output MUST be valid JSON containing a `hooks` object with the Spectty-managed
  entries AND all other absent keys MUST remain absent (no key creation side-effects)

### Scenario: retract on a settings.json that has no Spectty hooks is idempotent  [unit]
- **Given** a settings.json with no Spectty-managed hook entries (fresh file or already retracted)
- **When** `retract_spectty_hooks` is called
- **Then** the output MUST equal the input structurally (no keys added or removed) AND MUST
  remain valid JSON — asserted as a pure unit

## Requirement: Settings.json scope path resolves correctly for Global and Project  [unit]

The `ClaudeSettingsProvisioner` MUST resolve `ProvisioningScope::Global` to
`~/.claude/settings.json` and `ProvisioningScope::Project(root)` to
`{root}/.claude/settings.json`. This is a DISTINCT path mapping from the M2 `ClaudeJsonProvisioner`
(which resolves Global to `~/.claude.json` and Project to `{root}/.mcp.json`). The path
resolution MUST be a pure function asserted without touching the filesystem. The existing
injected `is_git_tracked` predicate (M2) governs scope selection upstream; the settings path
resolver just maps the chosen scope to its file path.

### Scenario: Global scope resolves to ~/.claude/settings.json  [unit]
- **Given** the settings path resolver with `ProvisioningScope::Global`
- **When** the resolver runs
- **Then** it MUST return the path `~/.claude/settings.json` (expanded) with no filesystem access

### Scenario: Project scope resolves to {root}/.claude/settings.json  [unit]
- **Given** the settings path resolver with `ProvisioningScope::Project("/some/repo")`
- **When** the resolver runs
- **Then** it MUST return `/some/repo/.claude/settings.json` with no filesystem access

## Requirement: Settings.json writes are atomic with a one-time .spectty.bak backup  [unit]

`ClaudeSettingsProvisioner` MUST use the same M2 `ConfigFile` atomic-write seam (temp file →
fsync → atomic rename) for all writes to settings.json. Before the FIRST write to a given
settings.json path, it MUST copy the existing file to `<path>.spectty.bak`. The seam MUST be
injectable so backup + atomic-write behavior is testable with a fake filesystem, matching the
M2 `ClaudeJsonProvisioner` contract exactly.

### Scenario: First write creates a .spectty.bak backup of the original settings.json  [unit]
- **Given** an existing settings.json with user content and the atomic-write seam backed by a
  fake filesystem
- **When** `ClaudeSettingsProvisioner` performs its first write (inject call)
- **Then** a `<settings-path>.spectty.bak` copy of the ORIGINAL contents MUST exist AND the
  written file MUST land via temp-file-then-rename — asserted on the fake filesystem operations

### Scenario: Subsequent writes do not overwrite an existing .spectty.bak  [unit]
- **Given** a settings.json where a `.spectty.bak` already exists (from a prior inject)
- **When** `ClaudeSettingsProvisioner` performs a second write (e.g. retract then re-inject)
- **Then** the `.spectty.bak` MUST NOT be overwritten — the original pre-Spectty state is
  preserved as the escape hatch
