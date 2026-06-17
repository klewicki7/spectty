# M4 — The Triad (Living Spec Pane + VibeLens) — Manual Acceptance Checklist

> **Status: PENDING — not yet run.** All criteria 11.1–11.8 are UNCHECKED. This document
> is the `sdd-verify` pass/fail gate, written during the apply phase (WU-11, PR-6). The
> user MUST run it against the real app on macOS before M4 acceptance = PASS. Do NOT mark
> any criterion PASS until it has been observed on a real session.
>
> SDD apply phase, WU-11 (PR-6, the FINAL M4 slice). Consumes
> `sdd/M4-triad-spec-vibelens/tasks` (obs #871) + `design` (obs #869) +
> `openspec/changes/M4-triad-spec-vibelens/{design.md,specs/*}`. Maps **verbatim** to the
> eight roadmap M4 exit criteria (`specs/spec.md` §Acceptance gate, M4-REQ-25) and
> `tasks.md` §WU-11.
>
> **Gating: macOS.** Criteria 11.1–11.8 are the M4 pass/fail gate. Windows is best-effort
> / informational and MUST NOT block M4.
>
> These criteria CANNOT be unit-tested — they require a REAL Claude Code (Cooperative)
> install, a REAL running engram daemon (`:7437`), and the REAL VibeLens stdio server
> (`npx -y vibelens-mcp`), plus a real PTY session driven by hand. Each criterion lists:
> preconditions, exact manual steps, the expected observation, and the **automated floor**
> (the CI tests that already prove the mechanical core). The automated floor is the
> regression guard; the manual run validates the parts synthetic fixtures cannot reach.
>
> **Empirical-shape note.** The `show_diff_explanation` field names (G2, verified
> 2026-06-11 v0.1.0) and the engram REST shapes (G1) are EMPIRICAL. If a real run surfaces
> a shape drift, refining it is a DATA / adapter change (`crates/adapters/src/...`), never
> a Core change — Core stays `serde + thiserror` only (`cargo deny check bans` = `bans ok`).

---

## How to run

1. Build the packaged app (not `cargo run` — sidecars are only bundled via Tauri):
   ```
   pnpm tauri build --debug --config src-tauri/tauri.bundle.conf.json
   ```
   Open the resulting `.app` bundle from `src-tauri/target/debug/bundle/macos/`.
   (Local-dev gotcha from M3: `tauri build` clobbers `target/debug/spectty-hook` with the
   RELEASE sidecar copy — run `cargo build -p spectty-hook` before `cargo test --workspace`.)
2. Prerequisites on `PATH` / running:
   - a real `claude` (Claude Code CLI), for the Cooperative path;
   - a running **engram** daemon on `:7437` (the engram-as-bus, Decision 1 — D26/G1);
   - `npx` available so the app can spawn `npx -y vibelens-mcp` (G2 — the VibeLens stdio
     display sink);
   - a throwaway, writable local **git repo** to point sessions at (VibeLens needs a git
     working tree; the empty-repo path is handled — D35).
3. Keep a second terminal open to inspect engram (the bus):
   ```bash
   # the three per-session keys (replace {sid} with the live session id):
   curl -s "http://127.0.0.1:7437/observations?topic_key=spectty/{sid}/spec"
   curl -s "http://127.0.0.1:7437/observations?topic_key=spectty/{sid}/approval"
   curl -s "http://127.0.0.1:7437/observations?topic_key=spectty/{sid}/diff"
   ```
   (Use the exact path G1 pinned in `design.md` §Pre-Apply Gates if it differs.)

---

## Exit criteria

### Criterion 11.1 — Seed intent → agent submits plan via `spectty_spec`; SpecPane shows it, no refresh `[REQ:acceptance-gate/criterion-1]` (M4-REQ-09, M4-REQ-18)

**Preconditions**
- App opened from the packaged `.app`; engram running; throwaway git repo ready.

**Steps**
1. Spawn a **Claude Code** (Cooperative) session pointed at the throwaway repo.
2. Seed an intent (e.g. ask the agent to "add input validation to the login form").
3. Let the agent produce a plan and call `spectty_spec` with the contract.
4. Watch the **Spec pane** (left rail of the triad).

**Observe**
- The Spec pane shows the seeded intent and a task **checklist** (one row per task) with
  each task's `TaskState` label, appearing WITHOUT clicking any refresh control (the
  SpecBus poll loop emits `spec_updated`, the pane re-renders live).

