# Capability: tauri-bridge

> Living baseline spec. Established by change `M0-scaffold` (archived 2026-06-08).
> Extended by change `M4-triad-spec-vibelens` (archived 2026-06-17).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The Bridge proves bidirectional communication between the Rust shell and the React UI via
commands and events. M0 establishes the foundation; M4 adds spec and diff pipelines.

## Requirement: ping command emits an observable pong event
The `src-tauri` bridge MUST expose a `ping` Tauri command (Tauri v2). Invoking it MUST
result in a `pong` event emitted via the v2 `AppHandle::emit` API, observable in the
running app.

### Scenario: ping → pong is visible in the running app
- **Given** the app running via `pnpm tauri dev`
- **When** the UI invokes the `ping` command
- **Then** a `pong` event MUST be emitted by the bridge AND the UI listener MUST observe it and log it to the web console

### Scenario: Bridge uses Tauri v2 emit API (guard against v1 drift)
- **Given** the bridge implementation of `pong`
- **When** the emit call is inspected
- **Then** it MUST use the Tauri v2 `AppHandle::emit` API (via the `Emitter` trait) and MUST NOT use removed Tauri v1 emit signatures

## Requirement: get_spec and get_diff_explanation commands are registered  [unit]  (M4 ADDED)

`src-tauri` MUST register `get_spec(session_id)` → current `SpecContract` (or absent) and `get_diff_explanation(session_id)` → current `DiffExplanation` (or absent), each `Result<_, _>` over owned types. Existing M0/M2 commands MUST remain registered.

### Scenario: Both new commands are registered alongside existing ones
- **Given** the `generate_handler!` registration after M4
- **When** the command set is inspected
- **Then** `get_spec` AND `get_diff_explanation` MUST be present AND `spawn_session` / `close_session` / `list_sessions` / `get_session` MUST still be present

## Requirement: spec_updated and diff_updated are emitted via the Tauri v2 Emitter  [unit]  (M4 ADDED)

`src-tauri` MUST emit `spec_updated { session_id, spec }` on a detected spec change and `diff_updated { session_id, explanation }` on a new explanation, both via the Tauri v2 `Emitter` (NOT v1). Each MUST fire only on an ACTUAL change.

### Scenario: spec_updated fires only on an actual spec change
- **Given** the bridge wired to the poll seam
- **When** the seam reports no change
- **Then** NO `spec_updated` MUST be emitted; it MUST fire only when the payload actually changes

### Scenario: diff_updated carries session_id and explanation
- **Given** the diff pipeline producing a new explanation
- **When** `diff_updated` is emitted
- **Then** its payload MUST carry `session_id` and the `explanation`, via the v2 `Emitter`
