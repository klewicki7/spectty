# Capability: tauri-bridge

> M4 delta spec. MODIFIED capability (extends `tauri-bridge` baseline from M0).
> Change `M4-triad-spec-vibelens`. RFC 2119 keywords are normative. Full prose + tags live in
> the change-level `spec.md`.

The bridge gains `get_spec` / `get_diff_explanation` commands and `spec_updated` / `diff_updated`
events (Tauri v2 `Emitter`), plus per-session pipeline wiring in `spawn_session` /
`session_runtime.rs`. Existing commands/events are preserved.

## ADDED Requirements

### Requirement: get_spec and get_diff_explanation commands are registered
`src-tauri` MUST register `get_spec(session_id)` → current `SpecContract` (or absent) and
`get_diff_explanation(session_id)` → current `DiffExplanation` (or absent), each `Result<_, _>` over
owned types. Existing M0/M2 commands MUST remain registered.

#### Scenario: Both new commands are registered alongside existing ones
- **Given** the `generate_handler!` registration after M4
- **When** the command set is inspected
- **Then** `get_spec` AND `get_diff_explanation` MUST be present AND `spawn_session` / `close_session`
  / `list_sessions` / `get_session` MUST still be present

### Requirement: spec_updated and diff_updated are emitted via the Tauri v2 Emitter
`src-tauri` MUST emit `spec_updated { session_id, spec }` on a detected spec change and
`diff_updated { session_id, explanation }` on a new explanation, both via the Tauri v2 `Emitter`
(NOT v1). Each MUST fire only on an ACTUAL change.

#### Scenario: spec_updated fires only on an actual spec change
- **Given** the bridge wired to the poll seam
- **When** the seam reports no change
- **Then** NO `spec_updated` MUST be emitted; it MUST fire only when the payload actually changes

#### Scenario: diff_updated carries session_id and explanation
- **Given** the diff pipeline producing a new explanation
- **When** `diff_updated` is emitted
- **Then** its payload MUST carry `session_id` and the `explanation`, via the v2 `Emitter`
