# Capability: spec-contract

> Living baseline spec. Established by change `M4-triad-spec-vibelens` (archived 2026-06-17).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

`SpecContract`, `TaskState`, `ApprovalState` are PURE Core entities (ADR-0007): serde + thiserror only, no I/O, no time, no agent name. Serialization to engram is the adapter's job.

## Requirement: SpecContract is a pure Core aggregate with the living-contract fields

`spectty-core` MUST define `SpecContract { intent, proposal, tasks[]:{id,title,status:TaskState,notes?}, progress, approval:ApprovalState, steering_notes }`, serde-round-trippable, with no I/O/time/agent name.

### Scenario: SpecContract round-trips through serde
- **Given** a populated `SpecContract`
- **When** it is serialized then deserialized
- **Then** the round-tripped value MUST equal the original

### Scenario: SpecContract is pure
- **Given** the `SpecContract` module
- **When** it is inspected
- **Then** it MUST reference no filesystem/network/time API and no hard-coded agent name

## Requirement: TaskState enforces one-directional legal transitions

`TaskState` MUST define `pending`, `in_progress`, `done`, `skipped`. A pure transition rule MUST allow only `pending → {in_progress, skipped}`, `in_progress → {done, skipped}`; `done` is terminal; illegal transitions leave state unchanged.

### Scenario: pending advances to in_progress
- **Given** a task in `pending`
- **When** an `in_progress` transition is applied
- **Then** the task MUST become `in_progress`

### Scenario: done is terminal and rejects backward transition
- **Given** a task in `done`
- **When** an `in_progress` transition is applied
- **Then** the task MUST remain `done` (illegal backward transition ignored, not an error)

### Scenario: pending may be skipped
- **Given** a task in `pending`
- **When** a `skipped` transition is applied
- **Then** the task MUST become `skipped`

## Requirement: ApprovalState models the plan-approval lifecycle

`ApprovalState` MUST define `Pending`, `Approved`, `Rejected`, `Adjusted`. A freshly submitted plan MUST start `Pending`; resolution transitions MUST be pure values; `Approved` is the only state that satisfies the gate.

### Scenario: A submitted plan starts Pending
- **Given** a `SpecContract` from a freshly submitted plan
- **When** its `approval` is inspected
- **Then** it MUST be `ApprovalState::Pending`

### Scenario: Approval resolves to Approved
- **Given** an `ApprovalState::Pending`
- **When** an approve resolution is applied
- **Then** it MUST become `ApprovalState::Approved`
