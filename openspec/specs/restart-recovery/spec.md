# Capability: restart-recovery

> Living baseline spec. Established by change `M4-triad-spec-vibelens` (archived 2026-06-17).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

Restarting Spectty mid-session MUST restore the spec + progress from engram because engram IS the store.

## Requirement: Spec and progress are restored from engram on restart

On session re-attach after restart, Spectty MUST read `spectty/{session_id}/spec` (and `/progress`) from engram and reconstruct the `SpecContract` so the Spec pane shows the prior intent, plan, task states, and approval. If engram is unreachable at restart, the pane MUST degrade gracefully (empty/last-known) without crashing.

### Scenario: Restored spec reconstructs the contract
- **Given** a fake reader holding a `spectty/42/spec` payload from a prior run
- **When** the session is re-attached after restart
- **Then** the reconstructed `SpecContract` MUST equal the persisted one (intent, tasks, states, approval)

### Scenario: Restart with engram down degrades gracefully
- **Given** a restart where the engram reader returns a connection error
- **When** re-attach runs
- **Then** the Spec pane MUST show an empty/last-known state without crashing

### Scenario: Manual restart mid-session restores spec + progress
- **Given** an active session with an approved plan and partial task progress, then Spectty is restarted
- **When** the session is re-opened
- **Then** the Spec pane MUST show the prior intent, plan, task states, and approval restored from engram
