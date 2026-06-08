# Roadmap

> Milestones M0–M5 build one complete agent session end-to-end, then generalize and add
> the supervision surface. Each milestone is a vertical slice — a user can do something
> real with it. See [Features](features.md) for the full feature catalog and
> [Vision](vision.md) for the north-star experience.

**Philosophy:** M0–M3 prove the single-session core triad (Spec → Diff → Why). M4
generalizes to parallel sessions. M5 adds the supervision surface that makes parallel
sessions manageable. Post-MVP adds comms panels and depth.

Build-on-stack principle: **reuse before build**. Engram is wired in M0, not invented
later. The Provisioner ships in M2 alongside agent spawn. VibeLens (already exists) is
promoted to first-class in M3. No new storage layers are built from scratch.

---

## M0 — Scaffold + Engram Wiring

**Goal:** A runnable Tauri + Rust + React app that proves the stack wires together
end-to-end, with `PersistencePort` backed by engram from day one — not bolted on later.

### Scope
- Tauri project structure (Rust workspace + React frontend).
- Hexagonal architecture skeleton: `core/`, `adapters/`, `ports/` modules with placeholder types.
- `Session`, `Workspace`, `AgentStatus` types defined (no behavior yet).
- **`PersistencePort` defined in Core; engram adapter implements it.** Port is wired from
  this milestone — the Core never imports engram directly.
- One Tauri command (`ping`) and one event (`pong`) flowing through the Bridge — proves
  the communication channel.
- CI: build + format + clippy + vitest passing.
- Development tooling: hot-reload, cargo-watch, project conventions documented.

### Exit criteria
- `cargo build` and `npm run dev` succeed from a clean clone.
- The `ping → pong` round-trip works in the running app (visible in the web console).
- No architecture violations: `core` imports nothing from `adapters`, `tauri`, `engram`,
  or any external agent/tool crate.
- A `PersistencePort::write` / `PersistencePort::read` round-trip passes in a unit test
  (in-memory stub adapter acceptable).

### What it proves
The stack is wired. The hexagonal module boundaries are enforced from day one —
including the engram quarantine. New contributors can set up and build in < 30 minutes.

---

## M1 — Live PTY Terminal

**Goal:** A real terminal in the UI, backed by a real PTY, rendering live output correctly.

### Scope
- `PtyAdapter` wrapping `portable-pty`: spawn a shell process, read/write PTY, handle resize.
- xterm.js rendered in a React Pane, connected to the PTY via Tauri events.
- Terminal resize: SIGWINCH propagated when the Pane resizes.
- Scrollback buffer (configurable length).
- Keyboard input forwarded from xterm.js to PTY.
- Copy/paste via system clipboard.
- Colors and ANSI escape codes render correctly.

### Exit criteria
- Open a shell in the Pane; run `vim`, `htop`, `git log --oneline --graph` — each
  renders and behaves correctly.
- Resize the window; the PTY and rendering track the new size.
- Scrollback is retained after the shell produces more output than fits on screen.

### What it proves
The PTY adapter works. xterm.js integration is correct. The Bridge can carry high-
frequency I/O without jank. The baseline terminal is solid enough to build agent sessions on.

---

## M2 — Spawn Agent + Provisioner

**Goal:** Launch a real AI CLI agent inside the PTY, detect its lifecycle state, and
**inject the Spectty Agent Protocol** into the agent's config via the Provisioner.

### Scope
- `AgentRunner` port and trait defined; first implementation: Claude Code adapter.
- Generic adapter (any CLI command; idle-timeout status detection).
- `AgentStatus` state machine: Starting → Idle → Running → AwaitingInput / Completed / Error.
- Status detector reads `OutputSignal` from PTY and transitions state via the adapter.
- Session entity fully wired: `SessionId`, `WorkspaceId`, `AgentSpec`, `AgentStatus`,
  `CostMetrics` skeleton.
- `SessionRegistry` in Core: create, look up, close sessions.
- **`ProvisioningPort` + Provisioner adapter**: on session create, writes MCP tool
  registrations (`spectty_spec`, `spectty_diff`, `spectty_approval`) into the agent's
  config using managed-section markers, atomic writes, backup-before-write. GLOBAL scope
  by default; PROJECT scope when the file is committed.
- Provisioner teardown: restore agent config on session close.
- UI: spawn a session (pick agent + workspace directory); Session status indicator in
  Pane header.
