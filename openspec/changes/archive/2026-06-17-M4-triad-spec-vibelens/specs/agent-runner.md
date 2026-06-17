# Capability: agent-runner

> M4 delta spec. MODIFIED capability (W1 doc-only correction to the `agent-runner` baseline).
> Change `M4-triad-spec-vibelens`. RFC 2119 keywords are normative. Full prose + tags live in
> the change-level `spec.md`.

W1 is a ZERO-RISK documentation correction folded into M4 (Slice 1). The implementation was always
correct; the spec prose lagged. M4 locks the corrected pipeline-augmentation `transition` scenario so
it matches the M2 core table row `((Starting, Ready), Idle)`.

## MODIFIED Requirements

### Requirement: Hook-sourced Observed events go through the same transition() authority
(Previously: the pipeline-augmentation `Ready` scenario asserted `transition(Starting, Ready)` returns
`Starting` unchanged — contradicting the M2 baseline core table and this spec's own prose.)
The Core `transition()` function MUST remain the sole authority. A hook-derived `Observed` MUST be
processed by `transition(current, observed)` identically to a scrape-derived one; no hook-specific
bypass is permitted. The `(Starting, Ready) => Idle` rule MUST be stated consistently with the core
table row `((Starting, Ready), Idle)`. This is a doc-only change — NO code change, NO new test.

#### Scenario: Hook-derived Ready observation advances Starting to Idle
- **Given** `current = Starting` and the watcher emits `Observed::Ready` (from a hook event)
- **When** `transition(Starting, Ready)` runs
- **Then** it MUST return `Idle` (matching the M2 baseline rule `(Starting, Ready) => Idle`), with no
  contradictory "Starting unchanged" wording remaining

#### Scenario: Hook-derived Working observation advances Idle to Running
- **Given** `current = Idle` and the watcher emits `Observed::Working`
- **When** `transition(Idle, Working)` runs
- **Then** it MUST return `Running` (the legal Idle → Running transition)
