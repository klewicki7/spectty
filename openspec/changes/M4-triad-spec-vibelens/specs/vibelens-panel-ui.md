# Capability: vibelens-panel-ui

> M4 delta spec. NEW capability established by change `M4-triad-spec-vibelens`.
> RFC 2119 keywords are normative. Full prose + tags live in the change-level `spec.md`.

The UI gains a VibeLens panel rendering the `DiffExplanation` reacting to `diff_updated` (per-file
rationale) with a manual refresh control, surfacing the degraded "unavailable"/"parse error" state.
Same hook/test conventions as the Spec pane.

## ADDED Requirements

### Requirement: The VibeLens panel renders per-file rationale from diff_updated
The panel MUST render the `DiffExplanation` per-file rationale and update on each `diff_updated` event
without a manual refresh. An empty explanation MUST render a "no changes" state; a degraded state MUST
render "unavailable"/"parse error" (not a crash or blank panel).

#### Scenario: A diff_updated event renders per-file rationale
- **Given** the mounted VibeLens panel with the listener mocked
- **When** a `diff_updated` event delivers a multi-file explanation
- **Then** the panel MUST render each file's rationale

#### Scenario: A degraded explanation renders an unavailable state
- **Given** the panel receiving a degraded/unavailable explanation state
- **When** it renders
- **Then** it MUST show an "unavailable"/"parse error" indicator and MUST NOT blank or crash

### Requirement: A manual refresh control re-runs the diff explanation
The panel MUST provide a manual refresh control that triggers a fresh diff explanation for the session,
independent of the automatic FileWatch/cooperative trigger.

#### Scenario: Manual refresh triggers a fresh explanation
- **Given** the VibeLens panel with `invoke` mocked
- **When** the user clicks manual refresh
- **Then** a refresh invocation for the session MUST be issued (forcing a fresh diff explanation)
