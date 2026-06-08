# Keybindings

> ❓ OPEN: This is a **proposal**, not a finalized spec. Exact bindings will be confirmed
> with user testing before M4. All bindings must be remappable. Nothing here is locked.

Spectty is keyboard-first. The bindings below cover every primary action. They follow a
prefix model (`Cmd+K` as the leader for session actions, similar to tmux's `Ctrl+B`)
to keep single-key shortcuts inside the terminal from conflicting with the agent's CLI.

On macOS, `Cmd` is the modifier. A future Linux build would use `Ctrl` as the equivalent
system modifier where `Cmd` is not available.

---

## Session navigation

| Action | Binding | Notes |
|---|---|---|
| Switch to next session | `Cmd+]` | wraps around |
| Switch to previous session | `Cmd+[` | wraps around |
| Jump to session 1–9 | `Cmd+1` … `Cmd+9` | by sidebar order |
| New session | `Cmd+N` | opens new-session dialog |
| Close current session | `Cmd+W` | confirms if agent is Running |
| Rename current session | `Cmd+Shift+R` | inline rename in sidebar |

---

## Pane management

| Action | Binding | Notes |
|---|---|---|
| Split pane horizontally | `Cmd+D` | adds column to the right |
| Split pane vertically | `Cmd+Shift+D` | adds row below |
| Close current pane | `Cmd+Shift+W` | session keeps running in background |
| Focus next pane | `Cmd+Tab` | cycles through visible panes |
| Focus previous pane | `Cmd+Shift+Tab` | reverse cycle |
| Zoom current pane | `Cmd+Z` | maximizes pane; press again to restore |

---

## Panel toggles

| Action | Binding | Notes |
|---|---|---|
| Toggle VibeLens panel | `Cmd+L` | collapses/expands below the terminal |
| Toggle Sessions sidebar | `Cmd+B` | collapses/expands the left sidebar |
| Toggle Dashboard | `Cmd+Shift+D` | replaces pane area with Dashboard view |
| Open command palette | `Cmd+P` | fuzzy-find any action or session |

> ❓ OPEN: `Cmd+Shift+D` is used for both "split vertical" and "Dashboard toggle" in the
> table above — collision. Resolve before M4. One candidate: Dashboard gets `Cmd+\`,
> vertical split gets `Cmd+Shift+\`.

---

## Agent interaction

| Action | Binding | Notes |
|---|---|---|
| Approve current prompt (AwaitingInput) | `Cmd+Enter` | sends the confirmation response to the agent |
| Reject / cancel current prompt | `Cmd+Escape` | sends the rejection response |
| Send input to terminal | `Enter` | standard — typed input goes straight to PTY |

> ❓ OPEN: "Approve" semantics are agent-specific (Claude Code uses `y`/`yes`, others
> differ). The binding should trigger the correct response for the current agent's
> AwaitingInput prompt automatically. The `AgentRunner` port must expose the approval
> token. Tracked for M2.

---

## Session lifecycle

| Action | Binding | Notes |
|---|---|---|
| Merge session worktree | `Cmd+M` | merges Worktree branch to main; confirms first |
| Create Checkpoint | `Cmd+Shift+C` | snapshots current Worktree state |
| Restore last Checkpoint | `Cmd+Shift+Z` | rolls back to the most recent Checkpoint |

---

## Scrollback and history

| Action | Binding | Notes |
|---|---|---|
| Scroll terminal up | `Cmd+Up` or scroll gesture | standard terminal behavior |
| Scroll terminal down | `Cmd+Down` or scroll gesture | |
| Jump to terminal top | `Cmd+Home` | |
| Jump to terminal bottom | `Cmd+End` | |
| Search terminal output | `Cmd+F` | xterm.js search addon |

---

## Remapping

All bindings are remappable via a config file (location TBD at M0 — likely
`~/Library/Application Support/Spectty/keybindings.json` on macOS).

The config format will be a simple JSON map of action identifiers to binding strings:

```json
{
  "session.next":         "Cmd+]",
  "session.prev":         "Cmd+[",
  "session.new":          "Cmd+N",
  "panel.vibelens.toggle": "Cmd+L",
  "agent.approve":        "Cmd+Enter"
}
```

> ❓ OPEN: Finalize action identifier naming convention and the full list of remappable
> actions. Design at M3.

Chord bindings (two-key sequences) are on the roadmap but not in the MVP.

---

## Design rationale

- `Cmd+[` / `Cmd+]` for session switching mirrors browser tab navigation — muscle memory
  for developers who live in browser DevTools.
- `Cmd+L` for VibeLens mirrors "clear" in many terminals — the L mnemonic is borrowed
  and repurposed (VibeLens, not "clear").
- `Cmd+Enter` for approval is distinct from the bare `Enter` that types in the terminal,
  preventing accidental confirmations.
- Bindings avoid `Cmd+C` / `Cmd+V` (copy/paste) and other system-reserved combos.

---

## See also

- [UX Principles](ux-principles.md) — keyboard-first principle
- [Layout and Panels](layout-and-panels.md) — what each binding acts on
