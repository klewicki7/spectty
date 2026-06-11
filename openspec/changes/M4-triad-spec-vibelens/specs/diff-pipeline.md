# Capability: diff-pipeline

> M4 delta spec. NEW capability established by change `M4-triad-spec-vibelens`.
> RFC 2119 keywords are normative. Full prose + tags live in the change-level `spec.md`.

Pipeline: trigger (FileWatch debounced 500 ms–1 s OR cooperative `spectty_diff`) → `GitPort::diff_head`
→ hash-dedup vs `last_diff_hash` → `DiffExplainerPort::explain` (MCP client of VibeLens server, tool
`show_diff_explanation`) → `Session::update_diff` → `diff_updated` (Decision 4). Ports are Core-owned;
impls in adapters. Empty-repo and all failure modes degrade without crashing.

## ADDED Requirements

### Requirement: FileWatchPort / DiffExplainerPort / GitPort are Core-owned interfaces with adapter impls
`spectty-core` MUST define `FileWatchPort`, `DiffExplainerPort` (`explain(diff, workspace) ->
Result<DiffExplanation>`), `GitPort` (`diff_head`) as pure traits, and `DiffExplanation` as a pure
serde entity with an `empty()` form. All impls (notify, VibeLens MCP client, git) live in
adapters/`src-tauri`; Core gains no new dependency.

#### Scenario: The three ports are Core traits with no I/O dep
- **Given** `crates/core` after M4
- **When** the port modules are inspected and cargo-deny runs
- **Then** `FileWatchPort`, `DiffExplainerPort`, `GitPort`, `DiffExplanation` MUST be present and pure
  AND Core MUST carry no `notify`/`git`/MCP/`reqwest` dependency

#### Scenario: DiffExplanation round-trips and has an empty form
- **Given** a `DiffExplanation` value and `DiffExplanation::empty()`
- **When** each is serialized then deserialized
- **Then** both MUST round-trip unchanged AND `empty()` MUST represent "no diff to explain"

### Requirement: The diff pipeline hash-dedups and skips redundant explanations
The pipeline MUST hash `git diff HEAD` and compare against the Session's `last_diff_hash`. Unchanged
hash MUST skip `explain` (no redundant MCP call). Changed hash MUST call `explain`, update Session
with the new `DiffExplanation` + hash, and emit `diff_updated`. Empty-repo MUST diff vs the empty tree;
a truly empty diff MUST yield `DiffExplanation::empty()` with NO MCP call.

#### Scenario: An unchanged diff hash skips the explainer
- **Given** a Session whose `last_diff_hash` equals the current diff hash
- **When** the pipeline runs
- **Then** `explain` MUST NOT be called AND no `diff_updated` MUST be emitted

#### Scenario: A changed diff hash explains and emits once
- **Given** a Session whose `last_diff_hash` differs from the current diff hash
- **When** the pipeline runs over a fake `DiffExplainerPort`
- **Then** `explain` MUST be called once, the Session MUST store the new explanation + hash, AND
  EXACTLY ONE `diff_updated { session_id, explanation }` MUST be emitted

#### Scenario: A truly empty diff yields empty() with no MCP call
- **Given** an empty repository diffed against the empty tree
- **When** the pipeline runs
- **Then** the result MUST be `DiffExplanation::empty()` AND `explain` MUST NOT be called

### Requirement: The diff pipeline degrades gracefully on every failure mode
When the VibeLens MCP client is unreachable/times out/errors/returns unparseable output, OR
`GitPort::diff_head` fails, the pipeline MUST log, retain the previous `DiffExplanation`, surface an
"unavailable"/"parse error" state, and MUST NOT crash the session.

#### Scenario: VibeLens unreachable retains previous explanation
- **Given** a Session with a prior `DiffExplanation` and a fake `DiffExplainerPort` returning a connection error
- **When** the pipeline runs
- **Then** the prior explanation MUST be retained, an "unavailable" state MUST be surfaced, AND the session MUST stay alive

#### Scenario: A git failure does not crash the session
- **Given** a fake `GitPort` whose `diff_head` returns an error
- **When** the pipeline runs
- **Then** it MUST log and surface a degraded state without panicking or terminating the session