**Automated floor**
- `commands::spec::poll_change_becomes_spec_updated_with_deserialized_contract` (poll →
  `spec_updated` with the deserialized `SpecContract`).
- `crates/spectty-mcp` `spectty_spec` effect tests (upsert to `spectty/{sid}/spec`).
- vitest `SpecPane renders the live checklist from a spec_updated event without a manual
  refresh` + `triad-ipc` `listenSpecUpdated`.

**Result: ☐ PENDING**

---

### Criterion 11.2 — Plan-approval gate appears; user approves; agent unblocks; edits gated until Approved `[REQ:acceptance-gate/criterion-2]` (M4-REQ-07, M4-REQ-19, M4-REQ-10/11)

**Preconditions**
- A Cooperative session with a submitted plan (11.1 reached).

**Steps**
1. Have the agent call `spectty_approval` for the plan (it blocks, long-polling engram).
2. Watch the Pane-header badge and the SpecPane gate.
3. Click **Approve** in the SpecPane plan-approval gate.
4. Confirm the agent proceeds and begins edits.

**Observe**
- The Pane-header badge shows **Awaiting input** while the approval is pending (the
  approval poll loop emits `status_changed(AwaitingInput)` with quick_actions from the
  request `options[]` — WU-10.11, reusing the M2 status path).
- The SpecPane shows the Approve / Adjust / Reject gate.
- Clicking **Approve** invokes `approve_prompt(session_id, "plan", "approve")`, the blocked
  agent unblocks, and the gate disappears (approval resolved).
- A task only moves to `in_progress` AFTER approval (the Core gate `may_begin_edits` /
  `apply_progress` returns `GateNotApproved` while Pending).

**Automated floor**
- Core `apply_progress_blocks_in_progress_while_pending` + `may_begin_edits_true_only_
  when_approved` (the gate rule).
- `commands::spec::approve_prompt_resolves_and_unblocks_caller` +
  `approve_prompt_writes_approval_state_via_core_gate`.
- `commands::spec::pending_approval_change_maps_to_awaiting_input_with_quick_actions` +
  `resolved_approval_change_surfaces_nothing` (WU-10.11 surfacing).
- MCP `spectty_approval_long_poll_returns_resolution`.
- vitest `SpecPane approval gate calls approve_prompt and hides once resolved`.

**Result: ☐ PENDING**

---

### Criterion 11.3 — Task states update live (no refresh) `[REQ:acceptance-gate/criterion-3]` (M4-REQ-03, M4-REQ-09, M4-REQ-17, M4-REQ-18)

**Preconditions**
- A Cooperative session with an approved plan (11.2 reached).

**Steps**
1. Let the agent work through tasks, calling `spectty_spec` with updated progress as it
   advances each task (`pending → in_progress → done`).
2. Watch the SpecPane checklist WITHOUT touching any refresh control.

**Observe**
- Each task's row reflects its new `TaskState` within one poll tick (default 2s,
  `SPECTTY_POLL_MS`), no manual refresh — `done` tasks render struck-through, `in_progress`
  highlighted.

**Automated floor**
- `spec_bus` change-detection unit tests (emit once on `updated_at` advance, no re-emit on
  same `updated_at`).
- vitest `SpecPane renders the live checklist ... without a manual refresh` (task moved to
  `done` reflected live).

**Result: ☐ PENDING**

---

### Criterion 11.4 — VibeLens updates within seconds via `spectty_diff` `[REQ:acceptance-gate/criterion-4]` (M4-REQ-13, M4-REQ-15, M4-REQ-20)

**Preconditions**
- A Cooperative session in the throwaway git repo; `npx -y vibelens-mcp` reachable.

**Steps**
1. Have the agent edit one or more files, then call `spectty_diff` (the cooperative
   trigger).
2. Watch the **VibeLens panel** (right rail).

**Observe**
- Within seconds the panel shows a summary + per-file rationale for the edited files (the
  cooperative `spectty_diff` poll runs the pipeline immediately, bypassing the FileWatch
  debounce — D37). Re-editing the same file and re-triggering refreshes the explanation
  (the nonce makes consecutive triggers distinct).

**Automated floor**
- `diff_pipeline` `pipeline_explains_and_emits_once_on_change` +
  `pipeline_skips_explain_when_hash_unchanged` (hash dedup).
- MCP `spectty_diff_upserts_trigger_doc_to_canonical_key` +
  `spectty_diff_consecutive_triggers_write_distinct_docs_for_app_poll`.
