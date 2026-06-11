# M3 — Hook-Based Status Detection — Manual Acceptance Checklist

> **Status: PASS — manual run executed 2026-06-10 (KL) on macOS (aarch64), Claude Code
> v2.1.172, packaged debug bundle. All gating criteria 11.1–11.5 PASS; 11.6 SKIP (no
> Windows host). Three defects found and fixed during the run: PR #30 (Working-bounce),
> PR #31 (whitespace-insensitive patterns), PR #32 (AwaitingInput resolution). See the
> per-criterion result lines and the acceptance-gate section.**
>
> SDD apply phase, WU-11 (PR-5, the final M3 slice). Consumes
> `sdd/M3-hook-status-detection/tasks` (obs #831) + `design` (obs #829).
> Maps **verbatim** to the six roadmap M3 exit criteria
> (`openspec/changes/M3-hook-status-detection/tasks.md` §WU-11).
>
> **Gating: macOS.** Criteria 11.1–11.5 are the M3 pass/fail gate. Windows sidecar
> smoke (11.6) is best-effort/informational and MUST NOT block M3.
>
> These criteria CANNOT be unit-tested — they require a REAL Claude Code install and a
> real PTY session driven by hand. Each criterion lists: preconditions, exact manual
> steps (commands, clicks, where to look), expected observation, and the **automated
> floor** (the CI test that already proves the mechanical core). The automated floor is
> the regression guard; the manual run validates the parts synthetic fixtures cannot reach.
>
> **StopFailure / Error leg — DEFERRED (see 11.3 note).** `SubagentStop` fires on every
> subagent completion with no failure discriminator (design §3.4 deviation, PR-4
> `SubagentStop` note). The "API failure → `Error`" acceptance leg is therefore N/A for
> Slice 2 and must not be marked FAIL. It is documented as a conscious deferral — see
> ADR D21-D25 notes in `docs/decisions/0004-agent-agnostic-core.md` and the deferred
> items section at the bottom of this document.

---

## How to run

1. Build the packaged app (not `cargo run` — sidecars are only bundled via Tauri):
   ```
   pnpm tauri build --debug --config src-tauri/tauri.bundle.conf.json
   ```
   Open the resulting `.app` bundle from `src-tauri/target/debug/bundle/macos/`.
2. Have a real `claude` (Claude Code CLI) on `PATH` and a throwaway local **git
   repo** to point sessions at. The repo must be writable so the session can edit
   files (needed for 11.3 permission prompt).
3. Keep a second terminal open to inspect:
   - `~/.claude/settings.json` — managed hook rows (Global scope)
   - `{project}/.claude/settings.json` — managed hook rows (Project scope)
   - `~/.claude.json` — should have NO Spectty entry under `mcpServers`
   - `{data_local_dir}/app.spectty.desktop/runtime/` — state files
     (`data_local_dir` on macOS: `~/Library/Application Support`)

---

## Exit criteria

### Criterion 11.1 — Bypass-permissions session: `Running` → `Idle` driven by hook, not scraping `[REQ:acceptance-gate/criterion-1]`

**Preconditions**
- App opened from the packaged `.app` bundle (sidecars must resolve from bundle).
- The throwaway git repo is on `PATH`-accessible.

**Steps**
1. Open the Spawn dialog; pick **Claude Code**; set workspace to the throwaway git repo;
   give it a title; click **Spawn**.
2. Wait until the Pane-header badge shows `Idle` (blue) — Claude Code is at its prompt.
3. Type a simple, self-contained task (e.g. "add a comment to README.md") and submit.
4. Watch the badge while the task runs.
5. Wait for Claude Code to finish the task and return to its prompt.

**Observe**
- Badge transitions: `Idle` → `Running` (on submit) → `Idle` again within **~200 ms** of
  the turn ending — without any TUI scraping (the hook fires `Stop`, sidecar writes
  `{event:Stop}`, the next QUIESCE tick picks it up).
- The transition to `Idle` does NOT wait for PTY text patterns. If the session uses
  `--dangerously-skip-permissions`, scraping would previously leave the badge stuck at
  `Running`; the hook-sourced path resolves it correctly.

**Automated floor**
- `src-tauri/tests/hook_integration.rs` `real_pty_hook_sourced_stop_emits_idle` (#[cfg(unix)]):
  writes `{Stop, ts:1}` out-of-band while `run_signal_loop` runs over a real PTY; asserts
  `StatusChanged(Idle)` is emitted within 5 s. Proves the hook→loop→status pipeline end-to-end.
- `session_runtime.rs` unit tests on `run_signal_loop_hook_stop_from_running_emits_idle` and
  `run_signal_loop_hook_does_not_double_emit_when_same_tick_scrape_agrees`.

**Result: ☑ PASS — 2026-06-10 / KL.** Badge `Idle` at prompt → `Running` on submit (hook
`Submit`) → `Idle` ~200 ms after the turn ends (hook `Stop`), no scraping dependence.
Required PR #30 (gate suppresses scraped `Working` from `Idle`/`Starting` — Claude's TUI
redraws at the idle prompt and bounced the hook-sourced `Idle` back to `Running`).

---

### Criterion 11.2 — Managed hook rows in `settings.json`; foreign keys intact; no Spectty entry in `mcpServers` `[REQ:acceptance-gate/criterion-2]`

**Preconditions**
- Session has reached `Idle` at least once (inject fires on spawn, before PTY).
- The test repo already has some pre-existing `.claude/settings.json` content (add a
  foreign `"env": {"FOO": "bar"}` entry before spawning to exercise foreign-key survival).

**Steps**
1. Immediately after the session reaches `Idle`, in your second terminal:
   ```bash
   cat ~/.claude/settings.json        # Global scope
   # OR for Project scope:
   cat {your-test-repo}/.claude/settings.json
   ```
2. Also inspect `~/.claude.json`:
   ```bash
   cat ~/.claude.json | python3 -m json.tool | grep -A5 mcpServers
   ```

**Observe**
- `settings.json` contains a `hooks` object with entries for at least:
  - `UserPromptSubmit` → one hook entry with `command` pointing at the bundled
    `spectty-hook` binary and `args: ["--event", "Submit"]`.
  - `Stop` → similar entry with `args: ["--event", "Stop"]`.
  - (Slice 2) `Notification` → entry with `args: ["--event", "Permission"]` and a
    `matcher` field containing the permission-prompt string.
  - (Slice 2) `SessionEnd` → entry with `args: ["--event", "SessionEnd"]`.
- Any pre-existing foreign keys (`env`, `model`, `permissions`, or a user-authored hook
  on the same event) are **intact and in their original order**.
- `~/.claude.json` `mcpServers` has NO `spectty` or `spectty_*` entry (hook rows live
  in `settings.json`; MCP server rows live in `~/.claude.json` / `.mcp.json`).

**Automated floor**
- `json_namespace.rs` `inject_spectty_hooks_round_trip_preserves_foreign_keys_and_order`
  (R7 generalized headline) + `inject_spectty_hooks_notification_entry_has_permission_matcher`.
- `settings_provisioner.rs` `claude_settings_provisioner_inject_writes_correct_scope_path_and_backs_up`.
- **Manual-only gap**: the EXACT real `settings.json` shape is only exercised against
  synthetic `FakeConfigFile` fixtures. Confirm scope resolution and the pretty-printed
  shape against the real file.

**Result: ☑ PASS — 2026-06-10 / KL.** All four managed hook rows present (`UserPromptSubmit`,
`Stop`, `Notification` matcher=`permission_prompt`, `SessionEnd`) pointing at the bundle's
`Contents/MacOS/spectty-hook` with exec-form `args` (confirmed valid against the official
Claude Code hooks schema). Foreign keys intact: user-authored `gentle-ai` UserPromptSubmit
hook, `permissions.allow` (69 entries), `attribution`. Spectty rows live ONLY in
`settings.json` `hooks`; the `spectty` row in `~/.claude.json` `mcpServers` belongs to the
M2 MCP provisioner and is present only while a session is live.

---

### Criterion 11.3 — Slice 2 lifecycle: permission prompt → `AwaitingInput`; clean session end → `Completed`; API failure → `Error` (DEFERRED — see note) `[REQ:acceptance-gate/criterion-3]`

**Preconditions**
- Session is running with Slice 2 hooks active (all four hook rows visible in `settings.json`
  per criterion 11.2).

**Steps — AwaitingInput (permission prompt)**
1. From `Idle`, give the agent a task that requires a permission prompt (e.g. ask it to
   create or delete a file in a restricted path, or run a command it must confirm).
2. Watch the badge as the task runs and hits the prompt.
3. Provide or deny the permission; watch the badge after.

**Observe — AwaitingInput**
- Badge: `Running` → `AwaitingInput` when the `Notification` hook fires with the
  permission-prompt matcher. The transition is hook-driven, not PTY-scraping-driven.
- After you respond: `AwaitingInput` → `Running` (if task continues) or `Idle`.

**Steps — Completed (clean session end)**
1. Use a Claude Code session that completes naturally (e.g. a `bash` session that exits
   cleanly, or ask Claude Code to finish and exit).
2. Watch the badge.

**Observe — Completed**
- Badge: `Running` → `Completed` (or `Idle` → `Completed`) when `SessionEnd` fires.
  No scraping required.

**Note — API failure → `Error` leg: N/A-DEFERRED**

The `SubagentStop` hook fires on **every** subagent completion (success or failure) with
no failure discriminator in its payload. Wiring `SubagentStop → StopFailure → Error`
would flip healthy sessions to `Error` on every tool-call completion. Therefore the
`StopFailure` hook entry is **not** registered in the production event list for Slice 2.
The `Error` state remains reachable via non-hook paths (e.g. EOF with non-zero exit code
when that path is wired in a future milestone). This is a CONSCIOUS deferral — not a
regression. See design §3.4 deviation note and the deferred items section below.

**Automated floor**
- `event_to_observed_table` covers Permission→NeedsInput, SessionEnd→Finished mappings.
- `inject_spectty_hooks_notification_entry_has_permission_matcher` pins the matcher constant.
- `run_signal_loop_hook_stop_from_running_emits_idle` (analogous path for NeedsInput/Finished).
- **Manual-only gap**: the permission-prompt matcher string (`PERMISSION_PROMPT_MATCHER`) is
  EMPIRICAL — if the prompt is NOT detected, refine the constant in
  `crates/adapters/src/hook/state.rs` (one-line DATA edit) and add a unit test assertion.
  This is the only real-session validation of the matcher value.

**Result — AwaitingInput: ☑ PASS — 2026-06-10 / KL.** Hook `Permission` fired (state file
showed `{"event":"Permission"}`), badge `Awaiting input` while the dialog was up, `Running`
after approval, `Idle` at turn end. Required PR #31 (Ink renders space runs as
cursor-forward CSI sequences → patterns stored whitespace-free, matched against a
whitespace-stripped window) and PR #32 (core row `(AwaitingInput, Ready) => Idle` + gate
suppresses scraped `NeedsInput` everywhere and scraped `Ready` from `AwaitingInput` — the
resolved dialog's text lingers in the rolling window and re-pinned the state).
**Result — Completed: ☑ PASS — 2026-06-10 / KL.** `/exit` inside the session → badge
`Completed` via the `SessionEnd` hook.
**Result — Error (DEFERRED): ☑ N/A — deferred (SubagentStop no failure discriminator)**

