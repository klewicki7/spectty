# Capability: plan-approval-gate

> Living baseline spec. Established by change `M4-triad-spec-vibelens` (archived 2026-06-17).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The plan-approval gate is a PURE Core business rule (ADR-0007): the agent MUST NOT begin code edits until the human approves. Enforceable in unit tests with no I/O.

## Requirement: The gate blocks code edits until the plan is Approved

`spectty-core` MUST expose a pure predicate (e.g. `SpecContract::may_edit()`) returning true ONLY when `approval == Approved`. `Pending`/`Rejected`/`Adjusted` MUST return false. A dev override MUST be representable, MUST NOT be the default, and MUST be distinguishable from a normal `Approved`.

### Scenario: Pending plan does not permit edits
- **Given** a `SpecContract` with `approval = Pending`
- **When** the gate predicate is evaluated
- **Then** it MUST return false (edits gated)

### Scenario: Approved plan permits edits
- **Given** a `SpecContract` with `approval = Approved`
- **When** the gate predicate is evaluated
- **Then** it MUST return true

### Scenario: Rejected plan does not permit edits
- **Given** a `SpecContract` with `approval = Rejected`
- **When** the gate predicate is evaluated
- **Then** it MUST return false

### Scenario: Dev override permits edits without normal approval
- **Given** a `SpecContract` with `approval = Pending` and the dev override engaged
- **When** the gate predicate is evaluated
- **Then** it MUST return true AND the override MUST be distinguishable from a normal `Approved`