- vitest `VibeLensPanel renders per-file rationale from a diff_updated event` +
  `triad-ipc` `listenDiffUpdated`.
- **Manual-only gap**: the real `show_diff_explanation` WRITE round-trip against the live
  `npx -y vibelens-mcp` (the `#[ignore]` `vibelens_real_npx_show_diff_explanation` test
  asserts the locally-built explanation is returned even when the push degrades).

**Result: ☐ PENDING**

---

### Criterion 11.5 — Per-file rationale accurate/readable `[REQ:acceptance-gate/criterion-5]` (M4-REQ-12, M4-REQ-20)

**Preconditions**
- A VibeLens explanation is showing (11.4 reached).

**Steps**
1. Read the per-file rationale rows in the VibeLens panel against the actual edits.

**Observe**
- The panel lists EACH changed file with a readable per-file rationale (not just a single
  summary line). The path is shown monospaced; the rationale beneath it.
- **Known limitation (PR-5 review F3, deferred):** a file path containing a SPACE inside a
  quoted `diff --git "a/x y" "b/x y"` header is dropped from the per-file list (the summary
  line-counts are unaffected). Note any such case; it does not fail this criterion.

**Automated floor**
- `vibelens.rs` `vibelens_adapter_builds_explanation_and_pushes_show_diff_explanation` (the
  adapter builds per-file `FileExplanation`s from the diff headers).
- vitest `VibeLensPanel renders per-file rationale` (multi-file).

**Result: ☐ PENDING**

---

### Criterion 11.6 — Restart mid-session restores spec + progress `[REQ:acceptance-gate/criterion-6]` (M4-REQ-02, M4-REQ-23)

**Preconditions**
- A Cooperative session with an approved plan and partial task progress (11.3 reached).

**Steps**
1. Quit Spectty entirely (engram keeps running — it IS the store).
2. Re-open the packaged app and re-attach / re-open the same session.
3. Watch the SpecPane on re-attach.
4. THEN, to test the degraded path: stop the engram daemon, restart Spectty, re-attach.

**Observe**
- On re-attach with engram up: the SpecPane shows the prior intent, plan, task states, and
  approval IMMEDIATELY (the D38 hydrate emits an initial `spec_updated` BEFORE the first
  poll interval — no 2s blank window).
- On re-attach with engram down: the SpecPane degrades to an empty/last-known state WITHOUT
  crashing.

**Automated floor**
- `commands::spec::restart_hydrate_emits_initial_spec_updated`.
- `EngramAdapter` degrade-when-down contract tests (`PersistenceError::Backend`, never
  panic).
- vitest `SpecPane hydrates from getSpec on mount when a spec is already stored`.

**Result: ☐ PENDING**

---

### Criterion 11.7 — Generic agent degrades gracefully (PTY-scrape + FileWatcher) `[REQ:acceptance-gate/criterion-7]` (M4-REQ-15, M4-REQ-18, M4-REQ-14)

**Preconditions**
- A throwaway git repo. Use a **Generic**-tier agent (no `spectty_*` injection), e.g. a
  plain `bash -l` shell, or any non-cooperative program.

**Steps**
1. Spawn a **Generic** session in the repo.
2. Observe the SpecPane.
3. Edit files inside the session (e.g. `echo x >> README.md`), WITHOUT any `spectty_diff`
   call.
4. Observe the VibeLens panel after the debounce window (~500ms–1s).
5. (Degrade leg) Make `npx -y vibelens-mcp` unavailable (e.g. offline) and edit again.

