# M2 — Spawn Agent + Provisioner — Manual Acceptance Checklist

> SDD apply phase, WU-12 (PR7, the final M2 slice). Consumes
> `sdd/M2-spawn-agent-provisioner/tasks` (obs #803) + `design` (obs #802).
> Maps **verbatim** to the five roadmap M2 exit criteria
> (`docs/product/roadmap.md` §M2 → Exit criteria).
>
> **Gating: macOS.** Criteria 1–5 are the M2 pass/fail gate. Windows agent-spawn
> (12.6) is best-effort/informational and MUST NOT block M2.
>
> These five criteria CANNOT be unit-tested — they require a REAL Claude Code
> install and a real PTY session driven by hand. Each criterion below lists: the
> exact manual steps, what to observe, and the **automated floor** (the CI test
> that already proves the mechanical core of that criterion). The automated floor
> is the regression guard; the manual run validates the parts that synthetic
> fixtures cannot reach.

---

## How to run

1. Build the app in dev: `pnpm tauri dev` (or `cargo tauri dev`). The
   `spectty-mcp` sidecar in dev is invoked from `target/debug/spectty-mcp` — see
   **Known limitation L2** before a packaged build.
2. Have a real `claude` (Claude Code CLI) on `PATH` and a throwaway local **git
   repo** to point sessions at.
3. Keep a second terminal open to inspect `~/.claude.json` (GLOBAL) or the
   repo-root `.mcp.json` (PROJECT) between steps.

---

## Exit criteria

### Criterion 1 — Claude Code session reaches `Idle` `[12.1]` `[REQ:roadmap-exit/criterion-1]`

**Steps**
1. Open the Spawn dialog, pick **Claude Code**, set the workspace to your test git repo, give it a title, spawn.
2. Watch the Pane-header status badge.

**Observe**
- Badge progresses `Starting` → `Idle` (blue) once Claude Code is up and quiescent at its prompt.

**Automated floor**
- `src-tauri/src/session_runtime.rs` unit tests on `observe_and_diff` + the pure
  `transition` table prove `Starting → Idle` is reachable and that a quiescent
  PTY emits the `Idle` tick. The Generic real-PTY test (below) proves the live
  read-thread → producer → `detect_status` pipeline end-to-end. Claude Code's
  specific `Idle` quiescence is **manual-only** (depends on the real CLI's prompt
  output, not a fixture).

---

### Criterion 2 — Managed `spectty` MCP section present in the agent config `[12.2]` `[REQ:roadmap-exit/criterion-2]` `[REQ:provisioning-port/spectty-mcp-stub]`

**Steps**
1. Immediately after the session reaches `Idle`, inspect the resolved config:
   - **GLOBAL scope** (default, config not git-tracked): `~/.claude.json` top-level `mcpServers`.
   - **PROJECT scope** (config git-tracked): repo-root `.mcp.json` root `mcpServers`.
2. Look for the managed key `spectty_*` (the `MANAGED_SERVER_NAME` namespace).

**Observe**
- The managed `spectty_*` entry is present under `mcpServers`, points at the
  `spectty-mcp` binary, and **coexists** with any pre-existing user or
  `gentle-ai` entries (foreign keys, values, and order all intact).

**Automated floor**
- `crates/adapters/src/provision/json_namespace.rs`
  `inject_then_retract_preserves_hand_formatted_foreign_values` (R7 headline) +
  `crates/adapters/src/provision/claude_provisioner.rs`
  `inject_global_writes_managed_entry_and_returns_handle`,
  `inject_project_targets_repo_root_mcp_json`,
  `inject_backs_up_original_before_first_write`.
- `crates/spectty-mcp/tests/stdio_handshake.rs`
  `spectty_mcp_stdio_handshake_advertises_five_tools` proves the injected binary
  advertises the **5 frozen tools** over stdio.
- **Manual-only gap**: the EXACT real `~/.claude.json` shape is only exercised
  against synthetic fixtures (see **L1**). Confirm scope resolution and the
  pretty-printed shape against the real file.

---

### Criterion 3 — `Running` → `AwaitingInput` → `Running` across a permission prompt `[12.3]` `[REQ:roadmap-exit/criterion-3]`

**Steps**
1. From `Idle`, give the agent a task that triggers a permission prompt (e.g. ask it to edit/run something it must confirm).
2. Watch the badge as the task runs, hits the prompt, and after you answer.

**Observe**
- Badge: `Running` → `AwaitingInput` at the permission prompt → back to
  `Running` after you supply input.
- If the prompt is NOT detected: this is the **R5 empirical-pattern** path. Refine
  the matching string in `ClaudeCodeRunner` (a one-line `&'static str` DATA edit,
  never a Core change) and add a unit test for the refined pattern. This is the
  ONLY validation of the `AwaitingInput` scrape patterns — see **L4**.

**Automated floor**
- `ClaudeCodeRunner` `detect_status` unit tests cover the CURRENT pattern set as
  a table. The patterns are **empirical** and this criterion is their only
  real-session validation.

---

### Criterion 4 — Close session → PTY terminates AND managed section removed `[12.4]` `[REQ:roadmap-exit/criterion-4]` `[REQ:provisioning-port/inject-on-create-retract-on-close]`

**Steps**
1. Click **Close** in the Pane header.
2. Confirm the `claude` child process is gone (`ps`), then re-inspect the config from Criterion 2.

**Observe**
- PTY child terminates; the Pane returns to a spawnable state (SpawnDialog re-shows).
- The managed `spectty_*` key is **removed** from `mcpServers`; every foreign
  entry (user + gentle-ai) is intact.

**Automated floor**
- `claude_provisioner.rs` `inject_then_retract_removes_managed_entry` +
  `retract_absent_file_is_ok` (idempotent retract). `json_namespace.rs` proves
  retract leaves foreign keys untouched. Close/PTY-terminate wiring is covered by
  `src-tauri` `close_session` command tests.

---

### Criterion 5 — Generic `bash` → `Idle` → idle-timeout → `Completed` `[12.5]` `[REQ:roadmap-exit/criterion-5]` `[REQ:agent-runner/generic-idle-timeout]`

**Steps**
1. Spawn a **Generic** session with command `bash` (or `bash -l`) on any directory.
2. Leave it idle past the configured inactivity window.

**Observe**
- Badge: `Starting` → `Idle`, then `Completed` after the idle-timeout (or on a clean exit / EOF).
- NO `spectty_*` injection occurs — Generic agents skip provisioning
  (`requires_provisioning == false`, D7).

**Automated floor**
- `src-tauri/src/session_runtime.rs` `#[cfg(unix)]`
  `real_pty_generic_reaches_running_then_completed` (WU-11.1) spawns a real
  Generic command through the REAL read thread → `OutputSignal` producer →
  `signal_channel` → `run_signal_loop` and asserts status reaches `Running` then
  `Completed` via the EOF/exit path. This is exit-criterion 5 in miniature; the
  manual run adds the **wall-clock idle-timeout** path (the CI test uses the
  clean-exit path, not a real idle wait) and confirms no provisioning fires.

---

### Criterion 6 (best-effort, ungated) — Windows agent-spawn smoke `[12.6]` `[REQ:cross-platform/macos-gating-windows-best-effort]`

If a Windows host is available, smoke-test a Generic spawn. **Failure does NOT
block M2** — informational only.

---

## Exit-criteria coverage summary

| # | Criterion | Automated floor (CI) | Manual-only delta |
|---|-----------|----------------------|-------------------|
| 1 | Claude Code → `Idle` | `session_runtime` `observe_and_diff` + pure `transition` table; live pipeline via the Generic real-PTY test | Claude Code's real quiescence/prompt output |
| 2 | Managed `spectty` section present (5 tools) | `json_namespace` foreign-key round-trip; `claude_provisioner` inject (global/project/backup); `spectty-mcp` stdio handshake (5 tools) | Exact real `~/.claude.json` shape + scope (L1) |
| 3 | `Running`→`AwaitingInput`→`Running` | `ClaudeCodeRunner::detect_status` pattern table | Empirical R5 prompt patterns — manual-only validation (L4) |
| 4 | Close → PTY terminates + section removed | `claude_provisioner` inject→retract + idempotent retract; `close_session` wiring | Real child-process termination observation |
| 5 | Generic `bash` → `Idle` → `Completed` | `#[cfg(unix)] real_pty_generic_reaches_running_then_completed` (clean-exit path) | Wall-clock idle-timeout path; no-provisioning assertion |

Automated vs manual: criteria 2, 4, and 5 have a STRONG automated floor (the
mechanical core is regression-guarded in CI). Criteria 1 and 3 are
**manual-dominant** — their core signals are real-CLI-specific and cannot be
faithfully faked.

---

## Known limitations to validate manually / carry into verify + M3

- **L1 — `~/.claude.json` shape (synthetic fixtures only).** The injector is
  proven against in-memory `FakeConfigFile` fixtures and a hand-formatted config,
  NOT against a captured real Claude Code config. Confirm the real GLOBAL
  (`~/.claude.json` top-level `mcpServers`) vs PROJECT (`.mcp.json` root) shape and
  the pretty-printed result against an actual install. (Design open-question b.)

- **L2 — spectty-mcp sidecar bundling.** Dev runs the binary from
  `target/debug/spectty-mcp`. A **packaged** build needs the sidecar registered
  in `src-tauri/tauri.conf.json` under `bundle.externalBin` — **currently NOT
  configured**. Packaged-build provisioning is therefore unverified until that
  entry is added.

- **L3 — EOF exit-code = 0 limitation.** `pty_exit` derives termination from the
  read-side EOF and emits `code: None` (`src-tauri/src/commands/pty.rs:275`); it
  does NOT call `child.wait()` for the real status. A non-zero child exit is not
  reflected as a non-zero code, so the `Error`-on-failure transition cannot be
  driven by a failing exit code today. Affects how a crashed/failed agent surfaces
  (it reaches a terminal state via the read side, not via a distinguished error
  code). Carry to M3.

- **L4 — Claude `AwaitingInput` scrape patterns are empirical.** The
  `ClaudeCodeRunner` pattern set is hand-rolled `&'static str` data refined by
  observation. Exit-criterion 3 is their ONLY real-session validation. Refine
  against a real session; each refinement is a one-line DATA edit + a unit test,
  never a Core change.

- **L5 — R8 deferral: no boot-time orphan reconciliation (D14).** Provisioner
  startup orphan-reconciliation is **DEFERRED to M3**. M2 ships the escape hatch:
  `.spectty.bak` (atomic write: tmp → fsync → rename, backup-before-first-write)
  + an **idempotent retract** (`retract_absent_file_is_ok`). A crash between
  inject and retract can leak a single managed `spectty_*` key — harmless (it
  points at a real stub binary) but not auto-swept. **M3 adds persistence-backed
  reconciliation** that sweeps leaked keys on startup using the scope recorded on
  the Session. This is a CONSCIOUS, documented deferral — flag for `sdd-verify`.

---

## Acceptance gate (WU-12)

All macOS criteria (1–5) must pass for **M2 acceptance = PASS**; Windows (6) is
informational. Record real-run results here when executed against a live Claude
Code install. The automated floor above runs green in CI (`cargo test
--workspace`, unchanged from PR6) and guards every mechanical core of the five
criteria.
