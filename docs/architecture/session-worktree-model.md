# Session & Worktree Model

This document defines how Sessions achieve parallel isolation, how Worktrees are created
and destroyed, and how Checkpoints enable rollback before risky work. The domain types
(`Session`, `Workspace`, `Worktree`, `Checkpoint`) are defined in
[domain-model.md](domain-model.md). Git operations are performed through `GitPort` — the
Core never calls `git2` or the git CLI directly.

---

## Isolation: Worktree vs. main checkout

When a user creates a Session, Spectty offers two modes:

| Mode | Worktree? | When to use |
|---|---|---|
| **Isolated** (default) | Yes — dedicated git worktree | Running agents in parallel; any non-trivial task |
| **Direct** | No — agent runs in the main checkout | Quick single-agent tasks; exploratory runs where isolation is not needed |

**Default is Isolated.** The product's parallel-agent value proposition depends on agents
not stepping on each other. Direct mode is an opt-in escape hatch.

---

## Branch naming scheme

Each isolated Session gets a branch named:

```
spectty/<session-slug>
```

Where `<session-slug>` is derived from the Session's title, lowercased, with spaces and
non-alphanumeric characters replaced by hyphens, truncated to 50 characters. A short
random suffix (4 hex chars) is appended to prevent collisions between Sessions with
identical titles:

```
spectty/add-auth-endpoint-a3f2
spectty/refactor-payment-module-9c1e
spectty/fix-typo-in-readme-0041
```

The branch is created from the Workspace's current HEAD at Session creation time (not
from a fixed base like `main`). This lets the user start a new Session from whatever
working state they are in.

> ❓ OPEN: Should the base be configurable (e.g. always branch from `main` regardless of
> current HEAD)? Relevant when the user is already on a feature branch. Decide in design.

---

## Worktree lifecycle

```
Session created (Isolated mode)
        │
        ▼
GitPort::create_worktree(workspace, branch: "spectty/<slug>", path: .spectty/worktrees/<slug>)
        │
        ├──▶ branch "spectty/<slug>" created from HEAD
        ├──▶ worktree directory checked out at .spectty/worktrees/<slug>/
        └──▶ Session.worktree = Some(Worktree { branch, path })
        │
        ▼
Agent runs in worktree path (LaunchSpec.cwd = worktree path)
        │
  [agent edits files, makes commits in the worktree branch]
        │
        ▼
User approves Session  ──▶  review-then-merge flow (see below)
        │
        ▼
Session closed (cleanly)
        │
        ▼
GitPort::remove_worktree(workspace, worktree)
        │  git worktree remove --force <path>
        └──▶ worktree directory deleted; branch optionally deleted (see below)
```

Worktrees are stored at `.spectty/worktrees/<slug>/` relative to the Workspace root. This
path is added to the Workspace's `.git/worktrees/` by git and is invisible to the agent
(it sees a normal working directory).

> ✅ DECIDED: Yes — .spectty/worktrees/ is added to .gitignore automatically (the repo stays clean).

---

## Review-then-merge flow

When a Session completes and the user is ready to integrate the agent's work:

```
User: "approve Session"
        │
        ▼
UI shows VibeLens panel (DiffExplanation) for final review
        │
        ▼
User confirms merge
        │
        ▼
GitPort::merge_worktree_branch(workspace, branch: "spectty/<slug>", strategy: FastForwardOrMerge)
        │  equivalent to: git merge spectty/<slug> (on the main checkout's current branch)
        ▼
GitPort::remove_worktree(workspace, worktree)
        ▼
GitPort::delete_branch(workspace, branch: "spectty/<slug>")  ← optional, user preference
        ▼
Session status → Completed; Session closed
```

The merge happens on the **main checkout**, not in the worktree. `GitPort` switches to
the main checkout, performs the merge, then removes the worktree.

If the merge produces conflicts, `GitPort` surfaces them as an `Error` result. The
Session remains open; the worktree is preserved so the user can resolve the conflict
manually or discard the Session's work entirely.

> ❓ OPEN: Should Spectty offer an interactive conflict resolution UI, or hand off to the
> user's editor? For MVP, surface the conflict paths and let the user resolve externally.

---

## Collision avoidance between parallel agents

Multiple Sessions on the same Workspace each work in separate worktrees on separate
branches — they cannot modify the same working directory. However, logical conflicts can
still arise if two agents edit the same file independently.

