# Capability: persistence-port

> Living baseline spec. Established by change `M0-scaffold` (archived 2026-06-08).
> Extended by change `M4-triad-spec-vibelens` (archived 2026-06-17).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The persistence contract MUST live in Core as a port. Adapters implement it. Engram is the
first adapter, present as a skeleton in M0 (no daemon, no network), with a real HTTP impl in M4.

> Implementation note (M0 apply-time, FINAL): the `PersistencePort` contract takes
> `&self` (not `&mut self`) and `get` returns `Result<Option<String>, PersistenceError>`
> where `PersistenceError::Backend(String)` is the only variant. A missing key is
> `Ok(None)`, not an error. `&self` makes the port shareable as `Arc<dyn PersistencePort>`
> across concurrent Sessions, with mutability encapsulated inside the adapter via interior
> mutability. The async transition is deferred to M3; the `&self` shape is final.
>
> M4 amendment: The Core `PersistencePort` trait is UNCHANGED in shape (sync/`&self`/`String`/`Option<String>`). M4 implements `EngramAdapter` against engram's local HTTP API (`:7437`) with real effects. A separate adapter-side subscribe/poll seam (not the port itself) detects changes and surfaces events. `serde_json`/reqwest/async stay adapter-side; Core gains no new dependency.

## Requirement: PersistencePort contract defined in Core
`spectty-core` MUST define a `PersistencePort` trait exposing write and read operations
only. The trait MUST NOT reference engram, HTTP, tauri, or any adapter type — it is a pure
contract.

### Scenario: Port exposes write and read only
- **Given** the `PersistencePort` trait in `spectty-core`
- **When** its method set is inspected
- **Then** it MUST expose a write operation and a read operation, and MUST NOT expose engram/HTTP/adapter-specific methods

## Requirement: In-memory stub adapter satisfies the contract
An in-memory stub adapter MUST implement `PersistencePort` and MUST be usable in unit
tests with no external dependencies. A unit test MUST perform a write → read
round-trip and assert the stored value round-trips unchanged.

### Scenario: write → read round-trip returns the stored value
- **Given** an in-memory stub adapter implementing `PersistencePort`
- **When** a test writes value `V` under key `K`, then reads key `K`
- **Then** the read MUST return value `V` unchanged

### Scenario: read of a missing key returns empty (negative/guard)
- **Given** an in-memory stub adapter with no value stored under key `K`
- **When** a test reads key `K`
- **Then** the read MUST return the empty/absent result (`Ok(None)`) and MUST NOT error or return a stale value

## Requirement: EngramAdapter skeleton proves the adapter shape (M0)
`spectty-adapters` MUST contain an `EngramAdapter` that implements `PersistencePort` with
method bodies as `todo!()`. It proves the adapter shape without any network call. No engram
daemon is required for M0 exit.

### Scenario: EngramAdapter implements the port without running a daemon
- **Given** the `EngramAdapter` in `spectty-adapters`
- **When** the crate compiles and the adapter is type-checked against `PersistencePort`
- **Then** it MUST satisfy the `PersistencePort` trait, its method bodies MUST be `todo!()`, and compiling/type-checking it MUST NOT require a running engram daemon or any network access

## Requirement: PersistencePort signature is UNCHANGED by M4  [unit] [ci]  (M4 ADDED)

The Core `PersistencePort` trait MUST keep its shipped sync/`&self`/`String`/`Option<String>` shape exactly; M4 MUST NOT add async/subscribe/search/`serde_json::Value` or any method. `crates/core` gains NO new external dependency (cargo-deny quarantine intact).

### Scenario: Persistence port shape is unchanged after M4
- **Given** the `PersistencePort` trait after M4
- **When** its method set and signatures are inspected
- **Then** they MUST match the M0 contract exactly (sync, `&self`, `String` payload, `Option<String>` read, single `Backend` error variant) with no method added or removed

### Scenario: Core has no new external dependency
- **Given** `crates/core` after M4
- **When** cargo-deny and the boundary test run
- **Then** `crates/core` MUST contain no engram/reqwest/notify/git/serde_json::Value/tauri dependency

## Requirement: EngramAdapter upserts and reads via engram HTTP and degrades gracefully  [unit]  (M4 ADDED)

`EngramAdapter` MUST implement `PersistencePort` against engram's local HTTP API: `upsert` maps to a POST that creates-or-updates an observation keyed by `topic_key`; `get` maps to a read keyed by `topic_key` returning `Ok(Some(payload))` when present and `Ok(None)` when absent. All HTTP/reqwest/async/serialization MUST live in the adapter. When engram is unreachable or returns an error, the adapter MUST map it to `PersistenceError::Backend`, MUST log, MUST retain last-known state, and MUST NOT panic or crash the session. The HTTP seam MUST be injectable so behavior is unit-testable with a fake transport (no real daemon).

### Scenario: upsert then get round-trips the payload (fake transport)  [unit]
- **Given** an `EngramAdapter` over a fake HTTP transport
- **When** `upsert("spectty/42/spec", payload)` then `get("spectty/42/spec")` run
- **Then** the read MUST return `Ok(Some(payload))` unchanged

### Scenario: get of an absent topic_key returns Ok(None)  [unit]
- **Given** an `EngramAdapter` over a fake transport with no observation for the key
- **When** `get("spectty/99/spec")` runs
- **Then** it MUST return `Ok(None)` (absence is not an error)

### Scenario: engram unreachable degrades without crashing  [unit]
- **Given** an `EngramAdapter` whose fake transport returns a connection error
- **When** `upsert` or `get` runs
- **Then** it MUST return `Err(PersistenceError::Backend(_))`, MUST NOT panic, AND the calling session MUST remain alive (degrade, do not crash)

## Requirement: A per-session subscribe/poll seam detects change and emits without touching the port  [unit]  (M4 ADDED)

An adapter/`src-tauri`-side poll seam MUST read `spectty/{session_id}/spec` (and `/progress`) on a configurable interval (default 2 s), detect change via `updated_at`/`since`, and invoke a callback EXACTLY ONCE per change. Injectable for unit tests with a fake reader.

### Scenario: A changed observation triggers exactly one callback
- **Given** a fake reader returning `updated_at=t1` then `t2 (> t1)`
- **When** two poll ticks fire
- **Then** the callback MUST be invoked EXACTLY ONCE (second tick) with the new payload

### Scenario: An unchanged observation does not re-emit
- **Given** the seam after consuming `t2`
- **When** the next tick reads the same `t2`
- **Then** the callback MUST NOT be invoked again

### Scenario: A poll error is tolerated and the loop continues
- **Given** a fake reader erroring on one tick then returning a change
- **When** the ticks fire
- **Then** the errored tick MUST NOT stop the loop AND the subsequent change MUST still emit
