# Domain Model

The Core's entities and the rules that bind them. This is pure Rust — no I/O types leak
in. Types below are illustrative (shape, not final signatures).

## Entity map

```
Workspace 1 ──< many >── Session ──1── PTY (adapter handle)
    │                        │
    │                        ├──1── Worktree        (optional isolation)
    │                        ├──1── AgentStatus      (state machine)
    │                        ├──0..1── Spec          (living contract, plan + progress)
    │                        ├──1── CostMetrics
    │                        ├──0..1── DiffExplanation (latest VibeLens result)
    │                        └──*── Checkpoint
    └──1── git repo root

UI layout:  Workspace ──*── Panel  (Terminal | Diff | Spec | Git | Comms)
```

## Entities

### Session — the aggregate root
The central entity. Owns its lifecycle and is the only thing the UI binds a Pane to.

```rust
struct Session {
    id: SessionId,
    workspace: WorkspaceId,
    worktree: Option<Worktree>,
    agent: AgentSpec,          // which agent, how to run it (see agent-abstraction)
    status: AgentStatus,       // state machine, derived from PTY observation
    spec: Option<Spec>,        // living contract — present when agent is task-directed
    cost: CostMetrics,
    last_diff: Option<DiffExplanation>,
    created_at: Timestamp,
    title: String,             // user- or task-derived label
}
```

Invariants:
- A Session always has a Workspace.
- A Session in `AwaitingInput` MUST raise a notification exactly once per transition.
- A Session's Worktree, if present, belongs to the Session's Workspace.
- A Session's `Spec`, if present, is the authoritative source of task progress for that
  Session. Progress is **structured** (machine-readable `TaskState`) — never narrative text.

### Spec — the living contract

The Spec is the SOUL of Spectty's cockpit concept. It captures the *intent* the developer
seeds, the *plan* the agent proposes, live *progress* (structured, not narrative), and an
*approval-gate* that must be cleared before the agent writes code. The data model is
generalized from SDD (proposal → spec → tasks → apply-progress → verify).

```rust
struct Spec {
    id: SpecId,
    session: SessionId,
    intent: String,              // developer-authored goal / prompt seed
    plan_summary: Option<String>, // agent-generated plan overview
    tasks: Vec<Task>,            // structured task list with live state
    approval_gate: ApprovalState, // Pending | Approved | Rejected
    created_at: Timestamp,
    updated_at: Timestamp,
}

struct Task {
    id: TaskId,
    title: String,
    description: Option<String>,
    state: TaskState,
    subtasks: Vec<Task>,
}

enum TaskState {
    Pending,
    InProgress,
    Done,
    Blocked { reason: String },
}

enum ApprovalState {
    /// Agent has produced a plan; awaiting human review.
    Pending,
    /// Human approved — agent may proceed to write code.
    Approved,
    /// Human rejected — agent must revise before re-proposing.
    Rejected,
}
```

Key rules:
- The agent MUST NOT write code until `approval_gate == Approved` (enforced by the
  `spectty_approval` MCP tool for Cooperative agents; by an explicit UI confirmation
  for Generic agents when the feature is available).
- `tasks` is the **structured progress model** — the UI renders Done/InProgress/Pending
  badges; never parses natural-language output for progress.
- Spec is persisted via `PersistencePort` so it survives agent crashes and session reconnects.
- See [docs/product/spec-pane.md](../product/spec-pane.md) for the full product specification.

### Panel — UI layout intent

`Panel` is a Core concept (not just a UI concern) because the backend must know which
data streams to activate per panel type. The Core maintains which panels are open per
Workspace and fans out the right events.

```rust
enum PanelKind {
    Terminal { session: SessionId },
    Diff     { session: SessionId },
    Spec     { session: SessionId },
    Git      { workspace: WorkspaceId },
    Comms,                             // post-MVP: notifications, cost overview
}

struct Panel {
    id: PanelId,
    kind: PanelKind,
    position: PanelPosition,          // layout coordinates, opaque to Core logic
}
```

