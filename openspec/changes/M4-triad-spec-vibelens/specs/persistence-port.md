# Capability: persistence-port

> M4 delta spec. MODIFIED capability (extends `persistence-port` baseline from M0).
> Change `M4-triad-spec-vibelens`. RFC 2119 keywords are normative. Full prose +
> verification-class tags live in the change-level `spec.md`.

M4 implements the `EngramAdapter` `todo!()` against engram's local HTTP API on `:7437` and adds a
SEPARATE adapter/`src-tauri`-side subscribe/poll seam (NOT the Core port). The shipped SYNC/String
port is UNCHANGED (Decision 2). Canonical topic_key: `spectty/{session_id}/spec|progress|cost`
(Decision 5). `serde_json`/reqwest/async stay adapter-side; Core gains no dependency.

## MODIFIED Requirements

### Requirement: EngramAdapter skeleton proves the adapter shape
(Previously: `todo!()` skeleton with no network — M0.) `EngramAdapter` MUST now implement
`PersistencePort` against engram's local HTTP API: `upsert` = POST create-or-update by `topic_key`;
`get` = read by `topic_key` → `Ok(Some(payload))` / `Ok(None)`. HTTP/reqwest/async/serialization
live in the adapter. Engram-down MUST map to `PersistenceError::Backend`, log, retain last-known,
and MUST NOT crash the session. The HTTP seam MUST be injectable for unit tests with a fake transport.

#### Scenario: upsert then get round-trips the payload (fake transport)
- **Given** an `EngramAdapter` over a fake HTTP transport
- **When** `upsert("spectty/42/spec", payload)` then `get("spectty/42/spec")` run
- **Then** the read MUST return `Ok(Some(payload))` unchanged

#### Scenario: get of an absent topic_key returns Ok(None)
- **Given** a fake transport with no observation for the key
- **When** `get("spectty/99/spec")` runs
- **Then** it MUST return `Ok(None)` (absence is not an error)

#### Scenario: engram unreachable degrades without crashing
- **Given** a fake transport returning a connection error
- **When** `upsert` or `get` runs
- **Then** it MUST return `Err(PersistenceError::Backend(_))`, MUST NOT panic, AND the session MUST stay alive

## ADDED Requirements

### Requirement: PersistencePort signature is UNCHANGED by M4
The Core `PersistencePort` trait MUST keep its shipped sync/`&self`/`String`/`Option<String>` shape
exactly; M4 MUST NOT add async/subscribe/search/`serde_json::Value` or any method. `crates/core`
gains NO new external dependency (cargo-deny quarantine intact).

#### Scenario: Persistence port shape is unchanged after M4
- **Given** the `PersistencePort` trait after M4
- **When** its method set is inspected
- **Then** it MUST match the M0 contract exactly with no method added or removed

#### Scenario: Core has no new external dependency
- **Given** `crates/core` after M4
- **When** cargo-deny and the boundary test run
- **Then** Core MUST contain no engram/reqwest/notify/git/`serde_json::Value`/tauri dependency

### Requirement: A per-session subscribe/poll seam detects change and emits without touching the port
An adapter/`src-tauri`-side poll seam MUST read `spectty/{session_id}/spec` (and `/progress`) on a
configurable interval (default 2 s), detect change via `updated_at`/`since`, and invoke a callback
EXACTLY ONCE per change. Injectable for unit tests with a fake reader.

#### Scenario: A changed observation triggers exactly one callback
- **Given** a fake reader returning `updated_at=t1` then `t2 (> t1)`
- **When** two poll ticks fire
- **Then** the callback MUST be invoked EXACTLY ONCE (second tick) with the new payload

#### Scenario: An unchanged observation does not re-emit
- **Given** the seam after consuming `t2`
- **When** the next tick reads the same `t2`
- **Then** the callback MUST NOT be invoked again

#### Scenario: A poll error is tolerated and the loop continues
- **Given** a fake reader erroring on one tick then returning a change
- **When** the ticks fire
- **Then** the errored tick MUST NOT stop the loop AND the subsequent change MUST still emit