---

### Criterion 11.4 — Close session: PTY terminates; hook rows removed from `settings.json`; `.state` file deleted; foreign keys intact `[REQ:acceptance-gate/criterion-4]`

**Preconditions**
- A session is running with hook rows visible in `settings.json`.
- The runtime dir exists: `~/Library/Application Support/app.spectty.desktop/runtime/`
  and contains a `spectty-{session-id}.state` file.

**Steps**
1. Click **Close** in the Pane header (or use the keyboard shortcut).
2. In your second terminal, confirm the `claude` child process is gone:
   ```bash
   ps aux | grep claude | grep -v grep
   ```
3. Re-inspect `settings.json`:
   ```bash
   cat ~/.claude/settings.json
   ```
4. Check the runtime dir:
   ```bash
   ls ~/Library/Application\ Support/app.spectty.desktop/runtime/
   ```

**Observe**
- `claude` child process is no longer present in `ps` output.
- The Pane returns to a spawnable state (SpawnDialog re-shows or Pane shows idle state).
- All Spectty-managed hook rows are **removed** from `settings.json`'s `hooks` object.
  Every foreign entry (user-authored `env`, `model`, `permissions`, foreign hooks on
  any event) is intact.
- The `spectty-{session-id}.state` file is **absent** from the runtime dir.
  (`spectty-{id}.state.tmp` also absent.)

