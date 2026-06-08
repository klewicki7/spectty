# UX Principles

Seven rules that govern every design decision in Spectty. When a new feature is proposed
or a layout question arises, these principles are the tiebreaker — not personal taste,
not trend-following.

---

## 1. Keyboard-first, mouse optional

Every action that a user performs regularly must be reachable by keyboard without
touching the mouse. The mouse works too, but it is never the primary path.

**Why:** Spectty users are developers. Their hands are on the keyboard. Reaching for the
mouse to approve a blocked agent breaks flow and adds latency.

**UI consequence:** Every primary action (switch session, approve a prompt, toggle the
VibeLens panel, open the Dashboard) has a keyboard binding. Mouse interaction is
available but is never the only path. See [Keybindings](keybindings.md).

---

## 2. Glanceability — status in under one second

A user looking at Spectty for the first time in 30 seconds must be able to answer
"which agents are done, which are running, which are blocked?" in under one second,
without reading text.

**Why:** When supervising 3–5 agents in parallel, the overhead of interpreting each
session's state must be near zero. Reading text is too slow; scanning icons and colors
is not.

**UI consequence:** Every Session in the sidebar has a visible status indicator
(color + icon) that maps unambiguously to `AgentStatus`. Text labels supplement the
indicator, never replace it. The Dashboard is the extreme version of this: all sessions
at once, status-color-coded.

---

## 3. Never-lose-an-agent

A Session in `AwaitingInput` or `Error` state must be impossible to overlook. The UI
must actively demand the user's attention for these two states.

**Why:** The #1 failure mode when running multiple agents is not noticing one is stuck
waiting for a permission prompt. An unnoticed `AwaitingInput` session wastes the agent's
context window and blocks the whole task.

**UI consequence:** `AwaitingInput` sessions pulse visually in the sidebar and on the
Dashboard. An OS notification fires exactly once per transition into this state.
`Error` sessions display a distinct, high-contrast indicator. Neither state can be
"calm" or blend in.

---

## 4. Explained-not-raw

Users see the `DiffExplanation` — what the agent changed and why, per file — not a raw
`git diff`. Raw diffs are available on demand but are never the default view.

**Why:** Spectty's users are reviewing agent work, not writing patches. A raw diff is
optimized for patch authors; an explained diff is optimized for reviewers. Showing the
raw diff by default is solving the wrong problem.

**UI consequence:** The VibeLens panel is always visible below the terminal in every
Session Pane. It shows the latest `DiffExplanation` live, updating as the agent works.
A "view raw diff" escape hatch exists but is secondary.

---

## 5. Calm notifications — signal, don't spam

Spectty sends OS notifications only when a Session transitions to `AwaitingInput` or
`Error`. Once per transition, not repeatedly. No notifications for `Running`,
`Completed`, or routine progress.

**Why:** Notification fatigue kills the value of notifications. If Spectty notifies on
every event, users will turn notifications off — and then miss the one that matters.

**UI consequence:** Notification rules are not configurable by default (no "notify me
on Completed" checkbox in the MVP). The `AwaitingInput` case is the only genuinely
urgent event. `Completed` is surfaced passively via the Dashboard status change.

> ❓ OPEN: Post-MVP, allow users to opt into `Completed` notifications per workspace.
> Tracked in [features](../product/features.md).

---

## 6. Isolation is invisible

Worktrees are created and removed automatically. Users do not configure them, name
branches manually, or run `git worktree add` themselves.

**Why:** The isolation benefit (parallel agents without file conflicts) is real, but
the mechanism is an implementation detail. Exposing `git worktree` complexity to users
replaces one problem (file conflicts) with another (cognitive overhead).

**UI consequence:** When a new Session is created, Spectty creates a Worktree silently
using a sensible branch name derived from the Session title. Merge to main is
one action (a button or keybinding), not a sequence of git commands. The Worktree path
and branch name are visible in the Session details panel but not prominent.

---

## 7. The cockpit, not the pilot

Spectty supervises. It does not write code, suggest prompts, or complete the user's
sentences. The Agent writes code; the user sets direction; Spectty keeps the instruments
visible.

**Why:** Adding "smart" features to Spectty (autocomplete, AI-assisted session management,
prompt suggestions) risks confusing the supervision surface with the execution surface.
Users need to trust that Spectty faithfully shows what the Agent did — not what Spectty
thinks the Agent intended.

**UI consequence:** Every piece of text in the VibeLens panel is sourced from the
Agent's actual diff plus the `DiffExplainerPort`'s analysis. Spectty adds no editorial
commentary. The supervision UI does not suggest what to do next. Controls are neutral:
"Approve", "Reject", "Merge" — not "Looks good! Merge?" or "I recommend approving".
