# Testing Strategy

TDD is the default working mode for Spectty. Write the test first, watch it fail, make it
pass. The hexagonal architecture is the reason this is tractable: the Core has no I/O,
so tests are fast, deterministic, and require no setup beyond constructing a few structs.

---

## The pyramid

```
           ┌─────────────┐
           │   E2E (UI)  │   small count, high confidence
           ├─────────────┤
           │   UI units  │   component + hook tests (Vitest)
           ├─────────────┤
           │  Adapter    │   real pty, real git, real fs
           │  integration│
           ├─────────────┤
           │  Core units │   ← the largest layer; zero I/O; fake adapters
           └─────────────┘
```

The Core unit layer is the largest. That is intentional. The payoff of hexagonal
architecture is exactly this: the domain's most important behavior (state machine
transitions, DiffExplanation assembly, worktree rules) is verified without a running
PTY, a real git repo, or a live agent.

---

## Layer 1 — Core unit tests

**Location:** `crates/core/src/` — inline `#[cfg(test)]` modules next to the code they test.

**Tools:** `cargo test`, standard Rust test framework.

**Strategy:** replace every Port with a fake (a struct in the test that implements the
trait) and drive the Core's use cases and state machines directly.

### What to test here

- `AgentStatus` state machine — every valid transition, every invalid one (must not
  silently accept illegal transitions):
  - `test_starting_transitions_to_idle_on_ready_signal`
  - `test_running_transitions_to_awaiting_input_on_prompt_detected`
  - `test_awaiting_input_returns_to_running_after_input_given`
  - `test_error_is_terminal_no_further_transitions`
  - `test_completed_is_terminal`

- `DiffExplanation` assembly — given a fake `DiffExplainerPort` returning a canned
  response, verify the domain object is constructed with correct file paths, line counts,
  and rationale fields:
  - `test_diff_explanation_maps_all_changed_files`
  - `test_diff_explanation_handles_empty_diff`
  - `test_diff_explanation_handles_deleted_files`

- Session invariants — worktree ownership, notification-exactly-once on
  `AwaitingInput` transition, `CostMetrics` accumulation:
  - `test_session_notifies_exactly_once_on_awaiting_input`
  - `test_cost_metrics_accumulates_across_multiple_updates`
  - `test_session_worktree_must_belong_to_same_workspace`

- SessionRegistry — spawn, lookup, terminate, list:
  - `test_registry_rejects_duplicate_session_id`
  - `test_registry_list_returns_only_active_sessions`

### Fake adapter pattern

```rust
// In test modules — never ship these in production code
struct FakeNotifier { calls: Vec<String> }
impl NotifierPort for FakeNotifier {
    fn notify(&mut self, message: &str) { self.calls.push(message.to_owned()); }
}
```

Fakes live in `crates/core/src/ports/fakes.rs` (or per-port test modules) and are
`#[cfg(test)]` gated. They are never in `crates/adapters`.

---

## Layer 2 — Adapter integration tests

**Location:** `crates/adapters/src/` (inline) and `crates/adapters/tests/`.

**Tools:** `cargo test` — these tests ARE slow and DO hit the OS. Mark them
`#[ignore]` if they take >5s so `cargo test` runs them only when explicitly requested
(`cargo test -- --include-ignored`).

### What to test here

- **PTY echo test:** spawn a real PTY running `/bin/sh`, write a command, read the
  output, verify it round-trips:
  - `test_pty_adapter_echoes_input`
  - `test_pty_adapter_resize_updates_winsize`

- **Git worktree in a temp repo:** create a temp git repo, call `GitAdapter` to add a
  worktree, verify the path exists and the branch is checked out:
  - `test_git_adapter_creates_worktree_on_new_branch`
  - `test_git_adapter_removes_worktree_on_close`
  - `test_git_adapter_produces_diff_for_modified_file`

- **File-watch debounce:** write a file in a temp directory, assert the watcher fires
  after the debounce window, assert it does NOT fire multiple times for rapid writes:
  - `test_file_watcher_debounces_rapid_writes`
  - `test_file_watcher_detects_new_file`

- **Per-agent status detection with recorded fixtures:** each `AgentRunner` adapter must
  parse PTY output to derive `AgentStatus`. Test this with fixture files containing
  recorded output from each supported agent (Claude Code, Aider, etc.), rather than
  live agent processes:
  - `test_claude_runner_detects_awaiting_input_from_fixture`
  - `test_aider_runner_detects_running_from_fixture`
  - `test_claude_runner_parses_cost_from_session_end_output`

  Fixtures live in `crates/adapters/tests/fixtures/<agent-name>/`.

> ❓ OPEN: Decide the fixture format (raw bytes vs. annotated JSON) and the process for
> recording new fixtures when agent output format changes. Tracked for M2.

---

## Layer 3 — UI unit tests

**Location:** `ui/tests/unit/`

**Tools:** [Vitest](https://vitest.dev/) + `@testing-library/react`.

### What to test here

- Component rendering: given props, assert the correct DOM output.
- Hook behavior: mock Tauri's `invoke` / `listen` and verify hooks call them correctly
  and transform responses into the expected state.
- Status indicator logic: `AgentStatus` → correct CSS class / color.

Example test names:
- `VibeLensPanel renders all changed files from DiffExplanation`
- `SessionSidebar shows pulsing indicator for AwaitingInput session`
- `useSession hook subscribes to session_update events on mount`
- `Dashboard displays CostMetrics formatted to two decimal places`

**No Tauri runtime is available in Vitest.** Mock `@tauri-apps/api/tauri` at the test
setup level.

---

## Layer 4 — E2E tests

**Location:** `ui/tests/e2e/`

**Tools:** [Playwright](https://playwright.dev/) via the
[`@playwright/test`](https://playwright.dev/docs/intro) runner.

> ❓ OPEN: Tauri + Playwright integration requires a running app. Investigate
> `tauri-driver` or a Playwright-controlled Tauri build. Track the setup at M3.

### What to test here

End-to-end flows that exercise the full stack — UI → Tauri bridge → Core → Adapters:

- **Spawn → see output:**
  - `spawning a session shows the agent in the Sessions sidebar`
  - `typing a command sends it through the PTY and output appears in the terminal`

- **VibeLens panel:**
  - `after agent modifies a file, VibeLens panel updates with DiffExplanation`
  - `each FileChange row shows path, line counts, and rationale`

- **AwaitingInput flow:**
  - `session transitions to AwaitingInput and sidebar indicator pulses`
  - `pressing the approve key sends the response and session returns to Running`

- **Multi-session:**
  - `two sessions run in parallel without their PTY output mixing`
  - `Dashboard shows both sessions and their respective AgentStatus`

---

## TDD in practice

1. Write the test. Run it — it should fail (red).
2. Write the minimum production code to make it pass (green).
3. Refactor with the tests as a safety net.
4. Commit test + implementation together.

For Core logic, this cycle is seconds. For adapter integration tests, it is slower but
the discipline still applies: write the failing test, then wire the adapter.

Never commit Core code without a corresponding Core unit test. The CI gate enforces
`cargo test --workspace` on every PR.