**Automated floor**
- `close_session_impl_kills_pty_then_retracts_both_then_deletes_state`: asserts kill-first,
  then both retracts, then state-file deletion order.
- `close_session_impl_tolerates_absent_state_file`: idempotent close when `.state` absent.
- `json_namespace.rs` `inject_then_retract_preserves_hand_formatted_foreign_values`: retract
  leaves foreign keys untouched.

**Result: ☑ PASS — 2026-06-10 / KL.** After **Close**: all spectty-hook rows removed from
`settings.json` (foreign `gentle-ai` hook + `permissions.allow` 69 entries intact), the
`spectty` MCP row removed from `~/.claude.json`, and `spectty-{id}.state` deleted from the
runtime dir. Verified by direct filesystem inspection immediately after the click.

---

### Criterion 11.5 — Packaged build resolves both sidecars `[REQ:acceptance-gate/criterion-5]`

**Preconditions**
- A packaged `.app` has been built via the exact bundle command (not `cargo run`):
  ```
  pnpm tauri build --debug --config src-tauri/tauri.bundle.conf.json
  ```
- The `scripts/build-sidecars.sh` script has run (it is wired as `beforeBuildCommand` in
  `tauri.conf.json`, so it runs automatically as part of the command above).

**Steps**
1. Locate the built bundle (on macOS, typically under
   `src-tauri/target/debug/bundle/macos/spectty.app`).