- UI: named session title displayed.

### Exit criteria
- Spawn a Claude Code session on a local git repo; it launches and reaches `Idle`.
- Inspect the Claude Code config — Spectty's managed section with MCP tools is present.
- Give it a task; status transitions to `Running`, then `AwaitingInput` when it hits a
  permission prompt, then back to `Running` after input is given.
- Close the session; the PTY process terminates; the managed section is removed from the
  agent config.
- Generic adapter: spawn `bash`; status reaches `Idle`; idle-timeout transitions to
  `Completed` after inactivity (configurable).

### What it proves
Agent supervision works. The `AgentRunner` port is the right abstraction. The Provisioner
correctly injects and tears down the protocol. A second agent (Generic) works without
changing the Core.

---

## M3 — Living Spec Pane + VibeLens (The Triad)

**Goal:** The core triad is fully functional in a single session: SPEC (living contract
with plan-approval gate and live progress) → DIFF (VibeLens) → WHY (per-file rationale).
This is the first user-demoable moment and Spectty's signature.

### Scope
- **Spec pane**: dev seeds intent → agent calls `spectty_spec` to submit a structured
  plan (proposal + tasks with states) → Spectty shows the plan as a live checklist →
  **plan-approval gate fires before any code is edited** → user approves or steers.
- Live task progress: task states transition done / in-progress / pending as the agent
  calls `spectty_spec` updates; PTY-scraping fallback for Generic agents.
- Mid-flight steering: dev adjusts the plan while the agent is running.
- Verify step: agent signals completion; Spectty confirms task states vs. original plan.
- **PersistencePort** wired to engram: spec + progress stored under
  `spectty/{session-id}/spec` and `spectty/{session-id}/progress` topic_keys; survives
  Spectty restarts.
