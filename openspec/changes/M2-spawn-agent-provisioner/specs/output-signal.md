# Capability: output-signal

> M2 delta spec. New capability established by change `M2-spawn-agent-provisioner`.
> RFC 2119 keywords are normative. Full prose + verification-class tags live in the
> change-level `spec.md`.

`OutputSignal` is the Core serde value type consumed by `AgentRunner::detect_status`. Its
PRODUCER (ANSI strip + rolling window) is impure adapter code on a SECOND, independent
consumer of the PTY read stream that can never throttle the M1 render path.

## ADDED Requirements

### Requirement: OutputSignal is a Core serde value type with a non-Instant time field
`spectty-core` MUST define `OutputSignal` carrying at minimum an ANSI-stripped rolling text
window, an activity indicator, an optional exit code, and a SERDE-FRIENDLY time field
(elapsed-millis or an injected `Timestamp` — NEVER `std::time::Instant`). It MUST be
constructible in a test without a PTY.

#### Scenario: OutputSignal round-trips through serde and carries no Instant
- **Given** an `OutputSignal` value
- **When** it is serialized and deserialized
- **Then** the round-trip MUST succeed AND the time field MUST be a serde-friendly value, NOT
  `std::time::Instant`

#### Scenario: OutputSignal is constructible without a PTY
- **Given** a test that needs to drive `detect_status`
- **When** it constructs an `OutputSignal` directly
- **Then** construction MUST succeed with no real PTY, no real process, and no ANSI bytes
  required

### Requirement: The OutputSignal producer runs on an independent read-stream consumer
The producer MUST live in `crates/adapters`, be driven from a SECOND consumer of the PTY read
stream (independent of the M1 `pty_output` Channel), use a BOUNDED buffer, and DROP OLDEST on
overflow rather than block. The ANSI-strip + rolling-window assembly MUST be a pure unit.

#### Scenario: The producer strips ANSI and maintains a bounded rolling window
- **Given** raw byte chunks containing ANSI escape sequences fed to the producer
- **When** the producer assembles the rolling window
- **Then** the `OutputSignal` text window MUST contain the plain text with ANSI removed AND
  MUST NOT exceed the configured rolling-window size, asserted as a pure unit

#### Scenario: The producer cannot back-pressure the M1 render Channel
- **Given** the second consumer feeding the producer with a bounded buffer
- **When** the producer falls behind (buffer would overflow)
- **Then** it MUST drop the oldest buffered data rather than block, so the M1 render path is
  never throttled
