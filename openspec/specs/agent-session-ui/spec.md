# Capability: agent-session-ui

> Living baseline spec. Established by change `M2-spawn-agent-provisioner` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The UI gains a spawn flow (pick agent + workspace), an `AgentStatus` indicator in the Pane
header reacting to a `status_changed` event, and a named session title. The `src-tauri` bridge
gains the spawn/close commands and the `status_changed` event.

## Requirement: The bridge exposes spawn-session and close-session commands and a status event
`src-tauri` MUST register a spawn-session command (agent + workspace → resolve runner, inject
provisioning, spawn PTY) and a close-session command (kill PTY + retract provisioning), and
MUST emit a Tauri v2 `status_changed { session_id, status, quick_actions }` event via
`Emitter` whenever a session's status changes after running an observation through the pure
`transition`. Commands take owned types and return `Result<_, _>`.

### Scenario: spawn and close commands are registered in the handler
- **Given** the `src-tauri` `generate_handler!` registration after M2
- **When** the registered command set is inspected
- **Then** a spawn-session command AND a close-session command MUST each be present

### Scenario: status_changed is emitted only on an actual status change
- **Given** a session whose detector yields observations run through `transition`
- **When** an observation does NOT change the status (ignored illegal jump or a repeat)
- **Then** NO `status_changed` event MUST be emitted; the event MUST fire only when
  `transition` returns a DIFFERENT status

### Scenario: status_changed carries session_id, status, and quick_actions
- **Given** a status change for a session
- **When** `status_changed` is emitted
- **Then** its payload MUST carry `session_id`, the new `status`, and a `quick_actions` field
  (empty in M2 for scraped statuses)

## Requirement: The UI spawns a session and shows status + title in the Pane header
The UI MUST provide a spawn flow to pick an agent and a workspace directory and invoke the
spawn-session command, render an `AgentStatus` indicator in the Pane header reacting to
`status_changed`, and display a session title — following the M1 `useTerminal`/`usePingPong`
hook pattern (a `useSession`-style hook) with `invoke`, the event listener, and any Channel
mocked in vitest. React 19 named imports MUST be used; manual `useMemo`/`useCallback` MUST NOT
be added.

### Scenario: Selecting an agent and workspace invokes the spawn command
- **Given** the spawn flow with `invoke` mocked
- **When** the user picks an agent and a workspace directory and confirms
- **Then** the spawn-session command MUST be invoked carrying the chosen agent and workspace

### Scenario: The Pane header badge updates on a status_changed event
- **Given** the mounted session UI with the Tauri event listener mocked
- **When** a `status_changed` event delivers a new `AgentStatus` for the session
- **Then** the Pane-header status indicator MUST update AND the session title MUST be displayed