- **Real-time event layer**: Spectty polls/subscribes over engram to re-render the Spec
  pane live (engram is a store, not pub/sub — this polling layer is Spectty's addition).
- **VibeLens panel**: promoted from M3 plan.
  - `FileWatchPort` adapter (`notify` crate): watch Session's Workspace / Worktree,
    debounced.
  - `DiffExplainerPort` wired to VibeLens MCP client: given a git diff → `DiffExplanation`.
  - `DiffExplanation` stored on Session aggregate; emitted as Tauri event when updated.
  - Cooperative path: agent calls `spectty_diff` → immediate trigger (richer than
    file-watching heuristic).
  - Generic fallback: FileWatcher triggers diff.
  - UI: VibeLens panel alongside terminal — one-paragraph summary + per-file rationale.
  - Manual refresh trigger.
- Triad layout: Spec pane + Terminal + VibeLens panel, all visible at once per session.

### Exit criteria
- Spawn a Claude Code session (Cooperative tier); seed intent in the Spec pane.
- Agent submits a plan via `spectty_spec`; plan-approval gate appears; user approves.
- Agent begins work; task states update live in the Spec pane (no manual refresh).
- Agent edits 3 files; VibeLens panel updates within seconds via `spectty_diff` signal.
- Per-file rationale is accurate and human-readable.
- Restart Spectty mid-session; spec + progress are restored from engram.
- Generic agent (no injection): PTY-scraping drives approximate Spec state; FileWatcher
  drives VibeLens — both degrade gracefully.

### What it proves
The core triad works end-to-end. Plan-approval gate prevents runaway agent work. The
real-time event layer over engram is validated. Persistent spec + progress survive
restarts. Spectty is now a demoable cockpit, not just a terminal wrapper.

---

## M4 — Multi-Session + Worktree Isolation

**Goal:** Run multiple agent sessions in parallel, each in its own git worktree, without
collisions, navigated from a unified session switcher.

### Scope
- `GitPort` adapter: `git worktree add`, `git worktree remove`, branch creation/deletion, merge.
- Session creation flow: optionally create a Worktree (new branch + working copy) for the session.
- `SessionRegistry` extended: multiple sessions per Workspace, each with its own Worktree.
- Worktree cleanup on session close (remove worktree, optionally delete branch).
- Review-then-merge flow: explicit user approve → merge worktree branch; or discard → delete worktree.
- UI: multiple Panes / Tabs; each Pane bound to one Session.
- Command palette: keyboard-first session creation, navigation, merge, discard.
- Session list / switcher: all sessions visible, navigate by name.

### Exit criteria
- Open 3 sessions on the same Workspace, each in its own Worktree.
- Each agent modifies files; no session sees another's uncommitted changes.
- Complete the review-then-merge flow on one session: inspect DiffExplanation → approve → merge → branch deleted.
- Discard flow on another session: reject → worktree and branch removed cleanly.
- All sessions accessible via Tab navigation and the command palette.

### What it proves
Parallel sessions are safe. The Worktree isolation model works. The review-then-merge
flow is the complete user journey from "agent starts work" to "code lands on main."
The triad (Spec → Diff → Why) works correctly across multiple concurrent sessions.

---

## M5 — Dashboard + Cost + Notifications

**Goal:** A supervision surface that makes running 3–5 agents manageable — you never miss
a blocked or failed agent. Cost is visible in context. The full MVP is complete.

### Scope
- Dashboard view: all active sessions, their `AgentStatus`, `Workspace`, and `CostMetrics`
  in one screen.
- Filter / sort sessions by status (e.g., "show AwaitingInput first").
- Click / jump from Dashboard to the session's Pane.
- `AwaitingInput` and `Error` sessions surface loudly: pulsing indicator in navigation +
  Dashboard.
- `NotifierPort` adapter: OS notifications on `AwaitingInput` and `Error` transitions
  (one per transition; no spam).
- CostMetrics: live per-session token usage + estimated USD in Pane header and Dashboard.
  - Fed via `spectty_cost` tool (Cooperative) or parsed from PTY output (Generic fallback).
  - Degrades gracefully to "n/a" when agent does not report cost.
- `AwaitingInput` and `Error` in Spec pane: surfaced inline alongside live task progress.

### Exit criteria
- Run 3 sessions; one enters `AwaitingInput`: OS notification fires; Dashboard shows it
  prominently; indicator pulses in the session Tab.
- Session enters `Error`: same notification + surface behavior.
- Dashboard cost column shows live-updating USD estimates for Claude Code sessions; "n/a"
  for Generic.
- Navigating from a Dashboard row to the session's Pane works in one click/keystroke.

### What it proves
The supervision surface is complete. A user can comfortably run 3–5 agents in parallel —
the north-star scenario from [Vision](vision.md) — without losing track of any session.
M0–M5 together deliver the full MVP.

---

## Post-MVP Horizon

Features with clear value that are intentionally deferred to maintain MVP focus.
Not ordered by priority — that is a post-M5 conversation.

| Area | What | Why deferred |
|---|---|---|
| **Comms panels** | Slack + Gmail "dead-time" panels while agent runs | Core triad proven first; comms orbit the cockpit but are not the core |
| **CostMetrics depth** | Budget alerts, session cost reset, aggregate daily spend | Core tracking works in M5; alerts and reporting are additive |
| **Checkpoints & undo** | Snapshot worktree state before risky work; one-click rollback | Needs Checkpoint storage decision (open question); not blocking the core loop |
| **Declarative agent manifests** | `agent.toml` to register custom agents without recompiling | Generic adapter covers P3 users in MVP; manifests are quality-of-life |
| **Multi-agent compare** | Run same task on N agents; compare DiffExplanations side-by-side | Complex UI + routing; valuable but not core to the supervision story |
| **Session templates** | Save + reuse session configs (agent + workspace + task) | Session creation UX is adequate in MVP; templates are convenience |
| **Layout presets** | Named Pane layouts switchable via command palette | Pane splits work in M4; named presets are ergonomic polish |
| **Remote / SSH sessions** | Spawn agent sessions on a remote host | Major PTY adapter change; scope only after local multi-session is stable |
| **Plugin agent runners** | WASM/plugin runners for agents needing real custom logic | Phase 2 (`agent.toml`) covers 80% of cases; WASM is Phase 3 |
| **Notification depth** | Webhook / Slack routing; quiet hours; notification history | OS notifications are sufficient for solo-first audience in MVP |
| **Search in scrollback** | Regex or literal search in terminal scrollback buffer | Useful but not supervision-critical |
| **Session recording / replay** | Record full PTY + VibeLens state for async review | Significant storage and UX design work |
| **Team / shared Dashboard** | Multi-user session visibility (P2 persona) | Requires backend sync layer; solo-first audience is M0–M5 |
| **Historical spec view** | Browse prior sessions' living contracts | Needs archive integration with PersistencePort |
