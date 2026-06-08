# Features

> The prioritized feature catalog. See [Roadmap](roadmap.md) for milestone mapping and
> [Problem Statement](problem-statement.md) for the pains these solve.

**Priority tags:** `P0` MVP must-have · `P1` post-MVP soon · `P2` later / exploratory  
**Milestones:** M0 scaffold + engram wiring · M1 live PTY · M2 spawn agent + Provisioner · M3 living Spec pane + VibeLens triad · M4 multi-session + worktree · M5 dashboard + cost + notifications · post-MVP comms panels

---

## 1. Session Management & Navigation

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Create a Session (agent + workspace + optional worktree) | P0 | M2 | Core spawn flow |
| Close / terminate a Session | P0 | M2 | Graceful shutdown + PTY teardown |
| Named sessions with user-set or auto-derived title | P0 | M2 | Derived from task or branch name |
| Tab-based session grouping within a Window | P0 | M4 | Fast navigation by name |
| Side-by-side Pane splits within a Tab | P0 | M4 | Horizontal + vertical split |
| Session list / switcher (keyboard-driven) | P0 | M4 | Jump to any session by name or status |
| Persist session layout across restarts | P1 | M5+ | Window/Pane geometry saved |
| Session templates (preset agent + workspace config) | P1 | M5+ | One-key launch of known setups |
| Layout presets (single, side-by-side, quad) | P1 | M5+ | Switchable via command palette |

---

## 2. Live PTY Terminal

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Full PTY in every Pane (xterm.js + portable-pty) | P0 | M1 | Colors, escape codes, resize |
| Keyboard input forwarded to agent process | P0 | M1 | Raw input passthrough |
| Terminal resize propagates to PTY | P0 | M1 | SIGWINCH signaling |
| Scrollback buffer per Session | P0 | M1 | Configurable length |
| Copy / paste in terminal | P0 | M1 | System clipboard integration |
| Search in scrollback | P1 | M5+ | Regex or literal string |

---

## 3. Living Spec Pane — The Soul (Core Triad)

> The marquee feature. See [Spec Pane](spec-pane.md) for the full design.

Spectty's center of gravity is the **core triad**: SPEC (what you asked) → DIFF (what the
agent did, via VibeLens) → WHY (the rationale). All three are visible and permanent at once,
for every session.

