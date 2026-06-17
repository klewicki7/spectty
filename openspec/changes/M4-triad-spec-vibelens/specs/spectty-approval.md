# Capability: spectty-approval

> M4 delta spec. NEW capability established by change `M4-triad-spec-vibelens`.
> RFC 2119 keywords are normative. Full prose + tags live in the change-level `spec.md`.

`spectty_approval` is the ONLY genuinely BLOCKING tool. M4 implements the pending-future / resolver
seam (Decision 1, carve-out B): a `tools/call` registers a pending request keyed `(session_id,
action_id)`, surfaces it as `AwaitingInput` + `quick_actions` (reusing the M2 status path), and
resolves when the UI sends `approve_prompt`. Exact plumbing pinned in design.

The UI resolves whatever pending `(session_id, action_id)` actually exists for the session (read via
`get_approval`) — the `action_id` is the agent's free-form id, NOT a fixed constant. There is no
hardcoded `action_id` contract between the UI and the agent.

## ADDED Requirements

### Requirement: spectty_approval registers a pending request and surfaces it as AwaitingInput
A `tools/call` for `spectty_approval` (advertised schema `{session_id, action_id, description,
risk_level, options[]}` UNCHANGED) MUST register a pending request keyed `(session_id, action_id)`
and surface it as `AwaitingInput` carrying `quick_actions` from `options[]`. The request MUST remain
pending until resolved; duplicate `(session_id, action_id)` registrations MUST be idempotent.

#### Scenario: An approval call registers one pending request
- **Given** the resolver seam with no pending requests
- **When** a `spectty_approval { session_id, action_id, options }` is registered
- **Then** EXACTLY ONE pending request keyed `(session_id, action_id)` MUST exist AND surface as
  `AwaitingInput` with `quick_actions` from `options`

#### Scenario: Duplicate action_id registration is idempotent
- **Given** a pending request for `(42, "edit-1")`
- **When** the same `(42, "edit-1")` is registered again
- **Then** there MUST still be exactly one pending request for that key

### Requirement: approve_prompt resolves the pending request and unblocks the agent
When the UI sends `approve_prompt(session_id, action_id, decision)`, the resolver MUST resolve the
matching pending request, remove it from pending, and make the resolution observable to the blocked
caller. An `approve_prompt` for an unknown key MUST be a no-op.

#### Scenario: approve_prompt resolves a pending approval
- **Given** a pending request for `(42, "edit-1")`
- **When** `approve_prompt(42, "edit-1", Approved)` is received
- **Then** the request MUST resolve as `Approved`, MUST be removed from pending, AND the resolution
  MUST be retrievable by the blocked caller

#### Scenario: approve_prompt for an unknown key is a no-op
- **Given** no pending request for `(42, "ghost")`
- **When** `approve_prompt(42, "ghost", Approved)` is received
- **Then** it MUST be ignored without error and MUST NOT create a pending entry