Spectty does not prevent logical conflicts (that is a product-layer concern). It prevents
*filesystem* collisions through worktree isolation. At merge time, git will detect the
conflict.

**Naming collision** (two Sessions producing the same `spectty/<slug>` branch name) is
prevented by the 4-hex random suffix. Before creating a branch, `GitPort` checks for
existence and regenerates the suffix if needed (up to 3 retries before surfacing an error).

---

## Checkpoints

A Checkpoint is a snapshot of a Worktree's state taken before a risky action (e.g.
before the agent begins destructive refactoring), enabling one-click rollback.

### Storage recommendation: dedicated commit on the worktree branch

Three options considered:

| Option | Pros | Cons |
|---|---|---|
| **Git stash** | Simple, familiar | Stash is global to the repo, not isolated to the worktree; can be confused with manual stashes |
| **Dedicated commit** (recommended) | Isolated to the worktree branch; survives branch switches; visible in `git log`; clean rollback via `git reset --hard <checkpoint-sha>` | Adds a commit to the branch history (can be cleaned at merge time with `--squash`) |
| **Separate ref namespace** (`refs/spectty/checkpoints/<session>/<n>`) | No branch pollution | More complex; requires custom ref management; harder to inspect manually |

**Recommendation: dedicated commit on the worktree branch**, with a well-known commit
message prefix (`spectty: checkpoint <label>`) so the merge step can optionally squash them.
The commit carries the full snapshot including any uncommitted changes (staged via
`git add -A` before committing, then restoring the worktree to the pre-checkpoint state
with `git reset HEAD~1 --mixed` after saving the ref). This gives a clean rollback
target without forcing the agent to work on a dirty tree.

```rust
// GitPort checkpoint operations:
async fn create_checkpoint(&self, worktree: &Worktree, label: &str) -> Result<Checkpoint>;
async fn restore_checkpoint(&self, worktree: &Worktree, checkpoint: &Checkpoint) -> Result<()>;
async fn list_checkpoints(&self, worktree: &Worktree) -> Result<Vec<Checkpoint>>;
```

> ✅ DECIDED: Checkpoints use a dedicated commit on the worktree branch (resolved; not stash).

---

## Cleanup on crash

If the Spectty process crashes (or is force-killed) while Sessions are active:

1. **Worktrees survive** — git worktrees are filesystem-level; they persist across process
   restarts. This is the desired behavior: no work is lost.
2. **On next startup**, Spectty scans `GitPort::list_worktrees()` for any `spectty/*` branches
   that correspond to known Session IDs (Session state is persisted separately — see
   [data-flow.md](data-flow.md)). It reconciles them and marks Sessions as `Error` if
   their agent process is no longer running.
3. **Orphan worktrees** (no matching Session record) are surfaced to the user for manual
   cleanup. Spectty offers a "clean up orphan sessions" action.

> ❓ OPEN: Define the Session persistence store. Options: a SQLite DB in the app data
> directory, or a flat JSON file per Workspace under `.spectty/sessions/`. Decide before
> implementing crash recovery. SQLite is recommended for query flexibility.

---

## Lifecycle diagram

```
                    ┌─────────────────────────────────────────────────────┐
                    │              Workspace                               │
                    │  main branch: <user's current branch>               │
                    │  .spectty/worktrees/                                    │
                    └───────┬─────────────────────────────────────────────┘
                            │
              ┌─────────────▼──────────────┐
              │  Session created            │
              │  mode: Isolated             │
              └─────────────┬──────────────┘
                            │  GitPort::create_worktree()
                            │  branch: spectty/<slug>
                            ▼
              ┌─────────────────────────────┐
              │  Worktree active            │
              │  agent running in cwd       │
              │  FileWatchPort watching     │
              │  VibeLens pipeline live     │
              └──┬──────────┬──────────────┘
                 │          │
    [optional]   │          │  [agent edits, commits]
  create_checkpoint()       │
       saves ref            │
                 │          ▼
              [restore?]   Session completes / user approves
              git reset         │
              to checkpoint     │
                                ▼
                    ┌───────────────────────┐
                    │  Review & Merge       │
                    │  VibeLens final view  │
                    └───────────┬───────────┘
                                │  GitPort::merge_worktree_branch()
                                │  GitPort::remove_worktree()
                                ▼
                    ┌───────────────────────┐
                    │  Session Completed    │
                    │  worktree deleted     │
                    │  branch deleted (opt) │
                    └───────────────────────┘
```
