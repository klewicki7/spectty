# Capability: persistence-port

> Living baseline spec. Established by change `M0-scaffold` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The persistence contract MUST live in Core as a port. Adapters implement it. Engram is the
first adapter, present as a skeleton only in M0 (no daemon, no network).

> Implementation note (M0 apply-time, FINAL): the `PersistencePort` contract takes
> `&self` (not `&mut self`) and `get` returns `Result<Option<String>, PersistenceError>`
> where `PersistenceError::Backend(String)` is the only variant. A missing key is
> `Ok(None)`, not an error. `&self` makes the port shareable as `Arc<dyn PersistencePort>`
> across concurrent Sessions, with mutability encapsulated inside the adapter via interior
> mutability. The async transition is deferred to M3; the `&self` shape is final.

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

## Requirement: EngramAdapter skeleton proves the adapter shape
`spectty-adapters` MUST contain an `EngramAdapter` that implements `PersistencePort` with
method bodies as `todo!()`. It proves the adapter shape without any network call. No engram
daemon is required for M0 exit. Real engram HTTP and polling/subscribe are DEFERRED to M3.

### Scenario: EngramAdapter implements the port without running a daemon
- **Given** the `EngramAdapter` in `spectty-adapters`
- **When** the crate compiles and the adapter is type-checked against `PersistencePort`
- **Then** it MUST satisfy the `PersistencePort` trait, its method bodies MUST be `todo!()`, and compiling/type-checking it MUST NOT require a running engram daemon or any network access