### Workspace
A git repository the user works in. Holds the canonical branch list and is the parent of
all worktrees spawned from it.

### Worktree
Isolation unit. Wraps a git worktree path + branch name. Created on session start (when
isolation is on), removed on session close/merge. See
[Session & Worktree Model](session-worktree-model.md).

### AgentStatus — state machine
The heart of "supervision over execution". Transitions are driven by the status detector
reading PTY output and process signals (or `spectty_status` MCP signals for Cooperative
agents — these take precedence over PTY heuristics).

```
Starting ──ready──▶ Idle ──task──▶ Running ──┬──prompt──▶ AwaitingInput
                     ▲                        │                 │
                     └──────input given───────┴─────────────────┘
                                              │
                                  done ──▶ Completed
                                  exit≠0 / crash ──▶ Error
```

Rules:
- `AwaitingInput` and `Error` are the two "needs human" states → both notify + surface
  on the Dashboard.
- The detector must be **per-agent** (each agent signals "waiting" differently) → it is a
  responsibility delegated through the [Agent Abstraction](agent-abstraction.md), not a
  giant hardcoded regex.

### DiffExplanation — the VibeLens model
```rust
struct DiffExplanation {
    generated_at: Timestamp,
    files: Vec<FileChange>,
    summary: String,                 // one-paragraph "what happened"
}
struct FileChange {
    path: PathBuf,
    added: u32,
    removed: u32,
    kind: ChangeKind,                // Added | Modified | Deleted | Renamed
    rationale: String,               // AI: WHY this changed
}
```
Built by the `DiffExplainerPort` from a git diff. Today the implementation calls the
VibeLens MCP; tomorrow it could be a local model. The Core does not care.

### CostMetrics
```rust
struct CostMetrics { input_tokens: u64, output_tokens: u64, estimated_usd: f64 }
```
Accumulated per Session. Source is agent-specific (parsed from agent output or its logs)
and therefore arrives through the `AgentRunner` port (`parse_cost`) or the `spectty_cost`
MCP tool for Cooperative agents.

### Checkpoint
A labeled snapshot (commit/stash/worktree ref) taken before risky agent work, enabling
one-click rollback.

## Ports the Core depends on

| Port | Responsibility | Primary adapter |
|---|---|---|
| `AgentRunner` | spawn/observe an agent, expose status signals & cost, declare tier | per-agent adapter |
| `GitPort` | worktrees, diffs, branches, merge | `git2` / git CLI |
| `FileWatchPort` | notify on workspace file changes (debounced) | `notify` crate |
| `DiffExplainerPort` | turn a diff into a `DiffExplanation` | VibeLens MCP client |
| `NotifierPort` | raise OS/app notifications | OS notification adapter |
| `ClockPort` | current time (testability) | system clock / fake |
| `PersistencePort` | store/retrieve Sessions, Specs, CostMetrics, Checkpoints | `EngramAdapter` (engram via HTTP `:7437`) |
| `ProvisioningPort` | inject/retract Spectty Agent Protocol into agent configs | `ProvisionerAdapter` (per-agent, per-scope) |

**Persistence note:** Sessions, Specs, cost history, and Checkpoints all persist through
`PersistencePort`. The `EngramAdapter` is the sole concrete implementation in MVP. The
Core never imports `engram` directly — it only calls `PersistencePort` traits.

> ✅ DECIDED: Checkpoints use a DEDICATED COMMIT on the worktree branch (not stash, not a separate ref namespace).

> ✅ DECIDED: If an agent does not report cost, the Dashboard shows "n/a" (graceful degradation); Spectty never guesses.

> ✅ DECIDED: The Panel layout is a TREE OF SPLITS (nested horizontal/vertical splits, tmux/IDE-style), not a fixed grid.
