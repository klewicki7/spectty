# Capability: spectty-diff-effect

> Living baseline spec. Established by change `M4-triad-spec-vibelens` (archived 2026-06-17).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

`spectty_diff` (advertised schema `{session_id, hint?}` UNCHANGED) gains a real EFFECT: a cooperative signal that triggers the diff pipeline immediately, bypassing the FileWatch debounce. The `emits_diff_signals` capability (false for claude-code today) governs the cooperative path; otherwise the FileWatch fallback drives the pipeline.

## Requirement: spectty_diff cooperatively triggers the pipeline, bypassing debounce

A `tools/call` for `spectty_diff` MUST trigger the diff pipeline for `session_id` WITHOUT waiting for the FileWatch debounce, then return promptly. With no cooperative signal (generic tier), the FileWatch debounced trigger MUST drive the pipeline. Both paths converge on the SAME pipeline.

### Scenario: A cooperative spectty_diff bypasses the debounce
- **Given** a session with the diff pipeline wired and a fake clock
- **When** a `spectty_diff { session_id }` signal arrives
- **Then** the pipeline MUST run immediately without waiting for the debounce window

### Scenario: Generic tier falls back to the debounced FileWatch trigger
- **Given** a session whose agent has `emits_diff_signals = false` and a debounced `FileWatchPort`
- **When** files change with no cooperative signal
- **Then** the pipeline MUST run via the debounced FileWatch trigger (degrades gracefully) — same pipeline, no cooperative signal required