2. Inspect `Contents/MacOS/`:
   ```bash
   ls -lh src-tauri/target/debug/bundle/macos/spectty.app/Contents/MacOS/
   ```
3. Open the `.app` bundle (double-click or `open` command). Spawn a Claude Code session.
4. Watch the badge and confirm hook-sourced status changes occur (per criterion 11.1).

**Observe**
- `Contents/MacOS/` contains three files: `spectty` (main app), `spectty-mcp`, and
  `spectty-hook` — all non-zero byte size.
- When a session is spawned from the packaged app, both sidecars resolve correctly from
  the bundle (no "binary not found" or provisioning errors in the app logs).
- The hook rows injected into `settings.json` point at the path inside the bundle's
  `Contents/MacOS/spectty-hook` (or the Tauri-resolved sidecar path).
- Claude Code starts successfully with both MCP server and hooks registered.

**Automated floor**
- PR-4b bundle verification evidence (empirical, recorded in apply-progress):
  `Contents/MacOS/spectty` (25 676 744 bytes), `Contents/MacOS/spectty-hook`
  (489 744 bytes), `Contents/MacOS/spectty-mcp` (544 048 bytes) — all present after
  `pnpm tauri build --debug --config src-tauri/tauri.bundle.conf.json`.
- `src-tauri/tests/hook_integration.rs` `spectty_hook_end_to_end_monotonic_ts_and_path_agreement`:
  asserts `src-tauri`'s `spectty_runtime_dir()` and the sidecar's resolver return the SAME
  path (D25 path agreement — load-bearing test).
- **Manual-only gap**: a packaged launch (not `cargo run`) is the only validation that
  Tauri's sidecar resolution path works end-to-end with a real Claude Code binary.

**Result: ☑ PASS — 2026-06-10 / KL.** The ENTIRE acceptance run was executed from the
packaged bundle (`target/debug/bundle/macos/Spectty.app`). `Contents/MacOS/` contains
`spectty` (26 MB), `spectty-hook` (490 KB), `spectty-mcp` (544 KB); the injected hook rows
point at the bundle path and the state files prove the bundled sidecar executed.
Local-dev gotcha recorded: `tauri build` clobbers `target/debug/spectty-hook` with the
RELEASE sidecar copy — run `cargo build -p spectty-hook` before `cargo test --workspace`.

---

### Criterion 11.6 (best-effort, ungated) — Windows `spectty-hook` binary smoke `[REQ:cross-platform/macos-gating-windows-best-effort]`

If a Windows host is available, smoke-test the `spectty-hook` binary:
```
spectty-hook.exe --event Stop
# With SPECTTY_SESSION_ID set and a writable runtime dir
```
Failure does **NOT block M3** — informational only. The native binary avoids shell-quoting
issues on Windows; the atomic `rename` on Windows is handled by Rust's `fs::rename`.

**Result: ☑ SKIP (no Windows host) — 2026-06-10 / KL.** Informational only; does not gate M3.

---

## Exit-criteria coverage summary

