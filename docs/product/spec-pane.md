# Spec Pane — The Living Contract

> The marquee feature. The center of Spectty's soul.  
> See also [Vision](vision.md) · [Features §3 & §4](features.md) · [Roadmap M3](roadmap.md)
> · [Agent Protocol](../architecture/agent-abstraction.md) · [Domain Model](../architecture/domain-model.md)
> · [VibeLens Integration](../architecture/vibelens-integration.md)

---

## What it is

The Spec pane is a **living contract between developer and agent**. It captures what you
asked, how the agent plans to do it, and the live status of every task in that plan — all
visible and permanent alongside the terminal and the VibeLens diff panel.

It is the SPEC arm of Spectty's **core triad**:

```
SPEC (what I asked + live plan progress)
  → DIFF (what the agent changed — VibeLens)
    → WHY (per-file AI rationale)
```

All three are rendered side-by-side in every session. None of them are modals or views
you navigate to; they are always on, always current, always yours.

---

## Why it exists

Without the Spec pane, a developer directing an AI agent has one source of truth: the PTY
scrollback. They must scroll back to remember what they asked, grep for task boundaries,
and read raw diffs to verify results. The agent is a black box that produces output.

With the Spec pane, the interaction becomes **structured and steerable**:

- The developer's intent is captured, not lost in scrollback.
- The agent's plan is explicit and approved before any code is touched.
- Progress is observable live, not inferred from terminal noise.
- Steering mid-flight is a first-class action, not a workaround.

The Spec pane is Spectty's answer to the question: *what does it mean to direct an AI
agent rather than just run one?*

---

## Data model

The Spec pane's data model is generalized from the SDD artifact pipeline
(proposal → spec → tasks → apply-progress → verify). It is adapted for interactive,
real-time use — no full ceremony required for simple tasks.

```
SpecContract
  ├── intent          String         -- dev's raw intent (free-form or structured)
  ├── proposal        Option<String> -- agent's initial framing before the full plan
  ├── plan            Option<Plan>   -- produced by the agent; gated on approval
  │     └── tasks[]   Vec<Task>
  │           ├── id           TaskId
  │           ├── description  String
  │           ├── state        TaskState  -- Pending | InProgress | Done | Skipped
  │           └── notes        Option<String>
  ├── approval        ApprovalState  -- Pending | Approved | Rejected | Adjusted
  ├── progress        ProgressSummary
  │     ├── done       u32
  │     ├── in_progress u32
  │     └── pending    u32
  ├── verify          Option<VerifyResult>  -- agent's completion signal vs. plan
  └── created_at / updated_at  Timestamp
```

`TaskState` transitions are one-directional in the normal path:
`Pending → InProgress → Done`. The agent can also mark tasks `Skipped` (with a reason).
The developer can override any state mid-flight via the Spec pane UI.

---

## How the agent feeds it — the Protocol

The Spec pane is populated via **two tiers**:

### Tier 1 — Cooperative (structured)

The agent has been provisioned with the Spectty MCP tools by the Provisioner at session
start. It calls:

| Tool | When | What it does |
|---|---|---|
| `spectty_spec` (init) | After receiving intent | Submits proposal + full task list |
| `spectty_approval` | Before editing any code | Blocks until the developer approves |
| `spectty_spec` (update) | As tasks progress | Updates individual task states |
| `spectty_spec` (verify) | On completion | Marks plan complete, sends verify result |

The agent speaks to Spectty **structurally** — via JSON tool calls, not narrative text.
Spectty receives typed updates and renders them directly into the live checklist. No
parsing, no scraping.

### Tier 2 — Generic (scraping fallback)

When injection is not possible (agent does not support MCP, or user opted out of
Provisioner), Spectty falls back to PTY-scraping:

- Heuristic task detection from common agent output patterns.
- Idle-timeout-based state inference.
- Manual task entry: the developer populates the Spec pane directly from the UI.

The Generic tier is a degraded-but-functional fallback. Cooperative agents produce
significantly richer Spec pane data.

---

## The plan-approval gate

Before the agent edits any code, `spectty_approval` is called. The Spec pane transitions
to **Approval Required** state and renders:

- The full task list the agent proposes.
- Estimated scope (number of files, if available).
- Any open questions or assumptions the agent flagged.

The developer must take one of three actions:

| Action | Effect |
|---|---|
| **Approve** | Agent proceeds; tasks begin executing |
| **Reject** | Agent is stopped; session returns to Idle |
| **Adjust** | Developer edits the task list; adjusted plan is confirmed; agent proceeds with the revised plan |

