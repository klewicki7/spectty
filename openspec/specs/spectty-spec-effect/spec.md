# Capability: spectty-spec-effect

> Living baseline spec. Established by change `M4-triad-spec-vibelens` (archived 2026-06-17).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

`spectty_spec` gains a real EFFECT behind the UNCHANGED advertised schema. The MCP binary upserts to `spectty/{session_id}/spec` via its thin engram HTTP client (serde only — never core/tauri) and returns immediately; the poll loop emits `spec_updated`.

## Requirement: The MCP tool schema is FROZEN; only effects change

`crates/spectty-mcp/src/main.rs` MUST keep the advertised `tools/list` schema for all five tools (canonical order, parameter shapes) byte-for-byte as shipped. M4 changes ONLY `tools/call` effect bodies. The binary MUST depend on `serde`/`serde_json` (MAY add a thin engram HTTP client) and MUST NOT depend on `crates/core` or `tauri`.

### Scenario: tools/list schema is unchanged
- **Given** the `tools/list` response after M4
- **When** compared to the frozen baseline schema
- **Then** advertised tool names, order, and parameter schemas MUST be identical

### Scenario: spectty-mcp depends on serde only (no core/tauri)
- **Given** `crates/spectty-mcp` dependencies after M4
- **When** inspected
- **Then** they MUST NOT include `spectty-core` or `tauri`

## Requirement: spectty_spec upserts the contract to engram and surfaces live

A `tools/call` for `spectty_spec` MUST parse `{session_id, spec}` and upsert to `spectty/{session_id}/spec`, returning immediately. The poll loop MUST, on detecting the change, emit `spec_updated { session_id, spec }`. A malformed payload MUST be rejected without crashing.

### Scenario: spectty_spec upserts under the canonical key
- **Given** a `spectty_spec` call for `session_id = 42` over a fake engram client
- **When** the effect runs
- **Then** an upsert MUST target `spectty/42/spec` with the serialized contract AND the call MUST return promptly

### Scenario: A spec change emits spec_updated once
- **Given** the poll loop detecting a new `spectty/42/spec` payload
- **When** the change is observed
- **Then** EXACTLY ONE `spec_updated { session_id: 42, spec }` MUST be emitted

### Scenario: Malformed spectty_spec payload is rejected without crash
- **Given** a `spectty_spec` call missing required `spec` fields
- **When** `handle_message` dispatches it
- **Then** it MUST return an error/benign result without panicking and MUST NOT upsert a partial blob
