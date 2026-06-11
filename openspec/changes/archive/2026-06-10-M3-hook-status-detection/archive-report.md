# Archive Report: M3 — Hook-Based Status Detection

**Change**: M3-hook-status-detection
**Project**: spectty (repo: ai-terminal)
**Artifact store**: hybrid (filesystem `openspec/` + Engram)
**Archived**: 2026-06-10
**Status**: ARCHIVED — SDD cycle complete, change CLOSED.
**Verdict**: PASS WITH WARNINGS (0 CRITICAL, 1 WARNING, 2 SUGGESTIONS; the WARNING is a spec-authoring inconsistency, not an implementation defect).

## Traceability (Engram observation IDs)

| Phase / artifact | Topic key | Obs ID |
|---|---|---|
| Exploration | `sdd/M3-hook-status-detection/explore` | #827 |
| Proposal | `sdd/M3-hook-status-detection/proposal` | #828 |
| Spec (delta) | `sdd/M3-hook-status-detection/spec` | #830 |
| Design | `sdd/M3-hook-status-detection/design` | #829 |
| Tasks | `sdd/M3-hook-status-detection/tasks` | #831 |
| Apply progress | `sdd/M3-hook-status-detection/apply-progress` | #832 |
| Verify report | `sdd/M3-hook-status-detection/verify-report` | #863 |
| Archive report | `sdd/M3-hook-status-detection/archive-report` | #864 (Engram) |

Implementation merged to `main` @ `b39b40f`. Manual acceptance run: 2026-06-10 (macOS aarch64, Claude Code v2.1.172).

## What M3 delivered

