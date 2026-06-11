# Capability: triad-layout

> M4 delta spec. NEW capability established by change `M4-triad-spec-vibelens`.
> RFC 2119 keywords are normative. Full prose + tags live in the change-level `spec.md`.

The session view MUST present the triad — Spec pane + Terminal + VibeLens panel — all visible for a
single session (roadmap exit criterion 8).

## ADDED Requirements

### Requirement: The triad layout shows Spec pane, Terminal, and VibeLens per session
The session view MUST lay out three regions simultaneously for one session: the Spec pane, the existing
Terminal pane, and the VibeLens panel. All three MUST be visible without navigating away.

#### Scenario: All three triad regions render for a session
- **Given** the mounted session view for one active session
- **When** it renders
- **Then** the Spec pane, the Terminal, AND the VibeLens panel MUST all be present in the layout
