# Capability: terminal-ui

> Living baseline spec. Established by change `M1-live-pty-terminal` (archived 2026-06-08).
> RFC 2119 keywords (MUST, MUST NOT, SHALL, SHOULD, MAY) are normative.

An `@xterm/xterm` terminal is mounted in a React Pane, renders live ANSI output, forwards
keystrokes, tracks resize, retains scrollback, and supports copy/paste. The PTY wiring is
expressed through a `useTerminal` hook mirroring the M0 `usePingPong` mock pattern.

## Requirement: xterm.js is mounted in a Pane and wired to the PTY via a hook
The UI MUST mount an `@xterm/xterm` 6 `Terminal` inside a Pane component, with the
PTY wiring expressed through a `useTerminal` (or equivalent) hook mirroring the M0
`usePingPong` mock pattern. The hook MUST: invoke `pty_spawn` on mount with a Channel,
forward `term.onData` to `send_input`, drive `pty_resize` from fit, write Channel output
into `term.write`, and on unmount dispose the terminal and invoke `pty_kill`. React 19
named imports MUST be used; manual `useMemo`/`useCallback` MUST NOT be added (the compiler
handles memoization).

> Channel decode note (resolved risk R1): Channel output arrives as `number[]` at runtime
> (not `Uint8Array`). The hook MUST decode defensively — a `decodeChannelBytes` helper that
> handles `number[]`, `ArrayBuffer`, and `Uint8Array` shapes — before feeding bytes to
> `term.write`.

### Scenario: Hook spawns on mount and tears down on unmount
- **Given** the `useTerminal` hook with `invoke`, `Channel`, and `@xterm/xterm` mocked
- **When** the component mounts and later unmounts
- **Then** `pty_spawn` MUST be invoked on mount AND on unmount the terminal MUST be disposed
  AND `pty_kill` MUST be invoked

### Scenario: Keystrokes forward to the PTY
- **Given** the mounted terminal with mocked `invoke`
- **When** `term.onData` yields input data
- **Then** the hook MUST invoke `send_input` carrying that data

### Scenario: Channel output is written to the terminal
- **Given** the mounted terminal with a mocked Channel and mocked `term.write`
- **When** the Channel delivers an output chunk
- **Then** the hook MUST decode the chunk and call `term.write` with the received bytes

## Requirement: Terminal tracks resize via fit
The UI MUST track Pane size changes (via a `ResizeObserver` and the fit addon) and, on
each fit, read the resulting columns/rows and invoke `pty_resize` so the PTY size follows
the rendered size. An initial size MUST be established at/before spawn.

### Scenario: Fit drives a pty_resize invoke
- **Given** the mounted terminal with mocked `invoke` and a mocked fit addon
- **When** a fit is triggered yielding new columns/rows
- **Then** the hook MUST invoke `pty_resize` with those columns and rows

## Requirement: Terminal renders ANSI and colors (manual acceptance)
The terminal MUST render ANSI escape sequences and colors correctly, feeding raw bytes
into `term.write` (no FE-side ANSI stripping — xterm renders raw). The UI MUST NOT include
any `OutputSignal` / ANSI-strip-for-status decode path (that is M2).

### Scenario: Interactive programs render and behave correctly
- **Given** a shell open in the Pane
- **When** the user runs `vim`, `htop`, and `git log --oneline --graph`
- **Then** each MUST render correctly (full-screen TUI layout, colors, cursor) AND behave
  correctly (keystrokes reach the program, the screen updates live without jank)

## Requirement: Terminal retains configurable scrollback (manual acceptance)
The terminal MUST be configured with a scrollback buffer of a configurable length (a
constant), and output exceeding one screen MUST remain accessible by scrolling back.

### Scenario: Scrollback is retained beyond one screen
- **Given** a shell open in the Pane
- **When** the shell produces more output than fits on one screen
- **Then** earlier output MUST remain retrievable by scrolling back up to the configured
  scrollback limit

## Requirement: Terminal supports copy and paste (manual acceptance)
The terminal MUST support copying selected text and pasting from the system clipboard,
using xterm selection plus the clipboard addon (OSC 52). Copy/paste MUST NOT require any
extra Tauri capability beyond what in-webview selection already permits.

### Scenario: Copy and paste use the system clipboard
- **Given** a shell open in the Pane with text on screen
- **When** the user selects text to copy and later pastes
- **Then** the selection MUST reach the system clipboard on copy AND clipboard contents
  MUST be delivered to the PTY on paste
