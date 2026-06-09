# M2 — Spawn Agent + Provisioner — Delta Spec

> SDD spec phase. Consumes `sdd/M2-spawn-agent-provisioner/proposal` (obs #800) and
> `openspec/changes/M2-spawn-agent-provisioner/proposal.md`, plus the AUTHORITATIVE roadmap
> M2 exit criteria (`docs/product/roadmap.md` → "M2 — Spawn Agent + Provisioner"), the
> agent-protocol.md tool suite, ADR-0004 (agent-agnostic core), and domain-model.md.
> Drives `sdd-tasks` (with `sdd-design`). Artifact store: HYBRID
> (engram `sdd/M2-spawn-agent-provisioner/spec` + this file + per-capability files under
> this directory).
>
> This is a DELTA spec: it states WHAT MUST be true after M2 is applied, on top of the
> archived M0+M1 baseline (`hexagonal-core`, `tauri-bridge`, `persistence-port`,
> `pty-adapter`, `pty-bridge`, `terminal-ui`). It describes outcomes, NOT implementation.
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.
>
> Each requirement is tagged with its verification class:
> - **[unit]** — assertable under Strict TDD (`cargo test --workspace` / `pnpm -C ui test`)
>   without a real PTY, a real agent, or a running app. These seed the Strict-TDD task list.
> - **[manual]** — real-app / real-Claude-Code manual acceptance check; maps to a roadmap M2
>   exit criterion; the `sdd-verify` pass/fail gate.
> - **[ci]** — enforced by existing CI gates (cargo build, clippy, cargo-deny).

M1 delivered a real terminal over a real PTY: a raw byte-pump that supervises nothing. M2
turns it into the cockpit's first instrument: launch a real AI CLI agent inside the PTY,
detect its lifecycle state, and inject the Spectty Agent Protocol's MCP tools into the
agent's config. M2 MUST hold the agent-agnostic boundary (ADR-0004): the Core gains
`AgentRunner`, `ProvisioningPort`, `OutputSignal`, `AgentSpec`, the pure `transition`
function, `Session`, and `SessionRegistry` — and ZERO agent names, config-format knowledge,
ANSI/regex parsing, or file-IO. All of those live in `crates/adapters` / `src-tauri`.

This delta spec is organized by capability. The full text lives here; the per-capability
files in this directory (`agent-runner.md`, `output-signal.md`, `session-registry.md`,
`provisioning-port.md`, `agent-session-ui.md`, `hexagonal-core.md`) mirror these sections
for the `openspec/specs/` promotion at archive time.

---

## Capability: agent-runner

The `AgentRunner` port (Core trait) abstracts every supported AI CLI agent behind ONE
interface. M2 ships two adapters — `ClaudeCodeRunner` (Cooperative tier) and `GenericRunner`
(Generic tier) — in `crates/adapters`, plus the pure Core `AgentStatus` state machine.

### Requirement: AgentRunner is a Core port with the M2 method subset  [unit]
`spectty-core` MUST define an `AgentRunner` trait with the M2 method subset FULLY
implementable: `launch_spec(ctx) -> LaunchSpec` (program / args / env / cwd), `detect_status(&OutputSignal) -> Option<AgentStatus>`, `descriptor() -> AgentDescriptor`, and `tier() -> AgentTier`. The
trait MUST also declare `parse_cost` and `quick_actions` as M2 SKELETON methods (present,
typed, honestly stubbed, and tested as skeletons — they MUST NOT pretend to parse real cost
or answer prompts in M2). The trait MUST NOT carry provisioning: `ProvisioningPort` is a
SEPARATE Core port (Lock 1), so `AgentRunner` MUST NOT expose a `provisioner()` method (this
explicitly supersedes the ADR-0004 `AgentRunner::provisioner()` method shape for M2, R9).

#### Scenario: AgentRunner exposes the four full methods plus two skeleton methods
- **Given** the `spectty-core` `AgentRunner` trait after M2
- **When** its method set is inspected
- **Then** `launch_spec`, `detect_status`, `descriptor`, and `tier` MUST each be present and
  fully specified, AND `parse_cost` and `quick_actions` MUST each be present as typed
  skeletons, AND there MUST be NO `provisioner()` method on the trait

#### Scenario: AgentRunner carries no agent name or config-format knowledge  [ci]
- **Given** the `spectty-core` crate after M2
- **When** the `AgentRunner` trait, `AgentSpec`, `AgentTier`, `AgentDescriptor`, and
  `LaunchSpec` definitions are inspected
- **Then** none MUST contain an agent-name literal (no `"claude"`, no `"bash"`), a config
  file path, or any ANSI/regex parsing, AND the Core MUST remain `serde` + `thiserror` only

### Requirement: AgentSpec and descriptor types are Core, agent-agnostic value types  [unit]
`spectty-core` MUST define `AgentSpec` (the persisted, serde value identifying which agent a
session runs and its parameters), `AgentTier` (at minimum `Cooperative` and `Generic`), and
`AgentDescriptor` (display name + tier + capability hints for the UI). These MUST be plain
serde value types with no behavior that branches on a hard-coded agent name.

#### Scenario: AgentSpec round-trips through serde
- **Given** an `AgentSpec` value
- **When** it is serialized and deserialized
- **Then** the round-tripped value MUST equal the original (so it can be persisted on the
  `Session` aggregate and restored)

#### Scenario: AgentTier distinguishes Cooperative from Generic
- **Given** the `AgentTier` enum
- **When** a runner reports its tier via `tier()`
- **Then** `ClaudeCodeRunner` MUST report `Cooperative` AND `GenericRunner` MUST report
  `Generic`, asserted without spawning either agent

### Requirement: ClaudeCodeRunner produces a correct LaunchSpec  [unit]
`ClaudeCodeRunner` (in `crates/adapters`) MUST implement `launch_spec` so that, given a
launch context (workspace cwd, session id, resolved MCP-config scope), it produces a
`LaunchSpec` invoking the Claude Code CLI in the given workspace directory. The mapping MUST
be a pure, testable function — asserted on the produced `LaunchSpec` fields, NOT by spawning
Claude Code.

#### Scenario: launch_spec maps context to program, cwd, and env
- **Given** a launch context carrying a workspace directory and a session id
- **When** `ClaudeCodeRunner::launch_spec` runs
- **Then** the resulting `LaunchSpec` MUST name the Claude Code program, set `cwd` to the
  workspace directory, AND carry any session-identifying env (e.g. `SPECTTY_SESSION_ID`),
  asserted on the `LaunchSpec` value with no real process spawned

### Requirement: ClaudeCodeRunner detects status by scraping known patterns  [unit]
`ClaudeCodeRunner::detect_status` MUST map an `OutputSignal` to an observed `AgentStatus` by
matching a list of EMPIRICAL Claude Code output patterns (R5): a ready/prompt signal →
`Idle`; active-work signals → `Running`; a permission-prompt / "Do you want…" pattern →
`AwaitingInput`; a completion signal → `Completed`; an error signal → `Error`. The pattern
list MUST live as DATA inside `ClaudeCodeRunner` (a table of patterns), NOT as Core logic,
so the patterns are testable in isolation and revisable without touching Core. When no
pattern matches, `detect_status` MUST return `None` (no observation, leave status unchanged).

#### Scenario: Permission-prompt output is observed as AwaitingInput
- **Given** an `OutputSignal` whose text window contains a Claude Code permission-prompt
  pattern from the runner's pattern table
- **When** `ClaudeCodeRunner::detect_status` runs
- **Then** it MUST return `Some(AwaitingInput)`, asserted as a pure function over a
  constructed `OutputSignal` with no real PTY

#### Scenario: Unrecognized output yields no observation
- **Given** an `OutputSignal` whose text window matches none of the runner's patterns
- **When** `ClaudeCodeRunner::detect_status` runs
- **Then** it MUST return `None` so the state machine leaves the current status untouched

### Requirement: GenericRunner detects Idle and idle-timeout Completed via injected time  [unit]
`GenericRunner::detect_status` MUST implement the Generic-tier baseline: first activity after
spawn → `Idle`; ongoing activity → `Running`; and after a CONFIGURABLE inactivity window with
no new output → `Completed`. The inactivity decision MUST be driven by an INJECTED time seam
(`ClockPort`-style / the `OutputSignal` non-`Instant` time field), NOT by reading a wall
clock directly, so the idle-timeout is RED→GREEN testable with a fake clock. The Generic
adapter advertises NO `spectty_*` tools and performs NO config injection.

#### Scenario: GenericRunner transitions to Completed after the configured idle window
- **Given** a `GenericRunner` with a configured idle-timeout and an injected fake clock
- **When** an `OutputSignal` reports no new activity for at least the configured window
- **Then** `detect_status` MUST return `Some(Completed)`, asserted deterministically with the
  fake clock and no real process

#### Scenario: GenericRunner reaches Idle on first output  [manual]
- **Given** a `GenericRunner` spawning `bash`
- **When** the shell starts and produces its first prompt output
- **Then** status MUST reach `Idle` (maps to roadmap exit criterion 5, first half)

### Requirement: parse_cost and quick_actions ship as honest, tested skeletons  [unit]
For M2, `parse_cost` and `quick_actions` on BOTH runners MUST be skeletons: `parse_cost` MUST
return an empty / zero `CostMetrics` delta (no real regex cost extraction — that is M3), and
`quick_actions` MUST return an empty or static action set (no real prompt-answering — that is
M3). Each skeleton MUST have a test asserting its skeleton behavior so the seam exists and is
honestly documented as not-yet-implemented.

#### Scenario: parse_cost returns a zero/empty delta in M2
- **Given** any `OutputSignal`
- **When** a runner's `parse_cost` is called in M2
- **Then** it MUST return an empty or zero cost delta (the skeleton contract), NOT a parsed
  value, AND a test MUST assert this skeleton behavior

---

## Capability: agent-status-machine

The `AgentStatus` lifecycle is governed by a PURE Core function. Detection (impure scraping)
lives in the runner adapters; the transition RULES live in Core.

### Requirement: AgentStatus enum carries the full M2 lifecycle  [unit]
`spectty-core` MUST define `AgentStatus` with the variants `Starting`, `Idle`, `Running`,
`AwaitingInput`, `Completed`, and `Error`. This grows the M0 placeholder `AgentStatus` into a
behavior-bearing domain type. It MUST be a serde value type carried on the `Session` aggregate.

#### Scenario: AgentStatus exposes all six lifecycle variants
- **Given** the `spectty-core` `AgentStatus` enum after M2
- **When** its variants are inspected
- **Then** `Starting`, `Idle`, `Running`, `AwaitingInput`, `Completed`, and `Error` MUST all
  be present

### Requirement: A pure Core transition function enforces legal transitions  [unit]
`spectty-core` MUST expose a PURE function `transition(current: AgentStatus, observed:
AgentStatus) -> AgentStatus` that enforces the legal lifecycle and rejects illegal jumps. The
legal transitions are: `Starting → Idle`; `Idle → Running`; `Running → AwaitingInput`,
`Running → Completed`, `Running → Error`; `AwaitingInput → Running` (after input is given);
and any state MAY go to `Error`. An illegal observed transition MUST leave `current`
unchanged (the function returns `current`), so a spurious detector observation cannot drive
an impossible state. The function MUST be deterministic, total, and contain NO agent name,
NO I/O, and NO time access.

#### Scenario: Running → AwaitingInput → Running is legal (the permission-prompt round trip)
- **Given** `current = Running`
- **When** `transition(Running, AwaitingInput)` then `transition(AwaitingInput, Running)` run
- **Then** the first MUST yield `AwaitingInput` AND the second MUST yield `Running`
  (mapping the roadmap exit criterion 3 lifecycle, asserted as a pure unit)

#### Scenario: An illegal jump is rejected and leaves current unchanged
- **Given** `current = Starting`
- **When** `transition(Starting, Completed)` runs (an illegal jump skipping Idle/Running)
- **Then** it MUST return `Starting` unchanged (the illegal observation is ignored)

#### Scenario: Any state may transition to Error
- **Given** any `current` status
- **When** `transition(current, Error)` runs
- **Then** it MUST return `Error` (error is reachable from every state)

#### Scenario: Starting reaches Idle on the first Idle observation
- **Given** `current = Starting`
- **When** `transition(Starting, Idle)` runs
- **Then** it MUST return `Idle` (mapping roadmap exit criterion 1's "reaches Idle")

---

## Capability: output-signal

`OutputSignal` is the Core value type that crosses the port boundary into `detect_status`.
Its PRODUCER (ANSI strip + rolling window) is impure adapter code on a SECOND, independent
consumer of the PTY read stream.

### Requirement: OutputSignal is a Core serde value type with a non-Instant time field  [unit]
`spectty-core` MUST define `OutputSignal` as a serde value type carrying at minimum: an
ANSI-STRIPPED rolling text window (the recent output, plain text), an activity indicator,
an optional process exit code, and a SERDE-FRIENDLY time field (elapsed-millis-since-last-byte
or an injected `Timestamp` — NEVER `std::time::Instant`, which is neither serde-serializable
nor comparable across the boundary, Lock 2). `OutputSignal` MUST be constructible in a test
WITHOUT a PTY so `detect_status` is a pure unit.

#### Scenario: OutputSignal round-trips through serde and carries no Instant
- **Given** an `OutputSignal` value
- **When** it is serialized and deserialized
- **Then** the round-trip MUST succeed AND the time field MUST be a serde-friendly value
  (millis or `Timestamp`), NOT `std::time::Instant`

#### Scenario: OutputSignal is constructible without a PTY (detector test seam)
- **Given** a test that needs to drive `detect_status`
- **When** it constructs an `OutputSignal` directly (text window + activity + time)
- **Then** the construction MUST succeed with no real PTY, no real process, and no ANSI bytes
  required (the stripper already ran in the producer)

### Requirement: The OutputSignal producer runs on an independent read-stream consumer  [unit]
The `OutputSignal` PRODUCER (ANSI stripping + rolling-window assembly) MUST live in
`crates/adapters` and MUST be driven from a SECOND consumer of the PTY read stream — a path
INDEPENDENT of the M1 raw `pty_output` Channel (R6). The producer MUST NOT be able to
back-pressure or throttle the M1 render path: its buffer MUST be bounded and MUST drop oldest
data on overflow rather than blocking the read loop. The ANSI-strip + rolling-window assembly
MUST be a pure unit testable on raw byte input without a PTY.

#### Scenario: The producer strips ANSI and maintains a bounded rolling window
- **Given** a sequence of raw byte chunks containing ANSI escape sequences fed to the producer
- **When** the producer assembles the rolling window
- **Then** the resulting `OutputSignal` text window MUST contain the plain text with ANSI
  sequences removed AND MUST NOT exceed the configured rolling-window size (older text is
  dropped), asserted as a pure unit with no PTY

#### Scenario: The producer cannot back-pressure the M1 render Channel  [unit]
- **Given** the second read-stream consumer feeding the producer with a bounded buffer
- **When** the producer falls behind (buffer would overflow)
- **Then** it MUST drop the oldest buffered data (drop-oldest) rather than block, so the M1
  `pty_output` render path is never throttled — asserted on the buffer seam without a PTY

---

## Capability: session-registry

The `Session` aggregate root and the `SessionRegistry` live in Core, following the M0/M1
`PersistencePort`-style `&self` interior-mutability convention, shared as `tauri::State`.

### Requirement: Session aggregate carries the M2 fields  [unit]
`spectty-core` MUST grow `Session` from the M0 placeholder into an aggregate root carrying at
minimum: `id: SessionId`, `workspace: WorkspaceId`, `agent: AgentSpec`, `status: AgentStatus`,
`title`, and `created_at`. `Spec`, `CostMetrics`, `Worktree`, and `last_diff` MAY be present
as skeleton/stub fields or deferred — M2 MUST NOT implement their behavior (those are M3/M4).
`Session` MUST be a Core type with no agent name, no I/O, and no file-format knowledge.

#### Scenario: Session exposes id, workspace, agent, status, title, created_at
- **Given** the `spectty-core` `Session` aggregate after M2
- **When** its fields are inspected
- **Then** `id`, `workspace`, `agent`, `status`, `title`, and `created_at` MUST each be
  present (with `CostMetrics` permitted as a skeleton field)

### Requirement: SessionRegistry creates, looks up, and closes sessions  [unit]
`spectty-core` MUST define a `SessionRegistry` that owns `Session` aggregates and exposes
`create` (mint a new `Session` and return it / its id), look-up by `SessionId`, and `close`
(remove / mark the session closed). It MUST use the `&self` interior-mutability convention
(matching the M0 `PersistencePort` / `in_memory` adapter pattern) so it is shareable as
`tauri::State`. The registry MUST mint `SessionId`s, migrating the M1 `next_pty_id` counter so
that `SessionId == PtyId` (Lock 6) — the Core `SessionRegistry` and the `src-tauri`
`PtyRegistry` stay in lockstep with no cross-mapping table.

#### Scenario: create then look up returns the same session
- **Given** an empty `SessionRegistry`
- **When** `create` is called with a workspace + `AgentSpec`, then the returned id is looked up
- **Then** the looked-up `Session` MUST be the one just created, with matching workspace and
  agent, asserted as a pure unit (no PTY, no Tauri)

#### Scenario: close removes the session from lookup
- **Given** a `SessionRegistry` holding one created session
- **When** `close` is called with that session's id
- **Then** a subsequent look-up MUST report the session as closed / absent

#### Scenario: SessionRegistry mints ids via &self interior mutability
- **Given** a `SessionRegistry` shared behind a shared reference
- **When** `create` is invoked twice through `&self`
- **Then** two DISTINCT `SessionId`s MUST be minted (monotonic, migrating `next_pty_id`'s
  role), asserted without a mutable borrow of the registry

### Requirement: SessionRegistry stays distinct from the src-tauri PtyRegistry  [unit]
The Core `SessionRegistry` MUST own ONLY the `Session` aggregate (domain state). OS-level
handles (writer, child handle, read-thread stop handle) MUST remain in the M1 `src-tauri`
`PtyRegistry`. The Core `SessionRegistry` MUST NOT import `tauri`, `portable-pty`, or hold OS
handles. `SessionId == PtyId` keys both registries in lockstep.

#### Scenario: The Core registry holds no OS handle
- **Given** the `spectty-core` `SessionRegistry`
- **When** its stored entry shape is inspected
- **Then** it MUST hold only `Session` domain state AND MUST NOT hold a PTY writer, a child
  handle, or any `portable-pty`/`tauri` type

---

## Capability: provisioning-port

The `ProvisioningPort` (Core trait) + `ProvisionerAdapter` implement M2 Layer-1 ONLY: register
the `spectty_*` MCP tools in the agent's config on session create, and retract them on close.
A registered-but-stubbed `spectty-mcp` server backs the registration.

### Requirement: ProvisioningPort is a Core trait separate from AgentRunner  [unit]
`spectty-core` MUST define a `ProvisioningPort` trait, SEPARATE from `AgentRunner` (Lock 1),
exposing at minimum `inject(scope) -> Result<...>` (register the managed `spectty_*` config)
and `retract(scope) -> Result<...>` (remove the managed config). The trait MUST be
agent-agnostic: it MUST NOT name an agent or a config file path (those live in the adapter).
A Generic-tier session simply is NOT wired to a provisioner (the Generic adapter needs no
injection).

#### Scenario: ProvisioningPort exposes inject and retract and is its own port
- **Given** the `spectty-core` `ProvisioningPort` trait after M2
- **When** its methods are inspected
- **Then** `inject` and `retract` MUST be present AND `ProvisioningPort` MUST be a DISTINCT
  trait from `AgentRunner` (no provisioning method on `AgentRunner`)

### Requirement: The JSON managed-namespace editor owns only spectty_* keys  [unit]
The Provisioner adapter MUST implement a PURE `String -> String` JSON editor (in
`crates/adapters`) that registers `spectty_*` entries under the agent config's `mcpServers`
object and OWNS ONLY keys in the `spectty_*` namespace. It MUST NOT use text managed-markers
(which corrupt structured JSON) and MUST NOT shell out to `claude mcp add` (not atomic, not
unit-testable) — Lock 5. Foreign keys (user keys, `gentle-ai` keys, other `mcpServers`
entries) MUST round-trip UNTOUCHED (R7). `retract` MUST remove only the `spectty_*` keys,
leaving foreign keys intact.

#### Scenario: inject adds spectty_* keys and leaves foreign keys untouched (round-trip)
- **Given** a JSON config string containing a user `mcpServers` entry and a `gentle-ai` entry
- **When** the pure editor injects the `spectty_*` registration
- **Then** the output MUST contain the new `spectty_*` keys AND the user and `gentle-ai`
  entries MUST be byte-for-byte / structurally preserved, asserted as a pure `String -> String`
  unit with no file-IO

#### Scenario: retract removes only spectty_* keys
- **Given** a JSON config string that already contains `spectty_*` keys plus foreign keys
- **When** the pure editor retracts the `spectty_*` namespace
- **Then** all `spectty_*` keys MUST be gone AND every foreign key MUST remain, asserted as a
  pure unit

#### Scenario: Editing malformed or missing mcpServers is handled, not corrupting
- **Given** a config string with no `mcpServers` object (or an empty document)
- **When** the editor injects the `spectty_*` registration
- **Then** it MUST create a valid `mcpServers` object containing the `spectty_*` keys AND
  produce valid JSON (never a partially-written or corrupt document)

### Requirement: Config writes are atomic with a backup  [unit]
The Provisioner MUST write config changes behind an ATOMIC-write file-IO seam: write to a
temp file → fsync → atomic rename (Lock 5). Before the FIRST write to any file, it MUST copy
the existing file to `<file>.spectty.bak` (backup-before-write). The file-IO MUST sit behind
an injectable seam so the atomic-write + backup behavior is testable with a fake filesystem
(no real disk required for the unit gate), while a real run uses real file-IO.

#### Scenario: First write creates a .spectty.bak backup
- **Given** an existing config file and the atomic-write seam backed by a fake filesystem
- **When** the Provisioner performs its first write to that file
- **Then** a `<file>.spectty.bak` copy of the ORIGINAL contents MUST exist AND the final write
  MUST land via temp-file-then-rename (asserted on the fake filesystem operations)

#### Scenario: A crash mid-write never leaves a partial config  [unit]
- **Given** the atomic-write seam
- **When** a write is interrupted before the rename completes (simulated on the fake)
- **Then** the original config file MUST remain intact (the temp file, not the live file, held
  the partial write), so the agent's startup is never broken by a half-written config

### Requirement: Scope resolves to GLOBAL by default, PROJECT when git-tracked  [unit]
The Provisioner MUST resolve injection SCOPE via a single INJECTED `is_git_tracked(path) ->
bool` predicate (NOT a full `GitPort` — that is M4). It MUST default to GLOBAL — writing the
`spectty_*` `mcpServers` entries into `~/.claude.json` (top-level `mcpServers`) — and MUST
resolve to PROJECT — writing into `.mcp.json` at the repo root — WHEN the agent's config file
is git-tracked (the predicate returns true). When the predicate is unavailable or false, it
MUST default to GLOBAL. The resolver MUST be a pure function over the injected predicate so it
is testable with a fake predicate.

#### Scenario: Git-tracked config resolves to PROJECT scope
- **Given** the scope resolver with a fake `is_git_tracked` predicate returning true
- **When** scope is resolved for an agent config path
- **Then** it MUST resolve to PROJECT scope targeting `.mcp.json` at the repo root, asserted
  with the fake predicate (no real git)

#### Scenario: Untracked or unknown config resolves to GLOBAL scope
- **Given** the scope resolver with a fake `is_git_tracked` predicate returning false (or
  unavailable)
- **When** scope is resolved
- **Then** it MUST default to GLOBAL scope targeting `~/.claude.json` top-level `mcpServers`

### Requirement: The spectty-mcp server ships registered-but-stubbed advertising five tool schemas  [unit]
M2 MUST ship a real `spectty-mcp` binary that EXISTS, STARTS over stdio, and ADVERTISES the
five Spectty Agent Protocol tool schemas — `spectty_spec`, `spectty_diff`, `spectty_approval`,
`spectty_status`, and `spectty_cost` (per agent-protocol.md "Tool suite"; the roadmap M2 text
names the first three as examples, the canonical suite is five). The registered config entry
MUST point at this binary (a missing binary breaks Claude Code startup — Lock 4). The tool
EFFECTS (persist spec, trigger diff, resolve approval, push status, ingest cost) are M3: in
M2 the tools MUST accept calls and return a benign acknowledgement WITHOUT side effects (R4).
The ADVERTISED SCHEMA is the forward-compatible contract — M3 swaps in effects WITHOUT
changing the registered schema.

#### Scenario: The stub server advertises exactly the five tool schemas
- **Given** the `spectty-mcp` stub server started over stdio
- **When** its advertised tool list is requested
- **Then** it MUST advertise `spectty_spec`, `spectty_diff`, `spectty_approval`,
  `spectty_status`, and `spectty_cost` with their declared input schemas, asserted against the
  agent-protocol.md schemas

#### Scenario: A stub tool call returns an acknowledgement with no side effect
- **Given** the stub server
- **When** any `spectty_*` tool is invoked
- **Then** it MUST return a benign acknowledgement AND MUST NOT persist a spec, trigger a diff,
  resolve an approval, or mutate any session state (effects are M3)

#### Scenario: The injected config entry points at the existing stub binary  [manual]
- **Given** a spawned Claude Code session with the Provisioner having injected the managed
  `spectty_*` registration
- **When** the Claude Code config is inspected and the agent starts
- **Then** the managed section with the MCP tools MUST be present (roadmap exit criterion 2)
  AND Claude Code MUST start successfully (the registered binary exists and speaks stdio MCP)

### Requirement: Provisioning is injected on create and retracted on close  [unit]
On session CREATE for a Cooperative-tier agent, `src-tauri` MUST call the Provisioner's
`inject` for the resolved scope BEFORE / around the PTY spawn. On session CLOSE, `src-tauri`
MUST kill the PTY (M1 path) AND call the Provisioner's `retract` to remove the managed
`spectty_*` keys. The wiring MUST be testable against a fake Provisioner (asserting inject/
retract are invoked at the right lifecycle points) without a real config file.

#### Scenario: Close retracts the managed section after killing the PTY
- **Given** a created Cooperative session wired to a fake Provisioner
- **When** the session is closed
- **Then** the PTY MUST be killed (M1 path) AND `retract` MUST be invoked for the session's
  resolved scope, asserted against the fake without a real config file

---

## Capability: agent-session-ui

The UI gains a spawn flow (pick agent + workspace), an `AgentStatus` indicator in the Pane
header reacting to a `status_changed` event, and a named session title. The `src-tauri` bridge
gains the spawn/close commands and the `status_changed` event.

### Requirement: The bridge exposes spawn-session and close-session commands and a status event  [unit]
`src-tauri` MUST expose, registered in `generate_handler!`, a command to SPAWN a session
(taking the chosen agent + workspace directory, resolving the runner, injecting provisioning,
and spawning the PTY) and a command to CLOSE a session (killing the PTY + retracting
provisioning). It MUST emit a Tauri v2 `status_changed { session_id, status, quick_actions }`
event (via `Emitter`, matching the M0/M1 v2 emit convention) whenever a session's `AgentStatus`
changes after running an observation through the pure `transition`. Commands MUST take owned
types and return `Result<_, _>` per the M0 convention.

#### Scenario: spawn and close commands are registered in the handler
- **Given** the `src-tauri` `generate_handler!` registration after M2
- **When** the registered command set is inspected
- **Then** a spawn-session command AND a close-session command MUST each be present (an
  unregistered command silently fails at invoke, so registration is the guard)

#### Scenario: status_changed is emitted only on an actual status change
- **Given** a session whose detector yields observations run through `transition`
- **When** an observation does NOT change the status (e.g. an ignored illegal jump, or a
  repeat of the current status)
- **Then** NO `status_changed` event MUST be emitted; the event MUST fire only when
  `transition` returns a DIFFERENT status, asserted on the wiring with a fake emitter

#### Scenario: status_changed carries session_id, status, and quick_actions
- **Given** a status change for a session
- **When** `status_changed` is emitted
- **Then** its payload MUST carry the `session_id`, the new `status`, and a `quick_actions`
  field (empty in M2 for scraped statuses, since structured quick-actions are M3)

### Requirement: The UI spawns a session and shows status + title in the Pane header  [unit]
The UI MUST provide a spawn flow letting the user pick an AGENT and a WORKSPACE DIRECTORY,
then invoke the spawn-session command. It MUST render an `AgentStatus` INDICATOR in the Pane
header that reacts to the `status_changed` event, and display a named SESSION TITLE. The
wiring MUST follow the M1 `useTerminal`/`usePingPong` hook pattern (a `useSession`-style hook),
with `invoke`, the event listener, and any Channel mocked in vitest. React 19 named imports
MUST be used; manual `useMemo`/`useCallback` MUST NOT be added.

#### Scenario: Selecting an agent and workspace invokes the spawn command
- **Given** the spawn flow with `invoke` mocked
- **When** the user picks an agent and a workspace directory and confirms
- **Then** the spawn-session command MUST be invoked carrying the chosen agent and workspace,
  asserted in vitest with no real backend

#### Scenario: The Pane header badge updates on a status_changed event
- **Given** the mounted session UI with the Tauri event listener mocked
- **When** a `status_changed` event delivers a new `AgentStatus` for the session
- **Then** the Pane-header status indicator MUST update to reflect the new status AND the
  session title MUST be displayed, asserted in vitest

---

## Capability: hexagonal-core (delta — quarantine evolves, agent names stay out)

This is a guard delta on the archived `hexagonal-core` baseline. M2 INTENTIONALLY adds
domain types to Core that the M1 guard deferred — `AgentRunner`, `ProvisioningPort`,
`OutputSignal`, `AgentSpec`, the `transition` function, `Session` behavior, and
`SessionRegistry` — while keeping the Core dependency set and the agent-agnostic invariant.

### Requirement: M2 grows the Core domain WITHOUT new dependencies and WITHOUT agent names  [ci]
M2 MUST add `AgentRunner`, `ProvisioningPort`, `OutputSignal`, `AgentSpec`/`AgentTier`/
`AgentDescriptor`, `LaunchSpec`, the pure `transition` function, the grown `Session`, and
`SessionRegistry` to `spectty-core` — and MUST do so with ZERO new dependencies: the Core
MUST remain `serde` + `thiserror` only (no `tokio`, no `tauri`, no `portable-pty`, no agent/
tool crate, no time crate — the `ClockPort`-style time seam is a Core TRAIT whose concrete
clock lives outside Core). The Core MUST contain NO agent-name literal and NO config-format
or ANSI/regex knowledge. The core-scoped `cargo-deny` gate MUST stay green. This requirement
SUPERSEDES, for M2, the M1 guard clause that forbade defining an `OutputSignal` type in Core
(that clause was M1-scoped; M2 deliberately introduces `OutputSignal` as a Core port type).

#### Scenario: Core manifest still lists only serde + thiserror after M2
- **Given** the `spectty-core` `Cargo.toml` after M2
- **When** its dependency list is inspected
- **Then** it MUST remain limited to `serde` + `thiserror`, with NO `tokio`, `tauri`,
  `portable-pty`, time crate, or agent/tool crate added

#### Scenario: No agent name appears anywhere in the Core
- **Given** the `spectty-core` source after M2
- **When** it is scanned for agent-name literals
- **Then** there MUST be no `"claude"`, no `"bash"`, and no `if agent == …` branch anywhere
  in Core — all agent-specific logic MUST live in `crates/adapters` / `src-tauri`

#### Scenario: core-scoped cargo-deny stays green and clippy is clean  [ci]
- **Given** the M2 changes applied (runners + provisioner + producer in adapters/src-tauri,
  Core grown but quarantined)
- **When** the core-scoped `cargo-deny` boundary gate and `clippy -D warnings` run in CI
- **Then** `cargo-deny` MUST exit 0 with no forbidden-dependency findings AND clippy MUST
  report no warnings AND `cargo build` MUST succeed

---

## Roadmap exit criteria (acceptance gate)

These five checks are the verbatim roadmap M2 exit criteria and the `sdd-verify` pass/fail
gate, on top of the strict-TDD unit gate (`cargo test --workspace`; `pnpm -C ui test`).
Checks (1)–(4) are real-Claude-Code manual-acceptance checks (the `AwaitingInput`/
permission-prompt patterns are empirical, R5); (5) is the Generic-tier baseline.

### Requirement: M2 satisfies all five roadmap exit criteria  [manual]

#### Scenario: (1) Spawn a Claude Code session on a git repo; it reaches Idle
- **Given** the running app and a local git repository
- **When** the user spawns a Claude Code session on that repo
- **Then** the agent MUST launch in the PTY AND its status MUST reach `Idle`

#### Scenario: (2) The managed section with MCP tools is present in the Claude Code config
- **Given** a spawned Claude Code session
- **When** the user inspects the Claude Code config (`~/.claude.json` or `.mcp.json` per scope)
- **Then** Spectty's managed `spectty_*` `mcpServers` registration MUST be present and
  inspectable AND MUST coexist with any pre-existing user / gentle-ai entries

#### Scenario: (3) Task → Running → AwaitingInput on a permission prompt → Running after input
- **Given** an Idle Claude Code session
- **When** the user gives it a task, it hits a permission prompt, and the user answers
- **Then** status MUST transition `Idle → Running`, then `Running → AwaitingInput` at the
  permission prompt, then `AwaitingInput → Running` after input is given

#### Scenario: (4) Close the session; PTY terminates and the managed section is removed
- **Given** a running Claude Code session with an injected managed section
- **When** the user closes the session
- **Then** the PTY process MUST terminate AND the managed `spectty_*` section MUST be removed
  from the agent config (foreign entries left intact)

#### Scenario: (5) Generic bash reaches Idle, then idle-timeout transitions to Completed
- **Given** the running app
- **When** the user spawns `bash` via the Generic adapter and leaves it inactive
- **Then** status MUST reach `Idle` AND, after the configurable inactivity window, MUST
  transition to `Completed`

---

## Cross-platform stance

### Requirement: macOS MUST pass; Windows agent spawn is best-effort  [manual]
M2 acceptance MUST pass on macOS (inheriting M1's stance). The real-PTY agent-spawn
integration test MUST be `#[cfg(unix)]`. Windows agent spawn SHOULD work best-effort but MUST
NOT be a CI-gated requirement for M2; Windows regressions MUST NOT block M2 acceptance.

#### Scenario: macOS acceptance is gating, Windows is best-effort
- **Given** the M2 acceptance run
- **When** acceptance is evaluated per platform
- **Then** all five exit criteria MUST pass on macOS AND a Windows agent-spawn failure MUST
  NOT block M2 (best-effort, ungated)

---

## Out of scope (NO requirements in M2 — M3/M4/M5)

The following carry NO M2 requirements and MUST NOT be built in M2 (explicit non-goals):

- **Layer-2 (`additionalContext` hook) and Layer-3 (SKILL.md/rules) injection** — M3 (they
  push the live Spec, which does not exist until M3). M2 = Layer-1 MCP registration + teardown
  only.
- **The live Spec pane and `Spec` aggregate behavior** (plan-approval gate, structured task
  progress, `spec_updated` polling) — M3.
- **`spectty_*` tool EFFECTS** (spec persistence, diff trigger, approval resolution/unblocking,
  cost ingestion) — M3. M2 ships the stub server (advertised schemas only) + Layer-1
  registration.
- **VibeLens / `DiffExplainerPort` / `spectty_diff` wiring** — M3.
- **Cost-parsing depth** (real `parse_cost` regexes, `CostMetrics` accumulation,
  `cost_updated`, `spectty_cost` ingestion) — M3. M2 = skeleton method + struct only.
- **`quick_actions` real prompt-answering** (sending `y\n`) and the structured
  `spectty_approval` `AwaitingInput` path — M3. M2 `AwaitingInput` is PTY-scraped only.
- **Worktrees / `GitPort` / Checkpoints / branch isolation** — M4. M2 uses only the injected
  `is_git_tracked` predicate for scope detection, not a real `GitPort`.
- **Multi-session UI** (tabs, panes, switcher, split tree) — M4. M2 grows session chrome on
  the single Pane only.
- **Per-agent format adapters beyond Claude Code JSON** (Cursor `.cursor/mcp.json`, Codex
  TOML, Aider YAML) — fast-follow / post-MVP. M2 = Claude Code JSON namespace editor only.
- **Provisioner refresh hook + SHA fingerprint cache** — Layer-2 dynamics, M3.
- **Startup reconciliation of orphaned `spectty_*` keys** (R8) — M2 relies on `.spectty.bak`
  as the manual escape hatch; automatic boot-time orphan retraction is deferred unless
  `sdd-design` elevates it.