**Observe**
- The SpecPane shows the COARSE generic badge ("progress is scraped from the terminal — no
  structured plan"), NOT a precise checklist (graceful degradation).
- VibeLens updates from the debounced **FileWatcher** fallback (the generic tier has no
  cooperative trigger). The `.git/`-internal churn does NOT self-trigger the pipeline
  (WU-8.0 filter).
- With VibeLens unavailable / a git failure: the panel shows an **"unavailable" / parse-error**
  state, never a blank panel or crash; the session keeps working.

**Automated floor**
- `file_watch` `notify_file_watcher_debounces_burst_into_one_batch`;
  `diff_pipeline` `git_internal_paths_do_not_trigger_workspace_edits_do` +
  `pipeline_degrades_on_git_failure` / `pipeline_degrades_on_explainer_error`.
- `vibelens.rs` `vibelens_adapter_degrades_on_unreachable_or_parse_fail`.
- vitest `SpecPane shows a coarse scraped badge for a generic-tier session` +
  `VibeLensPanel shows an unavailable indicator for a degraded explanation and does not
  crash`.

**Result: ☐ PENDING**

---

### Criterion 11.8 — Triad layout visible per session `[REQ:acceptance-gate/criterion-8]` (M4-REQ-22)

**Preconditions**
- Any active session (Cooperative or Generic).

**Steps**
1. With a session active, look at the whole window.

**Observe**
- The Spec pane (left), the Terminal (center), AND the VibeLens panel (right) are ALL
  visible simultaneously for the one session — no navigating away to see one region.

**Automated floor**
- vitest `TriadLayout renders the spec pane, the terminal region, and the vibelens panel
  for one session` (+ the generic variant).
- `App` routing test (exactly one `.terminal-pane` after spawn — the triad wraps the
  existing `SessionTerminal`).

**Result: ☐ PENDING**

---

### Criterion 11.9 (best-effort, ungated) — Windows smoke

If a Windows host is available, smoke-test that the app launches and a session spawns with
the spec/diff pipelines wired. Failure does **NOT block M4** — informational only.

**Result: ☐ PENDING (best-effort)**

---

## Exit-criteria coverage summary

| # | Criterion | Automated floor (CI) | Manual-only delta |
|---|-----------|----------------------|-------------------|
| 11.1 | Seed → plan via `spectty_spec`; SpecPane shows it, no refresh | poll→`spec_updated`; `spectty_spec` effect; SpecPane vitest | Real Claude Code plan submission |
| 11.2 | Approval gate appears; approve; edits gated until Approved | Core gate; `approve_prompt` resolve; approval→`AwaitingInput`; SpecPane gate vitest | Real blocked agent unblocking |
| 11.3 | Task states update live (no refresh) | `spec_bus` change-detection; SpecPane live vitest | Real multi-task progress stream |
| 11.4 | VibeLens < seconds via `spectty_diff` | pipeline dedup; `spectty_diff` doc; VibeLensPanel vitest | Real `npx vibelens-mcp` WRITE round-trip |
| 11.5 | Per-file rationale accurate/readable | adapter builds per-file explanations; vitest | Real diff readability; quoted-path F3 limitation |
| 11.6 | Restart restores spec + progress | hydrate test; EngramAdapter degrade; SpecPane mount vitest | Real quit/relaunch; engram-down leg |
| 11.7 | Generic degrades (PTY-scrape + FileWatcher) | debounce; `.git/` filter; degrade tests; generic vitest | Real generic session + VibeLens-down |
| 11.8 | Triad layout visible per session | TriadLayout + App routing vitest | Real packaged window |
| 11.9 | Windows smoke (ungated) | — | Manual only; does not gate M4 |

Automated vs manual: 11.1/11.3/11.8 have a strong automated floor. 11.2/11.4/11.6/11.7 are
manual-dominant for the real-CLI / real-engram / real-VibeLens legs. 11.5 is manual for diff
readability.

---

## Deferred items

- **VibeLens quoted-path parsing (PR-5 review F3)** — `vibelens.rs` `changed_files` splits
  on ` b/`, so a path containing a space inside a quoted `diff --git "a/x y" "b/x y"` header
  is dropped from the per-file annotations (summary line-counts unaffected). Data-only fix
  in the adapter; does not touch Core. Tracked in `tasks.md` WU-8 follow-up.
- **engram session-row memoization (PR-1 review Finding 5)** — confirm `ensure_session` is
  memoized so the 2s production poll/effect loop does not double session-row write traffic.
  Verify under the real loop during the acceptance run.
- **state-file side-channel (D30)** — deferred by design; the 2s engram poll satisfies
  criterion 3. Reopen only on acceptance evidence that 2s is too slow.

---

## Acceptance gate (WU-11)

All macOS criteria (11.1–11.8) must pass for **M4 acceptance = PASS**; 11.9 (Windows) is
informational. Record real-run results in the per-criterion result lines and the table above
when executed against a live Claude Code + engram + VibeLens stack.

**M4 ACCEPTANCE = PENDING.** The automated floor runs green on the PR-6 branch
(`cargo test --workspace`; `pnpm -C ui test` = 84 specs; fmt, clippy `-D warnings`,
`cargo deny check bans` = `bans ok`). It guards every mechanical core of the eight criteria;
the manual run validates the real-CLI / real-engram / real-VibeLens gaps that synthetic
fixtures cannot reach.
