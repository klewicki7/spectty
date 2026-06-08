# Data Flow — The Bridge Contract

The Bridge is the boundary between the Rust backend and the web UI. UI → backend via
Tauri **commands** (request/response); backend → UI via Tauri **events** (push, no
response). See [overview.md](overview.md) for the architectural principle: the backend is
the **single source of truth**; the UI is **event-driven and stateless about domain rules**.

The UI holds no business logic. It renders what the backend pushes and forwards user
intent as commands. A correct UI can be rebuilt entirely from the stream of events it has
received since startup.

This document also covers the **persistence flow** (how data reaches and leaves
`PersistencePort` / engram) and the **event-stream gap** — the fact that engram is a store
with no native pub/sub, and how Spectty bridges that gap with a polling layer.

Related: [agent-protocol.md](agent-protocol.md) · [stack-integration.md](stack-integration.md)

---

## Persistence flows (backend ↔ engram)

All persistence goes through `PersistencePort`. The concrete adapter
(`EngramAdapter`) calls engram's local HTTP API on `:7437`. No UI layer
touches engram directly — persistence is entirely a backend concern.

### Write path

Any backend operation that needs to persist state calls
`PersistencePort::upsert(topic_key, payload)`. The adapter serializes the
payload and POSTs to engram's REST endpoint. This happens synchronously within
the originating async task before the result is returned to the caller.

Write triggers:

| Trigger | Topic key written |
|---|---|
| `spawn_session` succeeds | `spectty/sessions/{session_id}` |
| `spectty_spec` MCP tool call | `spectty/specs/{session_id}` |
| `spectty_cost` MCP tool call | `spectty/cost/{session_id}` |
| `create_checkpoint` command | `spectty/checkpoints/{session_id}/{checkpoint_id}` |
| Provisioner writes agent config | `spectty/provisioner/{agent}/{scope}` |

### Read path

Reads happen at session start (to restore a prior session's Spec and cost if
it was interrupted) and on demand (e.g. `get_diff_explanation` command). All
reads go through `PersistencePort::get(topic_key)`.

### The event-stream gap and the polling layer

engram has no native pub/sub. When an agent writes to
`spectty/specs/{session_id}` (via `spectty_spec`), nothing in engram notifies
the Spectty backend. Spectty bridges this with a **polling loop** inside
`EngramAdapter`:

```
EngramAdapter (per session)
  └── Tokio task: poll loop at 2 s (configurable)
        ├── GET :7437/api/observations?topic_key=spectty/specs/{id}&since={ts}
        ├── if updated_at changed → emit internal broadcast
        └── backend event handler → emit Tauri `spec_updated` event → Spec pane
```

The `PersistencePort::subscribe` method hides this; callers register a
callback and receive change notifications without knowing they come from
polling. If engram gains native SSE support, the adapter can switch without
touching any code above the port.

> ❓ OPEN: The 2 s poll interval may feel sluggish with many sessions open.
> Evaluate adaptive polling (tighter when session is actively Running, looser
> when Idle) or a long-poll variant if engram supports it.

---

## Tauri Commands (UI → backend)

All commands are async. Errors return a structured `AppError` with a `code` and
`message`. Params and responses are serialized as JSON via `serde`.

| Command | Params | Response | Description |
|---|---|---|---|
| `spawn_session` | `workspace_path: String`, `agent: AgentSpec`, `title: String`, `isolated: bool` | `SessionId` | Creates a Session, optionally creates a Worktree, spawns the agent in a PTY. |
| `send_input` | `session_id: SessionId`, `data: Vec<u8>` | `()` | Forwards raw bytes (keystrokes) from xterm.js to the Session's PTY write path. |
| `resize_pty` | `session_id: SessionId`, `cols: u16`, `rows: u16` | `()` | Reports new terminal dimensions (from xterm-addon-fit) to the PTY adapter. |
| `approve_prompt` | `session_id: SessionId`, `action_id: String` | `()` | Sends the agent's approved quick action (e.g. `"y\n"`) via `AgentRunner::quick_actions`. |
| `merge_session` | `session_id: SessionId` | `MergeResult` | Merges the Session's worktree branch into the main checkout and removes the worktree. |
| `close_session` | `session_id: SessionId`, `discard: bool` | `()` | Terminates the agent process. If `discard: true`, removes the worktree without merging. |
| `list_sessions` | *(none)* | `Vec<SessionSummary>` | Returns all active Sessions with their current status and cost — powers the Dashboard. |
| `create_checkpoint` | `session_id: SessionId`, `label: String` | `CheckpointId` | Saves a Checkpoint on the Session's worktree branch before risky work. |
| `restore_checkpoint` | `session_id: SessionId`, `checkpoint_id: CheckpointId` | `()` | Rolls back the worktree to the named Checkpoint. |
| `get_diff_explanation` | `session_id: SessionId` | `Option<DiffExplanation>` | Returns the Session's current `last_diff` on demand (for initial panel load). |
| `get_spec` | `session_id: SessionId` | `Option<Spec>` | Returns the Session's current Spec JSON on demand (for initial Spec pane load). |

