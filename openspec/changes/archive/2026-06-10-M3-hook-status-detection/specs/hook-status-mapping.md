# Capability: hook-status-mapping

> M3 delta spec. New capability established by change `M3-hook-status-detection`.
> RFC 2119 keywords are normative. Full prose + verification-class tags live in the
> change-level `spec.md`.

The mapping from state-file status strings to `Observed` variants is DATA (a const table) in
`crates/adapters`, not Core logic. The injected hook entries point `spectty-hook --status`
at these five locked string values. The watcher reads the string and maps it to `Observed`
via the same table. Matcher strings for `Notification` are also adapter-level constants.

## ADDED Requirements

### Requirement: The five hook events map to Observed variants via a pure table
`crates/adapters` MUST define a PURE mapping function (or const lookup table) from the five
status strings to `Observed` variants. Unrecognized strings MUST map to `None`.

| State file `status` | `Observed` variant | Hook event |
|---|---|---|
| `"Working"` | `Observed::Working` | `UserPromptSubmit` |
| `"Ready"` | `Observed::Ready` | `Stop` |
| `"NeedsInput"` | `Observed::NeedsInput` | `Notification` (permission_prompt) |
| `"Finished"` | `Observed::Finished` | `SessionEnd` |
| `"Failed"` | `Observed::Failed` | `StopFailure` |

#### Scenario: "Ready" maps to Observed::Ready
- **Given** the hook-status mapping function
- **When** it is called with `"Ready"`
- **Then** it MUST return `Some(Observed::Ready)`

#### Scenario: "Working" maps to Observed::Working
- **Given** the hook-status mapping function
- **When** it is called with `"Working"`
- **Then** it MUST return `Some(Observed::Working)`

#### Scenario: "NeedsInput" maps to Observed::NeedsInput
- **Given** the hook-status mapping function
- **When** it is called with `"NeedsInput"`
- **Then** it MUST return `Some(Observed::NeedsInput)`

#### Scenario: "Finished" maps to Observed::Finished
- **Given** the hook-status mapping function
- **When** it is called with `"Finished"`
- **Then** it MUST return `Some(Observed::Finished)`

#### Scenario: "Failed" maps to Observed::Failed
- **Given** the hook-status mapping function
- **When** it is called with `"Failed"`
- **Then** it MUST return `Some(Observed::Failed)`

> **DEVIATION (PR-4)**: The `StopFailure` / `"Failed"` scenario is spec-conformant at the
> mapping level (`HookEvent::StopFailure → Observed::Failed` is implemented and tested), but
> the corresponding Claude Code hook (`SubagentStop`) is **not** registered in the production
> event list. `SubagentStop` fires on every subagent completion (success AND failure) with no
> failure discriminator in the payload; registering it would drive healthy sessions to `Error`.
> The `StopFailure` hook source is deferred until Claude Code exposes a failure-discriminating
> event. `Error` remains reachable via non-hook paths. This scenario documents the INTENDED
> eventual behaviour; implementation of the hook registration is a future work item.

#### Scenario: An unrecognized status string maps to None
- **Given** the hook-status mapping function
- **When** it is called with any string not in the five locked values
- **Then** it MUST return `None` — the watcher silently ignores the event

### Requirement: The hook event settings.json shape is DATA in the adapter
The settings.json `hooks` value injected by `inject_spectty_hooks` MUST embed the hook
configuration as DATA (constants) in the adapter. No-matcher events (`Stop`, `UserPromptSubmit`,
`SessionEnd`, `StopFailure`) MUST have no `matcher` field. The `Notification`
(permission-prompt) event MUST have a `matcher` field with the empirical constant string.

#### Scenario: No-matcher events have no matcher field in the injected JSON
- **Given** the output of `inject_spectty_hooks` for a session
- **When** the `Stop` and `UserPromptSubmit` hook entries are inspected
- **Then** neither MUST contain a `matcher` field (absent, not null)

#### Scenario: Notification event has a permission-prompt matcher
- **Given** the output of `inject_spectty_hooks` for a session
- **When** the `Notification` hook entry is inspected
- **Then** it MUST contain a `matcher` field with a non-empty string (the adapter-level
  empirical permission-prompt constant)
