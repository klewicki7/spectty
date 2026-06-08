# ADR-0007: Living Spec Pane as a Steerable Contract

- Status: Accepted
- Date: 2026-06-07
- Deciders: project owner

## Context

The #1 pain in vibe-coding is **agent drift**: the agent misunderstands the ask, makes
plausible-looking but wrong choices, and you only discover the divergence late — after
minutes or tokens are wasted, or worse, after a bad commit lands.

Spectty's "core triad" is Spec → Diff → Why. The Spec pane is the entry point of that
triad. Three approaches were on the table:

**Option A — Static user brief**: The developer types a brief; the agent reads it.
No progress tracking; no steering once the agent starts.

**Option B — Agent-generated plan only**: The agent produces a plan at session start.
The pane shows it read-only. No human steering; the developer seeds nothing.

**Option C — Living Contract**: The developer seeds intent; the agent produces a plan;
the pane tracks progress live (done / in-progress / pending); the developer can steer
mid-flight; a plan-approval gate exists before the agent edits code.

The question is whether Option C is worth the implementation cost over the simpler
alternatives.

## Decision

The Spec pane is a **Living Contract** (Option C).

### Shape of the contract

```
[Developer seeds intent]
        ↓
[Agent produces plan: list of tasks with estimates]
        ↓
[Plan approval gate — dev reviews before agent touches code]
        ↓
[Agent executes — calls spectty_spec and spectty_status per task]
        ↓
[Pane shows live: ✓ done / ⟳ in-progress / ○ pending]
        ↓
[Dev can inject a steering note mid-flight]
        ↓
[spectty_diff before each file write — dev sees what is about to change]
```

### Data model

The Spec pane's domain type (`SpecArtifact`) is the SDD artifact pipeline generalized:
it has the same Spec → progress → verify shape, but is not coupled to SDD's skill
system. It is Spectty's own type, informed by SDD's proven shape ([ADR-0005](0005-build-on-gentle-ai-stack.md)).

```
SpecArtifact {
  intent: String          // developer-seeded
  tasks: Vec<SpecTask>    // agent-generated at plan time
  progress: Vec<TaskProgress>  // updated live via spectty_* tools
  steering_notes: Vec<Note>    // dev injections mid-flight
}

SpecTask { id, title, estimate }
TaskProgress { task_id, status: Done | InProgress | Pending, timestamp }
```

### Progress source

Progress updates arrive via two paths (priority order):

1. **Structured**: agent calls `spectty_spec` / `spectty_status` — JSON, typed, reliable
   ([ADR-0006](0006-spectty-agent-protocol.md))
2. **Scraping fallback**: `AgentRunner.detect_status` infers coarse state (Working /
   AwaitingInput / Idle) from PTY output — no per-task granularity, but better than
   nothing for Generic-tier agents ([ADR-0004](0004-agent-agnostic-core.md))

The pane renders what it has. If only scraping is available, it shows a coarse status
badge rather than per-task checkboxes.

### Plan approval gate

Before the agent transitions from plan → execution (i.e., before the first file write),
Spectty interrupts and presents the task list for approval. The developer can:

- Approve as-is → agent proceeds
- Edit tasks → agent re-reads the updated plan
- Reject → session ends cleanly

This gate closes the loop on "agent starts doing the wrong thing before I notice."

## Consequences

**Positive**
- Closes the core triad: the Spec pane is the "ask" half; Diff is the "do" half; Why
  completes it. Together they give the developer a full, live picture of what the agent
  is doing and why.
- The plan approval gate catches misunderstandings before they become diffs.
- Mid-flight steering reduces the need to kill and restart sessions.
- The structured data model is testable and serializable — progress can be persisted
  across pauses and resumed.

**Negative**
- Requires the structured agent protocol ([ADR-0006](0006-spectty-agent-protocol.md))
  for full per-task granularity. Generic-tier agents get a coarse status badge, not
  a live checklist.
- The plan approval gate adds a required human interaction before every session —
  experienced users may find it friction for small tasks. A "quick mode" that skips
  approval for tasks flagged as low-risk is a future mitigation.
- The live update layer (pushing `SpecArtifact` changes to the UI as MCP tool calls
  arrive) requires a reactive state channel between the Rust backend and the Tauri
  frontend. Non-trivial to implement correctly under concurrent tool calls.

**Neutral**
- The `SpecArtifact` type is Spectty's own domain object. SDD artifacts from Claude
  Code sessions can be imported/displayed but are not the native format — this avoids
  coupling the Core to the SDD skill system.
- Option C subsumes Option A (the intent seed) and Option B (the agent plan). It is
  strictly more capable, at higher implementation cost.

## Alternatives considered

### Option A — Static user brief

The developer writes a brief; the agent reads it. No plan, no progress, no gate.

**Why not chosen:** Solves the "communicate intent" problem but does nothing about drift
detection. The brief is set-and-forget. By the time the agent produces a diff, the
developer has no intermediate checkpoints. This is what a README comment provides —
not a cockpit feature.

### Option B — Agent-generated plan (read-only)

The agent produces its own plan at session start; the pane shows it. No developer seed;
no steering.

**Why not chosen:** Removes the developer from the loop at the most important moment —
before the agent decides what to do. A plan the agent produces for itself, with no
human review or seed, is just a structured form of the same drift problem. The developer
needs to be the one who defines intent; the agent translates that into an executable plan.