| # | Criterion | Automated floor (CI) | Manual-only delta |
|---|-----------|----------------------|-------------------|
| 11.1 | Hook-sourced `Stop` → `Idle` (~200 ms, bypass-permissions) | `real_pty_hook_sourced_stop_emits_idle`; `run_signal_loop` hook unit tests | Real Claude Code session; bypass-permissions mode |
| 11.2 | Managed hook rows in `settings.json`; foreign keys intact; no Spectty in `mcpServers` | `inject_spectty_hooks` foreign-key round-trip; `ClaudeSettingsProvisioner` inject (global/project/backup); Notification matcher test | Exact real `settings.json` shape + scope |
| 11.3 | Slice 2: `AwaitingInput` (permission), `Completed` (SessionEnd); `Error` N/A-deferred | `event_to_observed_table`; matcher constant test; `run_signal_loop` analogous paths | Permission-prompt matcher value (empirical); real session-end observation |
| 11.4 | Close → PTY terminates + hooks removed + `.state` deleted; foreign keys intact | `close_session_impl` ordering + tolerates-absent; `json_namespace` retract-foreign-keys | Real child-process termination; real `settings.json` post-close |
| 11.5 | Packaged build resolves both sidecars | PR-4b bundle evidence (empirical, recorded); D25 path-agreement integration test | Packaged app launch with real Claude Code binary |
| 11.6 | Windows smoke (ungated) | — | Manual only; does not gate M3 |

Automated vs manual: criteria 11.4 and 11.5 have a strong automated floor. Criteria 11.1
and 11.2 are **manual-dominant** — their signals require a real Claude Code install.
Criterion 11.3 is manual-dominant for the `AwaitingInput` matcher and session-end
observation; the `Error` leg is explicitly deferred.

---

## Deferred items

### L-settings-orphan — deferred to M4 boot-sweep

This is M3's widening of M2's L5/R8 deferral.

**What is deferred**: full boot-time reconciliation of (a) leaked hook rows in
`settings.json` from a session that crashed between inject and retract, and (b) orphaned
`.state` files in the runtime dir from a session that was never closed cleanly.

**Mitigations shipped in M3 (today)**:

| Mitigation | Description |
|---|---|
| `.spectty.bak` | Atomic write seam: `tmp → fsync → rename`, backup before first write. Allows manual recovery if the hook list is corrupted. |
| Stale-state harmlessness | A stale `.state` file from a crashed session cannot corrupt a live session: `StateFileReader` checks `session_id` (D23 correlation — design §3.3). A stale file with the wrong `session_id` returns `None` on every poll tick. |
| Opportunistic pre-spawn sweep | `remove_stale_state_file` + `remove_stale_tmp_files` run at spawn time for the same `session_id` (W2 fix, PR-2 adversarial review). This sweeps the previous session's `.state` if it was not cleaned up, so a reused `session_id` starts fresh. |

**What M4 will add**: a boot-time sweep that scans the runtime dir and `settings.json` for
all orphaned Spectty-owned rows and files (using the session registry as the source of
truth) and removes anything not corresponding to a live session. This requires the
persistence-backed session registry to be available at boot, which is a separate milestone
dependency.

**Why this deferral is safe for M3**: a leaked hook row points at the real `spectty-hook`
binary and does nothing unless Claude Code fires that event. A leaked `.state` file is
silently ignored by the `session_id` guard. Neither causes data loss or crashes. The
`.spectty.bak` escape hatch allows manual recovery in the rare case of corruption.

---

## Acceptance gate (WU-11)

All macOS criteria (11.1–11.5) must pass for **M3 acceptance = PASS**; Windows (11.6) is
informational. Record real-run results in the table above when executed against a live
Claude Code install.

**M3 ACCEPTANCE = PASS — 2026-06-10 / KL.** All gating criteria 11.1–11.5 PASS on macOS
(aarch64), Claude Code v2.1.172, packaged debug bundle; 11.6 SKIP (no Windows host).

The run surfaced and fixed three live-only defects (each TDD'd with the real captured
evidence as fixture, adversarially reviewed with fresh context, and merged before
re-running):

| PR | Defect | Fix |
|----|--------|-----|
| #30 | Idle TUI redraw scraped as `Working` bounced hook-sourced `Idle`→`Running`; boot banner trapped `Starting` at `Running` | Gate suppresses scraped `Working` from `Idle`/`Starting` |
| #31 | Ink emits space runs as cursor-forward CSI → ANSI-stripped window concatenates words → spaced patterns never matched | Patterns stored whitespace-free, matched against a whitespace-stripped window |
| #32 | Resolved dialog text lingers in the 8KB window re-pinning `AwaitingInput`; core row `(AwaitingInput, Ready) => Running` blocked hook `Stop` from reaching `Idle` | Core row → `Idle`; gate suppresses scraped `NeedsInput` everywhere + scraped `Ready` from `AwaitingInput` |

The automated floor runs green on `main` after the three fixes (`cargo test --workspace`,
252 tests passing). It guards every mechanical core of the five criteria; the manual run
validated the real-CLI-specific gaps that synthetic fixtures cannot reach.