> ❓ OPEN: `approve_prompt` uses an `action_id` tied to the last detected `AwaitingInput`
> transition. Define the action ID lifecycle (generated at detection time, invalidated on
> next status transition) before implementation.

---

## Tauri Events (backend → UI)

Events are fire-and-forget. The UI subscribes on startup and updates its local view state
on each event. There is no acknowledgment.

All events carry a `session_id` (except `app_error`) so the UI can route them to the
correct Pane.

| Event | Payload | Description |
|---|---|---|
| `pty_output` | `session_id: SessionId`, `data: Vec<u8>` | Raw PTY bytes; xterm.js writes them directly. Sent at a rate-limited cadence (see [pty-layer.md](pty-layer.md)). |
| `status_changed` | `session_id: SessionId`, `status: AgentStatus`, `quick_actions: Vec<QuickAction>` | Fires on every AgentStatus transition. `quick_actions` is non-empty when `status == AwaitingInput`. |
| `diff_updated` | `session_id: SessionId`, `explanation: DiffExplanation` | Fires when a new DiffExplanation is available. UI updates the VibeLens panel. |
| `cost_updated` | `session_id: SessionId`, `metrics: CostMetrics` | Fires when the cost parser extracts a new `CostDelta` from agent output. |
| `session_notification` | `session_id: SessionId`, `kind: NotificationKind`, `message: String` | Mirrors OS notifications to the in-app Dashboard for completeness. `NotificationKind`: `AwaitingInput` \| `Error` \| `Completed`. |
| `session_created` | `session: SessionSummary` | Fires when a new Session is fully started (agent spawned, PTY ready). |
| `session_closed` | `session_id: SessionId` | Fires when a Session is fully cleaned up. UI removes the corresponding Pane. |
| `spec_updated` | `session_id: SessionId`, `spec: Spec` | Fires when the Spec pane content changes (agent called `spectty_spec`, poll loop detected a new version in engram). Spec pane re-renders. |
| `app_error` | `code: String`, `message: String` | Non-session-scoped errors (e.g. workspace not found). |

---

## Sequence diagrams

### (a) Spawning a Session and seeing first output

```
UI                          Backend (Rust)                      PTY / Agent
│                                │                                    │
│── spawn_session(workspace,     │                                    │
│     agent, title, isolated) ──▶│                                    │
│                                │── GitPort::create_worktree() ─────▶│ (if isolated)
│                                │── PtyAdapter::spawn(LaunchSpec) ──▶│
│                                │                                    │── agent starts
│◀── Ok(session_id) ────────────│                                    │
│                                │◀── pty output bytes ───────────────│
│◀── event: session_created ────│                                    │
│   (SessionSummary)             │                                    │
│◀── event: pty_output ─────────│                                    │
│   (raw bytes → xterm.js)       │                                    │
│                                │── OutputSignal → detect_status() ──┘
│                                │── status: Starting → Idle
│◀── event: status_changed ─────│
│   (status: Idle)               │
```

The UI renders the terminal immediately on the first `pty_output` event. The status badge
updates separately when `status_changed` fires — the two are independent streams.

---

### (b) Agent edits files → VibeLens panel updates

```
UI                          Backend (Rust)                   FileWatcher / Git / MCP
│                                │                                    │
│  [agent running in worktree]   │                                    │
│                                │◀── FileChanged { path } ──────────│ (notify crate)
│                                │   [debounce 500 ms]               │
│                                │── GitPort::diff_head() ───────────▶│
│                                │◀── unified diff string ───────────│
│                                │   [dedup: hash unchanged? skip]   │
│                                │── DiffExplainerPort::explain() ───▶│ (MCP call)
│                                │◀── DiffExplanation ───────────────│
│                                │── Session::update_diff()           │
│◀── event: diff_updated ───────│                                    │
│   (DiffExplanation)            │                                    │
│  VibeLens panel re-renders     │                                    │
```

The agent's terminal output (`pty_output`) and the VibeLens update (`diff_updated`) are
completely independent streams. A slow MCP call does not block PTY rendering.

---

### (c) Agent hits a prompt → `AwaitingInput` → notification → user approves

```
UI                          Backend (Rust)                  OS / Agent
│                                │                               │
│  [agent running]               │◀── pty output bytes ─────────│
│                                │── OutputSignal → detect_status()
│                                │   pattern match: "Do you want to…"
│                                │── status: Running → AwaitingInput
│◀── event: status_changed ─────│                               │
│   (status: AwaitingInput,      │── NotifierPort::notify() ────▶│ OS notification
│    quick_actions: ["Approve",  │                               │
│                   "Deny"])     │                               │
│  Pane badge pulses             │                               │
│  Dashboard highlights Session  │                               │
│                                │                               │
│── approve_prompt(session_id,   │                               │
│    action_id: "approve") ─────▶│                               │
│                                │── AgentRunner::quick_actions()
│                                │   → sends "y\n" to PTY write path
│                                │                    ──────────▶│ agent receives input
│                                │◀── pty output bytes ─────────│ agent resumes
│                                │── status: AwaitingInput → Running
│◀── event: status_changed ─────│                               │
│   (status: Running)            │                               │
│  Pane badge clears             │                               │
```