The Spec pane is a **living contract** between developer and agent: the dev seeds intent,
the agent turns it into a structured plan (proposal → tasks with live progress), and the pane
tracks execution live — done / in-progress / pending — and is steerable mid-flight. A
**plan-approval gate** fires before the agent edits any code.

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Spec pane visible alongside every Session's terminal | P0 | M3 | Persistent split, not a modal |
| Dev seeds intent → Spec pane captures it as living contract | P0 | M3 | Free-form or structured text |
| Agent turns intent into a structured plan (tasks with states) | P0 | M3 | Via `spectty_spec` MCP tool or PTY-scraping fallback |
| Plan-approval gate before agent edits code | P0 | M3 | User must approve the plan; no code until then |
| Live task progress: done / in-progress / pending | P0 | M3 | Checklist updated in real time |
| Mid-flight steering: adjust plan while agent is running | P0 | M3 | Via `spectty_spec` update or direct PTY input |
| Spec delta rendering: show what changed in the plan vs. original | P1 | M3 | Diff view of intent evolution |
| Persist spec + progress via PersistencePort (engram) | P0 | M3 | topic_key per session; survives restarts |
| Real-time re-render via polling/subscribe over PersistencePort | P0 | M3 | Engram is a store, not pub/sub — Spectty adds the event layer |
| Verify step: agent signals completion; Spectty confirms vs. plan | P0 | M3 | Closes the contract loop |
| Historical spec view (browse prior sessions' contracts) | P2 | — | Needs archive integration |

> ❓ OPEN: Polling interval for the real-time event layer — what latency is acceptable
> for live checklist updates? Starting at 500 ms; tune post-M3.

---

## 4. Spectty Agent Protocol + Provisioner

> The mechanism by which agents cooperate structurally instead of requiring PTY-scraping.
> See also [Agent Abstraction](../architecture/agent-abstraction.md).

The **Spectty Agent Protocol** injects MCP tools and skills/rules into the agent at session
start so it speaks to Spectty structurally. The **Provisioner** writes real config files in
the agent's native format using managed-section markers, atomic writes, and backup-before-write
— patterns copied from the gentle-ai CLI.

Two tiers:
- **Cooperative** — agent has the injected tools; produces structured JSON progress.
- **Generic** — no injection possible; Spectty scrapes PTY output as fallback.

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Per-agent Provisioner: inject MCP tools + skills into agent config | P0 | M2 | Native format, managed-section markers, atomic writes |
| GLOBAL scope injection (clean repo default) | P0 | M2 | Per-repo config committed when versioned (PROJECT scope) |
| `spectty_spec` tool: agent feeds plan + progress updates | P0 | M2 | Structured JSON; drives the Spec pane |
| `spectty_diff` tool: agent signals diff ready for VibeLens | P0 | M2 | Already exists as VibeLens MCP |
| `spectty_approval` tool: agent requests plan-approval gate | P0 | M2 | Blocks agent until user approves |
| `spectty_status` tool: agent reports current task status | P1 | M3 | Richer than PTY-scraping |
| `spectty_cost` tool: agent reports token/cost data | P1 | M3 | Feeds CostMetrics |
| PTY-scraping fallback for Generic (non-cooperative) agents | P0 | M2 | Idle-timeout + regex heuristics |
| Provisioner backup-before-write (never silently corrupt agent config) | P0 | M2 | Atomic writes + diff-marked sections |
| Per-session provisioner teardown on session close | P1 | M3 | Restore agent config to pre-Spectty state |

> ✅ DECIDED: PROJECT-scope protocol injection is added to .gitignore BY DEFAULT (the repo stays clean), with an explicit opt-in to commit when the injected artifact is a team-shared protocol.

---

## 5. VibeLens — Explained Diff Panel

> Part of the core triad (the DIFF + WHY). See [VibeLens Integration](../architecture/vibelens-integration.md).

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| VibeLens panel visible alongside every Session's terminal | P0 | M3 | Persistent split; part of the triad layout |
| Live DiffExplanation updated as agent edits files | P0 | M3 | FileWatcher → diff → DiffExplainerPort |
| Per-file breakdown: path, lines added/removed, change kind, AI rationale | P0 | M3 | Matches `DiffExplanation` domain model |
| One-paragraph session summary ("what happened overall") | P0 | M3 | Top of VibeLens panel |
| VibeLens fed via `spectty_diff` (Cooperative) or FileWatcher (Generic) | P0 | M3 | Protocol path is richer; both supported |
| Diff explanation refresh on demand (manual trigger) | P0 | M3 | For when auto-trigger misses a change |
| Collapsible VibeLens panel (hide when not needed) | P1 | M5+ | Panel width persisted per session |
| Historical diff explanations (scroll back through prior states) | P2 | — | Needs Checkpoint integration |

> ✅ DECIDED: The VibeLens panel is ALWAYS-ON (not opt-in) — it is the Diff leg of the core triad, the soul of the product.

---

## 6. Engram-Backed Persistence (Reused, Not Built)

> Spectty does not build a storage layer — it uses engram as an external runtime dependency
> behind `PersistencePort`. The port quarantines the coupling; the Core imports no engram types.

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| `PersistencePort` defined in Core; engram adapter implements it | P0 | M0 | Wired at scaffold; quarantines engram coupling |
| Session spec + progress persisted under topic_key per session | P0 | M3 | `spectty/{session-id}/spec`, `spectty/{session-id}/progress` |
| DiffExplanation history persisted per session | P1 | M3 | Enables historical VibeLens view |
| Session metadata (title, agent, workspace, status) persisted | P0 | M3 | Survives Spectty restarts |
| Polling/subscribe layer over engram for real-time UI re-renders | P0 | M3 | Engram is a store; Spectty adds the event loop |
| Graceful degradation when engram is unavailable | P1 | M5+ | In-memory fallback; warn user |

---

## 7. Git Worktree Isolation

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Automatic Worktree creation on session start (when enabled) | P0 | M4 | New branch + working copy per session |
| Worktree cleanup on session close | P0 | M4 | Remove worktree, optionally delete branch |
| Review-then-merge flow: inspect DiffExplanation, then merge worktree branch | P0 | M4 | Explicit user approval before merge |
| Abort / discard: delete worktree + branch, no merge | P0 | M4 | "Throw away this session's work" |
| Manual worktree association (attach existing worktree to session) | P1 | M5+ | For power users with existing worktrees |
| Worktree-per-session toggle (opt out of isolation) | P1 | M4 | Some users want shared workspace |

> ✅ DECIDED: Worktree isolation is ON BY DEFAULT (opt-out), with a per-session toggle to work in the main checkout. Parallel safety is the point; silent collisions are catastrophic. (Smart auto-escalation — isolate only when >1 session on a workspace — is a post-MVP refinement.)

---

## 8. AgentStatus Detection & Supervision

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Per-session AgentStatus state machine (Starting / Idle / Running / AwaitingInput / Completed / Error) | P0 | M2 | Core domain, surfaced in UI |
| Visual status indicator per session (color / icon in nav) | P0 | M2 | Instant at-a-glance state |
| `AwaitingInput` surfaces loudly (pulsing indicator) | P0 | M5 | Most critical state to surface |
| `Error` surfaces loudly (distinct indicator + count) | P0 | M5 | Second critical state |
| Quick actions for known permission prompts (one-key approve/deny) | P1 | M5+ | Requires `structured_permissions` capability |
| Per-agent status detector (not a global regex) | P0 | M2 | Delegated through `AgentRunner` port |

---

## 9. Dashboard

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Dashboard: cross-session overview (all sessions, status, workspace, cost) | P0 | M5 | The supervision surface |
| Filter / sort sessions by AgentStatus | P0 | M5 | "Show me all AwaitingInput first" |
| Click / jump to session from Dashboard | P0 | M5 | Navigates to the session's Pane |
| Aggregate CostMetrics across all sessions | P1 | M5 | Total spend this session / day |

---

## 10. OS Notifications

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| OS notification when a session enters `AwaitingInput` | P0 | M5 | Fire-and-forget, one per transition |
| OS notification when a session enters `Error` | P0 | M5 | With session name + last line of output |
| OS notification when a session reaches `Completed` | P1 | M5 | Optional; could be noisy |
| Notification click navigates to the session | P1 | M5+ | Deep-link into Spectty |
| Notification throttle / quiet hours | P2 | — | Avoid notification spam during fast loops |

---

## 11. CostMetrics Tracking

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Per-session live token + estimated USD display | P0 | M5 | In Pane header and Dashboard |
| Cost accumulated over session lifetime | P0 | M5 | Not just last call |
| Graceful "n/a" when agent does not report cost | P0 | M5 | Tied to `reports_cost` capability |
| Session cost reset on demand | P1 | M5+ | Clear and restart tracking |
| Budget alert threshold (notify when session exceeds $X) | P2 | — | Per-session configurable |

> ✅ DECIDED: When an agent does not report cost, the Dashboard degrades gracefully to "n/a" — Spectty never guesses cost.

---

## 12. Checkpoints & Undo

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Checkpoint: snapshot worktree state before risky agent work | P1 | M5+ | Git commit / stash on the worktree branch |
| One-click rollback to a Checkpoint | P1 | M5+ | Restore worktree to snapshot |
| Automatic Checkpoint before agent task starts | P2 | — | Requires reliable task-start signal from agent |
| Checkpoint history view | P2 | — | Browse + compare past snapshots |

> ✅ DECIDED: Checkpoints use a DEDICATED COMMIT on the worktree branch (not git stash, not a separate ref namespace) — clean, recoverable, visible in history.

---

## 13. Review-Then-Merge Flow

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Explicit "Review" state before merging worktree to main | P0 | M4 | User must accept, not auto-merge |
| DiffExplanation surfaced during review | P0 | M4 | The VibeLens panel is the review UI |
| Merge worktree branch to main (or target branch) | P0 | M4 | Via `GitPort`; conflicts surfaced |
| Discard (no merge) — delete worktree + branch | P0 | M4 | The "reject" path |
| Partial accept (cherry-pick files from worktree) | P2 | — | Complex; needs GitPort extension |

---

## 14. Command Palette

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Global command palette (fuzzy-match, keyboard-first) | P0 | M4 | Open any session, action, setting |
| Session actions via palette (new, close, navigate, merge, discard) | P0 | M4 | All session lifecycle via keyboard |
| Agent actions via palette (send input, quick-approve) | P1 | M5+ | For `AwaitingInput` handling |

---

## 15. Comms Panels — Slack & Gmail (Post-MVP orbit)

> Comms panels are the **"dead-time" layer** — the surface you use *while the agent works*.
> They orbit the cockpit; they are not co-equal with the core triad.
> Added after M3 proves the core; never part of MVP scope.

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Slack panel: browse channels, read threads, send messages | P1 | post-MVP | "Dead-time" while agent runs; orbit layer |
| Gmail panel: inbox view, read + reply to threads | P2 | post-MVP | Second comms orbit panel |
| Comms panel toggled on/off per window layout | P1 | post-MVP | User controls real estate |
| Notification bridge: agent `AwaitingInput` collapses comms panel, focuses terminal | P2 | — | Attention management between orbit and core |

---

## 16. Agent Configuration & Plugins

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Built-in Claude Code adapter | P0 | M2 | First-class status detection + cost parsing |
| Generic adapter (any CLI agent, idle-timeout heuristic) | P0 | M2 | Safety net for unknown agents |
| Declarative `agent.toml` manifest (Phase 2 of agent abstraction) | P1 | M5+ | Register custom agents without recompiling |
| WASM/plugin agent runners (Phase 3) | P2 | — | Full custom logic without a fork |
| Built-in Cursor CLI adapter | P1 | — | Second first-class agent; milestone TBD |
| Built-in Aider adapter | P1 | — | Third first-class agent; milestone TBD |

---

## 17. Multi-Agent Compare

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Run same task on N agents simultaneously (forked sessions) | P2 | — | Different agents, same prompt, compare outputs |
| Side-by-side DiffExplanation comparison of N session outputs | P2 | — | Choose the best agent result |
| Aggregate cost comparison across forked sessions | P2 | — | "Claude Code: $0.12, Aider: $0.04" |

---

## 18. Remote / SSH Sessions (P2)

| Feature | Priority | Milestone | Notes |
|---|---|---|---|
| Spawn agent sessions on a remote host via SSH | P2 | — | PTY forwarded over SSH tunnel |
| Remote Workspace + Worktree management | P2 | — | git operations on remote |
| VibeLens for remote sessions | P2 | — | Diff streamed from remote to local explainer |

> ❓ OPEN: Remote sessions change the PTY adapter significantly. Scope only after local
> multi-session is stable. No milestone assigned yet.

---

## Backlog / Ideas Parking Lot

> Items surfaced during brainstorming that have no priority or milestone yet. Not
> committed — here for later discussion.

- **Session sharing / co-pilot mode**: two users observe the same session live (pair
  supervision rather than pair programming).
- **Agent "briefing" input**: pre-fill the agent's first message from a template before
  spawning, so session setup is one step.
- **Session recording / replay**: record a full session (PTY output + VibeLens states)
  for asynchronous review or training.
- **Workspace-level token budget**: hard cap across all sessions on a Workspace; refuse
  to spawn new sessions once exceeded.
- **Auto-Checkpoint trigger**: take a Checkpoint automatically before any agent action
  that touches more than N files (requires agent signal).
- **Light/dark theme + custom color schemes**: cosmetic but often requested.
- **Multiple monitor / multi-Window layout**: span sessions across physical screens.
