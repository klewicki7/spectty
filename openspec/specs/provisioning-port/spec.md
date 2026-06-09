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
