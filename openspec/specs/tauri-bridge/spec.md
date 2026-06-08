# Capability: tauri-bridge

> Living baseline spec. Established by change `M0-scaffold` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

The Bridge proves bidirectional communication between the Rust shell and the React UI via
one command and one event.

## Requirement: ping command emits an observable pong event
The `src-tauri` bridge MUST expose a `ping` Tauri command (Tauri v2). Invoking it MUST
result in a `pong` event emitted via the v2 `AppHandle::emit` API, observable in the
running app.

### Scenario: ping → pong is visible in the running app
- **Given** the app running via `pnpm tauri dev`
- **When** the UI invokes the `ping` command
- **Then** a `pong` event MUST be emitted by the bridge AND the UI listener MUST observe it and log it to the web console

### Scenario: Bridge uses Tauri v2 emit API (guard against v1 drift)
- **Given** the bridge implementation of `pong`
- **When** the emit call is inspected
- **Then** it MUST use the Tauri v2 `AppHandle::emit` API (via the `Emitter` trait) and MUST NOT use removed Tauri v1 emit signatures
