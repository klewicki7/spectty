# Capability: pipeline-augmentation

> M3 delta spec. MODIFIED capability (extends `session-runtime` from M2) established by
> change `M3-hook-status-detection`. RFC 2119 keywords are normative. Full prose +
> verification-class tags live in the change-level `spec.md`.

Hook-sourced `Observed` events flow through the SAME `observe_and_diff → transition()` pipeline
as PTY-scraped observations. The QUIESCE(200ms) tick in `run_signal_loop` gains a state-file
poll step. `detect_status` is NOT modified (stays pure PTY-only, D24). Each event is consumed
exactly once (consume-once semantics over the `ts` field).

## MODIFIED Requirements

### Requirement: run_signal_loop reads the state file on QUIESCE ticks and emits Observed
`run_signal_loop` MUST augment each QUIESCE(200ms) tick to read the per-session state file.
If `ts` STRICTLY GREATER than last-consumed-ts, it MUST map status → `Observed` and feed
`observe_and_diff`. After feeding, it MUST record the new consumed-ts. Malformed or absent
files MUST be silently ignored.

#### Scenario: A new state file event triggers one Observed emission
- **Given** a fake state-file reader returning `{"status":"Ready","ts":1000}` and
  last-consumed-ts = 0
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST receive EXACTLY ONE `Observed::Ready` AND consumed-ts
  MUST be updated to 1000

#### Scenario: Same ts is not re-emitted on a subsequent tick
- **Given** the watcher after consuming a `ts=1000` event and the file still reads ts=1000
- **When** the next QUIESCE tick fires
- **Then** `observe_and_diff` MUST NOT receive a second emission

#### Scenario: A newer ts supersedes without re-emitting the old one
- **Given** the watcher after consuming `ts=1000` and the file now reads `ts=2000, Working`
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST receive `Observed::Working` (ts 2000) once

#### Scenario: A malformed state file is silently ignored
- **Given** a state file containing malformed JSON or missing `status` field
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST NOT receive any emission AND consumed-ts MUST remain unchanged

#### Scenario: An absent state file on a tick is silently ignored
- **Given** no state file present at the expected path
- **When** the QUIESCE tick fires
- **Then** `observe_and_diff` MUST NOT receive any emission AND no error is returned

### Requirement: Hook-sourced Observed events go through the same transition() authority
The Core `transition()` function (M2, UNCHANGED) MUST remain the sole authority. Hook-derived
`Observed` events MUST be processed by `transition(current, observed)` identically to
scrape-derived ones. No hook-specific bypass of the transition table is permitted.

#### Scenario: Hook-derived Ready observation is rejected by transition if current is Starting
- **Given** `current = Starting` and the watcher emits `Observed::Ready`
- **When** `transition(Starting, Ready)` runs
- **Then** it MUST return `Starting` unchanged (Starting → Idle is the only legal first step)

#### Scenario: Hook-derived Working observation advances Idle to Running
- **Given** `current = Idle` and the watcher emits `Observed::Working`
- **When** `transition(Idle, Working)` runs
- **Then** it MUST return `Running`

### Requirement: detect_status stays pure PTY-only and is not modified by M3
`ClaudeCodeRunner::detect_status` MUST NOT be modified to read files or incorporate hook data.
It MUST remain a pure function over `OutputSignal` only (D24 lock). Scraping stays the fallback.

#### Scenario: detect_status signature and purity are unchanged after M3
- **Given** `ClaudeCodeRunner::detect_status` after M3
- **When** its signature and body are inspected
- **Then** it MUST accept only `&self` and `&OutputSignal` and MUST NOT call any filesystem
  function, read any file, or access session-specific state beyond the signal