The gate is non-negotiable for Cooperative agents — `spectty_approval` always fires before
code changes. Generic agents get a soft gate: Spectty surfaces a UI prompt based on
heuristic task-start detection; the developer can skip it.

> ❓ OPEN: Should Adjust mode allow free-form edits to the task list, or only pre-defined
> operations (add task, remove task, reorder)? Structured edits are safer but constrain
> steering. Decide before M3 UI design.

---

## Steering mid-flight

After approval, the developer can adjust the plan while the agent is running:

- **Pause + redirect**: send a steering message to the agent via the terminal, then
  update the Spec pane to reflect the new direction.
- **Add a task**: append a new task to the plan; the agent is notified via `spectty_spec`
  if it is Cooperative.
- **Skip a task**: mark a pending task Skipped; Cooperative agents are notified to skip it.
- **Override a task state**: manually mark a task Done if the agent missed the signal.

Mid-flight steering is the difference between a contract that is alive and a plan that is
a historical artifact.

---

## Rendering

The Spec pane renders as a persistent vertical split alongside the terminal and the
VibeLens panel. Layout:

```
┌──────────────────────────────────────────────────────────┐
│  SPEC                │  TERMINAL             │  VIBELENS  │
│  ─────────────────   │                       │  ─────────  │
│  Intent: "..."       │  [PTY output]         │  Summary   │
│                      │                       │            │
│  Plan (approved ✓)   │                       │  file.rs   │
│  ✓ Task 1            │                       │  + 12 - 3  │
│  ⏳ Task 2  ←─ live  │                       │  "Because…" │
│  ○ Task 3            │                       │            │
│  ○ Task 4            │                       │  lib.rs    │
│                      │                       │  + 5 - 1   │
│  3/4 tasks done      │                       │  "Fixed…"  │
└──────────────────────────────────────────────────────────┘
```

The checklist is a **live render** — task states update without any user action. The
developer watches progress the same way they watch a CI pipeline: real output, no polling
by hand.

Spec deltas (when the plan changes mid-flight) are rendered as inline diffs — what was
added or removed from the task list, highlighted. This makes steering transparent.

---

## Persistence via PersistencePort

The entire `SpecContract` is persisted behind `PersistencePort` (engram adapter) under
deterministic topic_keys:

```
spectty/{session-id}/spec          -- intent + proposal + plan (with approval state)
spectty/{session-id}/progress      -- live task states (updated frequently)
spectty/{session-id}/verify        -- verify result (written once, on completion)
```

This means:
- **Spectty restarts** are transparent — open a session, the Spec pane is exactly where
  you left it.
- **Historical contracts** are browsable post-MVP (archive integration needed).
- The spec is readable by other tools (e.g., a CLI query against engram) without
  launching Spectty.

---

## The real-time event layer — the #1 technical problem

Engram is a persistent key-value store, not a pub/sub system. There is no native
"subscribe to topic_key changes" primitive. This is the gap Spectty fills.

For the Spec pane to update live, Spectty implements a polling loop:

```
every 500ms (configurable):
  read progress topic_key from PersistencePort
  compare with last-known state
  if changed → emit Tauri event → React re-renders Spec pane
```

For Cooperative agents, the latency of this loop is acceptable: `spectty_spec` writes the
update; Spectty polls and renders. Round-trip visible latency is under 1 second on a
local machine.

For a future native subscribe primitive (if engram adds one), the polling loop can be
replaced by a push adapter behind the same `PersistencePort` interface — no Core changes
required.

> ❓ OPEN: Is 500 ms polling acceptable for the live checklist UX? Alternative: write a
> small side-channel (e.g., a Tauri channel or IPC pipe) that the Provisioner-injected
> tool calls directly, bypassing engram for the hot path. Evaluate post-M3.

---

## Relation to VibeLens (Diff) and the WHY

The Spec pane is the SPEC arm; VibeLens is the DIFF arm; per-file rationale is the WHY.
They are rendered together and complement each other:

- **Spec → Diff**: when a task transitions to Done, the VibeLens panel shows *what
  changed* in the files the agent touched for that task. The plan and the diff are
  co-located in time.
- **Diff → Why**: every file in the VibeLens panel has an AI-generated rationale
  (`FileChange.rationale`) explaining why *that specific file* changed.
- **Spec → Why**: the developer can cross-reference the task description in the Spec pane
  with the why in VibeLens to verify the agent did what was asked.

The triad is not three separate panels that happen to be on screen — it is a single
coherent answer to the question: *what did the agent do, did it match what I asked, and
why did each piece of code change?*

For detailed VibeLens architecture, see
[VibeLens Integration](../architecture/vibelens-integration.md).
