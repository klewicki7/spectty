# Layout and Panels

The spatial model of Spectty. Every element of the UI has a precise location in this
hierarchy: **Window → Tab → Pane → Session**.

---

## Spatial hierarchy

```
Window
└── Tab  (one or more)
    └── Pane layout  (one or more Panes, tiled)
        └── Pane  →  one Session
                      ├── Terminal area  (xterm.js)
                      └── VibeLens panel  (DiffExplanation)
```

Additionally, there is a **Dashboard** view — a special Tab-level view that replaces
the Pane layout with a cross-session overview.

- A **Window** is a top-level OS window.
- A **Tab** holds one or more Panes arranged in a tile layout.
- A **Pane** renders exactly one Session (terminal + VibeLens panel stacked vertically).
- The **Sessions sidebar** spans the left edge of the Window and is always visible.

---

## View 1 — Single session (default)

```
┌─ Sessions ──────┐┌─ agent: feature-auth ──────────────────────────┐
│ ● feature-auth  ││ $ claude code                                  │
│   fix-bug       ││ > Analyzing existing auth middleware...        │
│   refactor      ││ > Writing JWT validation in auth.ts            │
│                 ││ > Done. 3 files modified.                      │
│                 ││                                                │
│                 │├─ VibeLens ─────────────────────────────────────┤
│                 ││ auth.ts      +42 −3   adds JWT middleware      │
│                 ││ login.ts     +12 −0   validates token on login │
│                 ││ auth.test.ts  +28 −0  unit tests for middleware│
│                 ││                                                │
│  + new session  ││ ↻ updated 4s ago                              │
└─────────────────┘└────────────────────────────────────────────────┘
```

**Sessions sidebar (left):**
- Lists all active Sessions by title.
- Each entry shows a status indicator (dot / icon) and the AgentStatus color.
- Scrollable when many sessions are open.
- "+" at the bottom opens the new-session flow.
- Selected session is highlighted; its Pane is displayed to the right.

**Terminal area (top-right):**
- Full-featured xterm.js rendering of the Session's PTY output.
- Scrollable history.
- User input goes here when the Session is `AwaitingInput`.

**VibeLens panel (bottom-right):**
- Always visible; height is adjustable via drag.
- Shows the latest `DiffExplanation`: one row per `FileChange` with path, `+`/`−` counts,
  and the AI-generated rationale.
- A footer line shows when the explanation was last updated.
- Collapsible to a single header line (toggle: see [Keybindings](keybindings.md)).

---

## View 2 — Split / tiled multi-pane

When the user splits a Tab, Panes tile horizontally or vertically. Each Pane continues
to show its own terminal + VibeLens panel.

```
┌─ Sessions ──────┐┌─ feature-auth ──────────┐┌─ fix-bug ───────────┐
│ ● feature-auth  ││ $ claude code           ││ $ aider             │
│ ● fix-bug       ││ > Writing auth.ts...    ││ > Patching null ptr │
│   refactor      ││                         ││                     │
│                 │├─ VibeLens ──────────────┤├─ VibeLens ──────────┤
│                 ││ auth.ts   +42  adds JWT ││ utils.ts  +3 null   │
│                 ││ login.ts  +12  validates││                     │
│  + new session  ││ ↻ 4s ago               ││ ↻ 12s ago          │
└─────────────────┘└─────────────────────────┘└────────────────────┘
```

- Each column is one Pane bound to one Session.
- The Sessions sidebar remains on the left and spans the full height.
- A session active in a visible Pane is still highlighted in the sidebar.

> ❓ OPEN: Maximum number of simultaneous visible Panes before the layout degrades. Three
> columns is probably the practical limit on a 27" display. Investigate at M4.

---

## View 3 — Dashboard

The Dashboard replaces the Pane area with a cross-session overview. It is the place you
look when you want to survey all agents at once without focusing on any one.

```
┌─ Sessions ──────┐┌─ Dashboard ────────────────────────────────────┐
│ ● feature-auth  ││                                                │
│ ⚡ fix-bug       ││  ⚡ fix-bug          AwaitingInput   $0.18    │
│ ✓ refactor      ││    › waiting for permission: write tests.ts    │
│                 ││    › workspace: ~/proj/fix-bug                 │
│                 ││                                                │
│                 ││  ● feature-auth      Running          $1.04    │
│                 ││    › auth.ts +42  login.ts +12  (3 files)      │
│                 ││    › workspace: ~/proj/feature-auth            │
│                 ││                                                │
│                 ││  ✓ refactor          Completed        $0.61    │
│                 ││    › 8 files modified — ready to review        │
│                 ││    › workspace: ~/proj/refactor                │
│                 ││                                                │
│  + new session  ││                          [Open]  [Merge]       │
└─────────────────┘└────────────────────────────────────────────────┘
```

Each row in the Dashboard shows:
- Status indicator + AgentStatus label
- Session title + Workspace
- A one-line summary (last VibeLens summary or current agent output line)
- `CostMetrics` (estimated USD)
- Quick actions: "Open" (jump to Pane) and "Merge" (merge Worktree, when applicable)

The Dashboard is toggled via keybinding (see [Keybindings](keybindings.md)) and can
replace the Pane area without closing it — switching back returns to the previous layout.

---

## Status indicators

Every Session has a visual indicator that communicates `AgentStatus` instantly.

| AgentStatus | Indicator | Color | Behavior |
|---|---|---|---|
| `Starting` | spinner ring | neutral gray | rotating animation |
| `Idle` | hollow circle | neutral gray | static |
| `Running` | filled circle | blue | static |
| `AwaitingInput` | lightning bolt or pulse dot | amber | pulsing animation |
| `Completed` | checkmark | green | static |
| `Error` | X or exclamation | red | static |

**`AwaitingInput` pulses** — this is non-negotiable per [UX Principles](ux-principles.md)
§ "Never-lose-an-agent". The animation draws the eye even in peripheral vision.

Colors follow a semantic palette (not the system accent color). Exact hex values will
be defined in the design token file at M3.

> ❓ OPEN: Support a colorblind-accessible mode that adds shape differentiation (not just
> color) for all six status states. Tracked for M3+.

---

## Panel sizing and state persistence

- The VibeLens panel height is user-adjustable via drag.
- The Sessions sidebar width is user-adjustable via drag.
- Panel sizes are persisted across restarts (stored in app config, not a session state).
- A collapsed VibeLens panel shows only a header line with the file count and last-updated
  timestamp; expanding restores the full view.

---

## See also

- [UX Principles](ux-principles.md) — the rules behind these decisions
- [Keybindings](keybindings.md) — keyboard navigation
- [Domain Model](../architecture/domain-model.md) — Session, Worktree, DiffExplanation
- [Data Flow](../architecture/data-flow.md) — how events reach the UI