Replaces TUI-scraping status source with Claude Code's official hooks mechanism (deterministic
lifecycle events), keeping scraping as fallback to close the async gap. Closes the M2 L2
bundling gap by shipping `spectty-hook` and `spectty-mcp` as Tauri `externalBin` sidecars.
Delivered across **11 work units (WU-1..WU-11)** in **5 stacked PRs (#21–#34)** plus 3 acceptance fixes
(PR-30: gate bounce suppression, PR-31: whitespace-insensitive patterns, PR-32: core row
amendment). Strict TDD throughout.

### Headline changes

- **`spectty-hook` standalone binary sidecar**: stateless, atomic-write JSON state file per session,
  accepts `--event {Submit, Stop, Permission, SessionEnd, StopFailure}`, reads `$SPECTTY_SESSION_ID`,
  exit non-zero on missing env / absent runtime dir / bad args.
- **`ClaudeSettingsProvisioner` (2nd ProvisioningPort impl)**: manages ONLY the `hooks` key of
  `~/.claude/settings.json` (Global) or `{project}/.claude/settings.json` (Project), reuses M2
  atomic-write seam + .spectty.bak + foreign-key preservation R7.
- **Hook-status mapping**: pure table `{Working,Ready,NeedsInput,Finished,Failed} → Observed`.
- **run_signal_loop hook-augmentation**: QUIESCE(200ms) poll on state file (consume-once via
  monotonic `ts` strictly-greater predicate), hook-sourced `Observed` feeds same `observe_and_diff`
  path as scraping, `detect_status` stays pure PTY-only.
- **Lifecycle**: spawn injects BOTH provisioners BEFORE PTY, close kills PTY then retracts BOTH
  then deletes state file; runtime dir created on spawn; stale state file opportunistically
  swept before loop.
- **Bundling**: both `spectty-hook` and `spectty-mcp` configured as `externalBin` in
  `tauri.conf.json`; `spectty_hook_command()` mirrors `spectty_mcp_command()` for path resolution.
- **Core quarantine intact**: zero additions to `spectty-core` (ProvisioningPort trait unchanged,
  `transition()` unchanged save the M3 amendment, detect_status unchanged). Only adapters +
  sidecar + UI harness wiring.

## Final test counts

- **Rust**: `cargo test --workspace` → **251 tests, 0 failed** (src-tauri 52, hook_integration 2
  [#[cfg(unix)] path-agreement + real-PTY hook→Idle], adapters 133, core 39, spectty-hook 1+11,
  spectty-mcp 11+2).
- **UI**: `pnpm -C ui test` → **64 tests, 10 files, 0 failed**.
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` no warnings;
  `cargo deny ... check bans` → `bans ok` (core quarantine intact); `cargo build -p spectty-hook` success.

## Acceptance gate (2026-06-10, macOS, manual run)

All five gating criteria **PASS**:

1. **11.1 — Bypass-permissions session**: `Running → Idle` driven by hook `Stop` within 200ms,
   no scraping dependence. Primary regression fix confirmed.
2. **11.2 — Managed hooks + foreign keys**: `settings.json` contains managed hook rows
   (UserPromptSubmit, Stop, Notification with permission_prompt matcher, SessionEnd), foreign
   keys intact (user hooks, permissions, env, attribution), no Spectty entry in `.claude.json`
   `mcpServers` (hooks live in `settings.json`, MCP server in `.claude.json`).
3. **11.3 — Slice 2 lifecycle** (permission → AwaitingInput, SessionEnd → Completed, StopFailure → Error):
   DEFERRED. `SubagentStop` has no failure discriminator; Error leg documented as deferred to M4.
   Approval-hook missing; `(AwaitingInput, Working)` scraped leg documented as open.
4. **11.4 — Close cleans up**: PTY killed, managed hooks retracted, state file deleted, foreign
   hooks/keys survive.
5. **11.5 — Packaged build + sidecars**: Contents/MacOS/ has spectty + spectty-mcp + spectty-hook,
   both resolve and Claude Code starts.

## Design deviations — all documented + test-pinned

| Deviation | Justification | Test pin |
|-----------|---------------|----------|
| StopFailure/Error deferred | SubagentStop no failure discriminator | `production_hook_events` len==4 |
| PR #30 gate `(Idle/Starting, Working)` | Claude TUI redraws at idle → scraped Working bounced hook Idle | `hook_gate_active_signal_does_not_bounce_idle_to_running` |
| PR #31 whitespace-insensitive patterns | Ink emits space runs as CSI cursor-forward | `claude_patterns_contain_no_whitespace` |
| PR #32 core `(AwaitingInput, Ready) => Idle` amendment | Resolved dialog lingers, re-pins AwaitingInput | core test + 3 hook_gate tests |

## Specs promoted to / extended in the living baseline

The M3 delta specs were merged into the project's living baseline at `openspec/specs/`. Two new
capability specs created, two baseline extended:

| Capability | Baseline spec file | Action |
|---|---|---|
| spectty-hook-sidecar | `openspec/specs/spectty-hook-sidecar/spec.md` | Created (2 requirements) |
| provisioning-port | `openspec/specs/provisioning-port/spec.md` | Extended (M3 ADDED: ClaudeSettingsProvisioner for settings.json hooks; trait unchanged) |
| agent-runner | `openspec/specs/agent-runner/spec.md` | Extended (M3 ADDED: hook-status-mapping, pipeline-augmentation, lifecycle, bundling capabilities) |
| agent-session-ui | (no change) | (badge now fed by hooks; no new spec requirements) |

The original M3 delta specs are preserved in this archive folder under `specs/` as the historical
record.

## Verify verdict (obs #863)

PASS WITH WARNINGS, **0 CRITICAL**. Implementation complete, fully tested (315 passing tests:
251 Rust + 64 UI; clippy/fmt/deny clean), manual acceptance gate passed macOS 11.1–11.5. Single
WARNING (W1): spec scenario in `pipeline-augmentation` contradicts the M2 baseline core table
(spec-authoring bug, not implementation defect). Implementation correctly preserves unchanged
M2 behavior. Does NOT block archive.

## CARRIED-FORWARD to M4

| Item | What M4 must do |
|---|---|
| **W1 / spec correction** | Doc-only fix: correct `pipeline-augmentation` spec scenario (Starting, Ready) to match M2 baseline and confirm M3 real behavior. |
| **StopFailure/Error leg** | Requires failure discriminator on SubagentStop hook event (Claude hooks API enhancement); deferred. |
| **Approval hook / (AwaitingInput, Working)** | User approval hook needed to close the `(AwaitingInput, Working)` scraping-only leg; M4 or later. |
| **Boot-time orphan reconciliation** | Full settings.json + state-file sweep; M3 mitigations: .spectty.bak + harmless stale state files + opportunistic pre-spawn sweep. |
| **Notify-crate filesystem watching** | State-file format chosen for notify upgrade (no hook-command / provisioner change); upgrade M4+. |
| **HTTP callback IPC** | Port negotiation needed; M4+. |

## Archive contents

- `proposal.md`
- `specs/` (original M3 delta specs — historical record)
  - `spec.md` (change-level delta)
  - `hook-provisioning.md`
  - `spectty-hook-sidecar.md`
  - `hook-status-mapping.md`
  - `pipeline-augmentation.md`
  - `lifecycle.md`
  - `bundling.md`
- `design.md`
- `tasks.md` (11 work units, all gates ticked; PR #21–#34 including 3 acceptance fixes)
- `acceptance.md` (5 manual exit criteria, all PASS except StopFailure deferred)
- `verify-report.md`
- `archive-report.md` (this file)

## SDD cycle complete

M3-hook-status-detection was explored → proposed → specified → designed → tasked → applied
(5 stacked PRs #21–#34, Strict TDD, 3 acceptance-fix PRs caught via manual gate) → verified
(PASS WITH WARNINGS, 0 CRITICAL) → archived. The change is CLOSED. Next: **M4** (carry-forward
items above: spec W1 correction, StopFailure discriminator, approval hook, full boot sweep,
notify upgrade, HTTP callbacks).
