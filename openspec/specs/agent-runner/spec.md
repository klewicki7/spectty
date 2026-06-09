# Capability: agent-runner

> Living baseline spec. Established by change `M2-spawn-agent-provisioner` (archived 2026-06-08).
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