The backend generates the `action_id` at the time of the `AwaitingInput` transition and
includes it in the `quick_actions` list carried by `status_changed`. The UI echoes it
back in `approve_prompt`. The backend validates that the action ID is still current
(i.e. the Session is still in `AwaitingInput`) before writing to the PTY.

---

### (d) Agent calls `spectty_spec` → Spec pane updates (Cooperative tier)

This is the canonical path for live Spec pane updates when the agent speaks the
Spectty Agent Protocol. See [agent-protocol.md](agent-protocol.md) for tool schemas and
[stack-integration.md](stack-integration.md) for the engram polling gap analysis.

```
Agent (MCP client)       Backend (Rust)             engram (:7437)          UI
│                             │                           │                   │
│── spectty_spec(session_id,  │                           │                   │
│    spec: { tasks: [...] })─▶│                           │                   │
│                             │── PersistencePort::upsert │                   │
│                             │   ("spectty/specs/{id}",  │                   │
│                             │    spec_payload) ─────────▶                   │
│                             │                           │── SQLite upsert   │
│                             │◀── Ok ────────────────────│                   │
│◀── tool result: Ok ────────│                           │                   │
│  (agent unblocks)           │                           │                   │
│                             │  [poll loop, ~2 s later]  │                   │
│                             │── GET /observations?      │                   │
│                             │   topic_key=spectty/specs/│                   │
│                             │   {id}&since={last_ts} ──▶│                   │
│                             │◀── {updated_at: newer} ──│                   │
│                             │   [change detected]       │                   │
│                             │── PersistencePort::get ──▶│                   │
│                             │◀── spec_payload ──────────│                   │
│                             │── emit Tauri event        │                   │
│                             │   `spec_updated`          │                   │
│                             │   (session_id, spec) ─────────────────────────▶
│                             │                           │   Spec pane       │
│                             │                           │   re-renders      │
```

Notes:
- The MCP call returns immediately after the engram upsert; the agent is not
  blocked waiting for the UI to update.
- The poll loop latency (≤2 s default) is the only lag between the agent's
  write and the UI refresh. This is acceptable for task-level granularity.
- If the poll detects no change (`updated_at` unchanged), no event is emitted
  and no UI work occurs.

---

### (e) Structured approval via `spectty_approval` (Cooperative tier)

```
Agent (MCP client)       Backend (Rust)                          UI
│                             │                                   │
│── spectty_approval(         │                                   │
│    session_id,              │                                   │
│    action_id: "drop-table", │                                   │
│    description: "DROP TABLE │                                   │
│      users — irreversible", │                                   │
│    risk_level: "high",      │                                   │
│    options: ["approve",     │                                   │
│              "deny"]) ─────▶│                                   │
│  [MCP call BLOCKS agent]    │── AgentStatus → AwaitingInput     │
│                             │── NotifierPort::notify()          │
│                             │── emit `status_changed`           │
│                             │   (status: AwaitingInput,         │
│                             │    quick_actions: ["approve",     │
│                             │                   "deny"]) ───────▶
│                             │                                   │ Pane badge pulses
│                             │                                   │ Dashboard highlights
│                             │                                   │
│                             │◀── approve_prompt(session_id,     │
│                             │     action_id: "drop-table",      │
│                             │     choice: "deny") ──────────────│
│                             │── resolve pending MCP future      │
│◀── tool result: "deny" ────│                                   │
│  (agent receives choice)    │── AgentStatus → Running           │
│  (agent skips drop)         │── emit `status_changed`           │
│                             │   (status: Running) ──────────────▶
│                             │                                   │ Pane badge clears
```

---

## Design principles enforced by this contract

1. **Backend is authoritative.** The UI never transitions `AgentStatus` locally. Status
   only changes when a `status_changed` event arrives. A user action that does not produce
   a backend event produces no UI change.

2. **UI is rebuildable.** Given the full event stream from `session_created` onward, the
   UI can reconstruct its complete state. This enables hot-reload in development without
   losing session state.

3. **Events are scoped.** Every event carries `session_id`. The UI router delivers events
   to the correct Pane component without broadcasting to unrelated state.

4. **Commands are thin.** Commands carry intent and minimal data. They do not carry
   pre-computed state. The backend validates and computes; the UI provides input.

5. **PTY bytes are never transformed in transit.** `pty_output` carries raw bytes from the
   PTY master. xterm.js receives them unmodified. ANSI interpretation for domain purposes
   happens in the Rust adapter (see [pty-layer.md](pty-layer.md)), never in the UI.

6. **Persistence is always behind `PersistencePort`.** No code above the adapter layer
   calls engram's HTTP API directly. The port contract allows swapping engram for any
   other store without touching the Core or the Tauri bridge.

7. **The poll loop is the publisher; the port is the abstraction.** The UI never polls
   engram. It receives `spec_updated` events just like any other Tauri event. The fact
   that those events originate from a poll loop (rather than a push notification) is an
   adapter implementation detail, invisible above `PersistencePort::subscribe`.
