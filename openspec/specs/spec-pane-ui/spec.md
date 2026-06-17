# Capability: spec-pane-ui

> Living baseline spec. Established by change `M4-triad-spec-vibelens` (archived 2026-06-17).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The UI gains a Spec pane: seeds intent, renders the living checklist reacting to `spec_updated` (no manual refresh), and presents the plan-approval gate (Approve/Edit/Reject) calling `approve_prompt`. Follows the M1/M2 hook pattern (`useSession`-style, React 19 named imports, no manual `useMemo`/`useCallback`); `invoke`/listeners mocked in vitest. Generic tier shows a coarse scraped badge.

## Requirement: The Spec pane renders the live checklist from spec_updated

The Spec pane MUST render the `SpecContract` tasks as a checklist and update task states LIVE on each `spec_updated` event WITHOUT a manual refresh. Each task MUST show its `TaskState`.

### Scenario: A spec_updated event updates the checklist without refresh
- **Given** the mounted Spec pane with the listener mocked
- **When** a `spec_updated` event delivers a contract with a task moved to `done`
- **Then** the checklist MUST reflect that task as `done` with no manual refresh

### Scenario: Generic tier shows a coarse scraped badge
- **Given** a session whose progress source is PTY-scraping only (no structured spec)
- **When** the Spec pane renders
- **Then** it MUST show a coarse status badge rather than a precise checklist, degrading gracefully

## Requirement: The plan-approval gate presents Approve/Edit/Reject and calls approve_prompt

When `approval == Pending`, the Spec pane MUST present Approve, Edit, and Reject actions; selecting one MUST invoke `approve_prompt(session_id, action_id, decision)`. The gate MUST NOT render once approval is resolved.

### Scenario: Approving the plan invokes approve_prompt
- **Given** the Spec pane with a Pending approval and `invoke` mocked
- **When** the user clicks Approve
- **Then** `approve_prompt` MUST be invoked with the session id, action id, and an `Approved` decision

### Scenario: The gate hides once approval is resolved
- **Given** the Spec pane after a `spec_updated` event with `approval = Approved`
- **When** the pane re-renders
- **Then** the plan-approval gate MUST NOT be shown
