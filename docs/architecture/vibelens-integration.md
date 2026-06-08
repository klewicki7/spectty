# VibeLens Integration

VibeLens is the signature feature of Spectty: every Session has a live panel showing *what*
the agent changed and *why*, per file, updated as the agent works. This document covers
the `DiffExplainerPort` contract, the adapter that calls the VibeLens MCP tool, and the
end-to-end pipeline from file change to rendered panel.

The `DiffExplanation` domain type is defined in [domain-model.md](domain-model.md). See
[overview.md](overview.md) for the dependency rule that keeps the Core ignorant of MCP.

---

## The `DiffExplainerPort` trait

```rust
// Shape only — not final signatures.
trait DiffExplainerPort: Send + Sync {
    /// Produce a DiffExplanation from a raw unified diff and a workspace path.
    /// The adapter is responsible for calling the VibeLens MCP tool.
    async fn explain(
        &self,
        diff: &str,           // unified diff (e.g. output of `git diff HEAD`)
        workspace: &Path,
    ) -> Result<DiffExplanation>;
}
```

The Core calls `explain()` and receives a `DiffExplanation`. It knows nothing about MCP,
HTTP, or the VibeLens tool. This is the boundary.

---

## The `McpAdapter` — calling `show_diff_explanation`

The concrete implementation of `DiffExplainerPort` is `McpAdapter`. It:

1. Takes the unified diff string produced by `GitPort`.
2. Constructs a per-file analysis by splitting the diff into file hunks and summarizing
   each hunk's changed line counts and paths.
3. Calls the VibeLens MCP tool `show_diff_explanation` with two arguments:
   - The full `git diff HEAD` output.
   - A structured per-file analysis (file path, lines added/removed, change kind).
4. Parses the tool's response into a `DiffExplanation`.

```rust
// Illustrative adapter shape.
impl DiffExplainerPort for McpAdapter {
    async fn explain(&self, diff: &str, workspace: &Path) -> Result<DiffExplanation> {
        if diff.trim().is_empty() {
            return Ok(DiffExplanation::empty());
        }
        let per_file = self.parse_file_hunks(diff);
        let response = self.mcp_client
            .call_tool("show_diff_explanation", json!({
                "diff": diff,
                "file_analysis": per_file,
            }))
            .await?;
        self.parse_response(response)
    }
}
```

> ❓ OPEN: Confirm the exact parameter schema expected by `show_diff_explanation`. The
> project's `CLAUDE.md` wires the tool with `git diff HEAD` output and "per-file
> analysis" — pin the field names once the VibeLens MCP spec is stable.

> ❓ OPEN: VibeLens MCP transport: stdio (spawn subprocess) vs. HTTP. If stdio, `McpAdapter`
> manages the subprocess lifecycle. If HTTP, `McpAdapter` holds a `reqwest` client with
> a base URL from config. See also [stack-decisions.md](stack-decisions.md).

---

## Context: Spectty internalizes the pattern from `CLAUDE.md`

The project's `CLAUDE.md` already wires VibeLens as a post-edit MCP hook for the
development workflow: after modifying files, call `show_diff_explanation` with `git diff HEAD`
and per-file analysis. Spectty promotes this from a developer convention into a first-class
product feature: every Session has its own always-on pipeline doing exactly this, surfaced
as the VibeLens panel. The mechanism is the same; the scope is per-agent-session, not
per-developer-edit.

---

## End-to-end pipeline

```
FileWatchPort
    │  FileChanged { path, kind }
    │  (debounced: 500 ms – 1 s window)
    ▼
GitPort::diff_head(workspace)
    │  → unified diff string (or empty string if no commits yet)
    ▼
dedup check
    │  hash(diff) == last_explained_hash? → skip
    ▼
DiffExplainerPort::explain(diff, workspace)
    │  → DiffExplanation
    ▼
Session::update_diff(explanation)
    │  stores last_diff on the Session aggregate
    ▼
Tauri event: diff_updated { session_id, explanation }
    ▼
UI: VibeLens panel re-renders
```

Each Session owns this pipeline independently. A file change in Session A's worktree
does not trigger Session B's diff pipeline.

### Debounce

The debounce window (500 ms default) is held in the `FileWatcher` adapter. Raw `notify`
events arrive at high frequency during a multi-file write; debouncing coalesces them into
a single trigger per quiesce period. The window is configurable.

> ❓ OPEN: Should the debounce window be per-Session (recommended) or global? Per-Session
> allows tuning per agent's write pattern without cross-session interference.

---

## Caching and deduplication

Before calling `DiffExplainerPort::explain()`, the pipeline computes a hash of the diff
string and compares it to the `last_explained_hash` stored on the Session. If they match,
the pipeline exits early — the explanation is still valid, no MCP call is made.

This prevents redundant calls when:
- A file system event fires but no tracked content changed (e.g. a `.gitignore`d temp file).
- The debounce window coalesces multiple events whose net diff is identical to the previous one.

The hash is stored alongside the `DiffExplanation` on the Session:

```rust
struct Session {
    // ... other fields
    last_diff: Option<DiffExplanation>,
    last_diff_hash: Option<u64>,  // hash of the diff string that produced last_diff
}
```

---

## Fallback: agents that emit their own diff signals

`AgentDescriptor` exposes `emits_diff_signals: bool` (see
[agent-abstraction.md](agent-abstraction.md)). When `true`, the agent explicitly signals
when it has finished editing files (e.g. via a structured output line). In this case:

- **The `FileWatchPort`-based debounce trigger is bypassed.**
- The `AgentRunner` adapter fires the diff pipeline directly when it receives the agent's
  edit-complete signal — typically with lower latency than waiting for filesystem quiescence.
- The pipeline (GitPort → dedup → DiffExplainerPort → event) is identical; only the
  trigger source changes.

When `emits_diff_signals == false` (the Generic adapter, most agents), the FileWatch
path is used exclusively.

---

## Edge case: no commits yet (empty repository)

When a Workspace has no commits, `git diff HEAD` fails because `HEAD` does not exist. In
this case, `GitPort` must handle the empty-tree case explicitly:

- Use `git diff --cached $(git hash-object -t tree /dev/null)` or the equivalent libgit2
  call to diff against the empty tree, capturing all staged/unstaged content.
- If the repository has no commits and no staged content (truly empty), return an empty
  diff and let the pipeline produce an `DiffExplanation::empty()` without calling the MCP.

```rust
// GitPort contract for this case:
async fn diff_head(&self, workspace: &Path) -> Result<String> {
    match self.has_commits(workspace).await? {
        true  => self.diff_against_head(workspace).await,
        false => self.diff_against_empty_tree(workspace).await,
    }
}
```

> ❓ OPEN: Confirm whether `git2` supports diff-against-empty-tree natively or requires
> constructing the empty tree object manually.

---

## Error handling and degradation

| Failure | Behavior |
|---|---|
| MCP server unreachable | Log warning; retain previous `DiffExplanation`; show "explanation unavailable" in VibeLens panel |
| MCP call times out | Same as unreachable; retry on next file-change trigger |
| `git diff` fails | Log error; skip pipeline; VibeLens panel shows last known state |
| Parse error on MCP response | Log + surface in panel as "parse error"; do not crash the Session |

The VibeLens panel degrades gracefully. A failed explanation does not affect the terminal,
the agent, or AgentStatus — it is a read-only enrichment surface.
