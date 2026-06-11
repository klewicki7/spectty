# M3 — Hook-Based Status Detection — Verification Report

> SDD verify phase. Strict TDD Mode ACTIVE. HEAD `cf5d5f5`, branch `main`.
> Reads: spec (obs #830 + specs/*), tasks (obs #831 + tasks.md), apply-progress (obs #832),
> design.md, acceptance.md. Verdict computed from source inspection + real test execution.

## Verdict: PASS WITH WARNINGS

All 11 work units complete and checked `[x]`. All gating tests pass (251 Rust + 64 frontend = 315).
All five manual acceptance criteria (11.1–11.5) PASS on macOS; 11.6 SKIP (no Windows host).
The three acceptance fixes and the StopFailure deferral are documented AND test-pinned.
One WARNING: a spec scenario in `pipeline-augmentation` is internally inconsistent with the
M2 baseline core table (spec authoring bug, not an implementation defect).

---

## Completeness

| Area | Result | Detail |
|------|--------|--------|
| Work units | ✅ 11/11 | WU-1..WU-11 all `[x]` in tasks.md |
| PR-2 adversarial fixes | ✅ 5/5 | C1/C2/C3/W1/W2 all `[x]` |
| Acceptance criteria | ✅ 5/5 gating | 11.1–11.5 PASS; 11.6 SKIP (ungated) |
| Files on disk | ✅ | spectty-hook/{handler,lib,main,runtime_dir}, adapters/hook/{state,reader,mod}, adapters/provision/settings_provisioner, all present |
| Git working tree | ✅ clean | no uncommitted changes |

## Build / Tests / Static analysis (real execution)

| Gate | Result | Evidence |
|------|--------|----------|
| `cargo fmt --all -- --check` | ✅ PASS | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ PASS | `Finished`, no warnings |
| `cargo build -p spectty-hook` | ✅ PASS | binary built (gotcha respected before tests) |
| `cargo test --workspace` | ✅ PASS | 251 tests, 0 failed |
| `pnpm -C ui test` | ✅ PASS | 64 tests, 10 files, 0 failed |
| `cargo deny --manifest-path crates/core/Cargo.toml check bans` | ✅ PASS | `bans ok` (core quarantine intact) |

Rust test breakdown: src-tauri lib 52 + hook_integration 2 (`#[cfg(unix)]` path-agreement D25
+ real-PTY hook→Idle) + adapters 133 + core 39 + spectty-hook 1+11 + spectty-mcp 11+2 = 251.

## TDD Compliance

| Check | Result | Details |
|-------|--------|---------|
| TDD evidence reported | ✅ | apply-progress + tasks.md per-WU RED→GREEN steps |
| All tasks have tests | ✅ | every `[unit]` WU pairs a named RED test with GREEN impl |
| RED confirmed (tests exist) | ✅ | named tests located in source for all WUs |
| GREEN confirmed (tests pass) | ✅ | 251/251 Rust + 64/64 frontend pass on execution |
| Triangulation adequate | ✅ | event_to_observed 5-event table; reader 8 consume-once cases; gate 5 legs |
| Safety net for modified files | ✅ | core/adapters pre-existing suites stayed green at each WU |

**TDD Compliance: 6/6 checks passed.**

## Assertion Quality

Scanned hook/reader.rs, hook/state.rs, settings_provisioner.rs, json_namespace.rs,
spectty-hook/handler.rs, session_runtime.rs hook tests.

- No tautologies (`assert!(true)` / `assert_eq!(1,1)`) anywhere.
- Assertions verify real values: specific `Observed`/`HookEvent` variants, consume-once
  strict-greater semantics, D23 session-id mismatch rejection, no-double-emit, gate suppression.
- Descriptive failure messages throughout.

**Assertion quality: ✅ All assertions verify real behavior.** 0 CRITICAL, 0 WARNING.

## Spec compliance matrix (by capability)

| Capability | Reqs | Status | Covering tests |
|------------|------|--------|----------------|
| hook-provisioning | 4 | ✅ | json_namespace round-trip/foreign-key (46 fns), settings_provisioner scope+backup (19), settings_path_for_scope |
| spectty-hook-sidecar | 2 | ✅ | handler.rs (10) + main.rs (11): atomic write, missing-env/dir non-zero, all 5 events |
| hook-status-mapping | 2 | ✅ | event_to_observed 5-event table; Notification matcher constant; no-matcher events |
| pipeline-augmentation | 3 | ⚠️ | reader consume-once (25), run_signal_loop hook tests; one spec scenario inconsistent (W1) |
| lifecycle | 4 | ✅ | spawn both-inject-before-PTY, close kill→retract-both→delete, tolerate-absent, stale sweep |
| bundling | 2 | ✅ | externalBin via tauri.bundle.conf.json; spectty_hook_command(); D25 path-agreement integration |
| acceptance-gate (manual) | 5 | ✅ | 11.1–11.5 PASS 2026-06-10/KL; documented in acceptance.md |

## Design-deviation evaluation (the 3 acceptance fixes + StopFailure deferral)

All deviations are JUSTIFIED, DOCUMENTED, and TEST-PINNED:

| Deviation | Justification | Documented | Test pin |
|-----------|---------------|------------|----------|
| StopFailure/Error leg deferred | SubagentStop has no failure discriminator; would flip healthy sessions to Error | design §3.4, acceptance §Deferred, ADR 0004 §M3, lib.rs `production_hook_events` doc | `production_hook_events` len==4 + SubagentStop-absent assertions |
| PR #30 gate `(Idle/Starting, Working)` | Claude TUI redraws at idle prompt → scraped Working bounced hook Idle→Running | acceptance.md PR table, gate rustdoc | `hook_gate_active_signal_does_not_bounce_idle_to_running` |
| PR #31 whitespace-insensitive patterns | Ink emits space runs as cursor-forward CSI → spaced patterns never matched | acceptance.md, design context | `claude_patterns_contain_no_whitespace` |
| PR #32 core row `(AwaitingInput, Ready) => Idle` + gate `(_, NeedsInput)` & `(AwaitingInput, Ready)` | resolved-dialog text lingers in window re-pinning AwaitingInput | acceptance.md, core row rustdoc | core `((AwaitingInput, Ready), Idle)`, `hook_gate_scraped_needs_input_is_suppressed`, `hook_gate_scraped_ready_does_not_resolve_awaiting_input`, `hook_stop_resolves_awaiting_input_to_idle` |

Architectural soundness confirmed: hook events flow UNgated through `emit_hook_if_present` →
`apply_observed` (D24 hook-first), while `emit_scraping_guarded` gates only the scraping path.
Hooks remain authoritative; scraping fills the async gap. `detect_status` stays pure PTY-only
(`fn detect_status(&self, signal: &OutputSignal) -> Option<Observed>` — unchanged, D24 lock).
Core quarantine intact (`cargo deny ... bans ok`).

---

## Findings

### CRITICAL — none

### WARNING

**W1 — Spec scenario contradicts the M2 baseline core table (spec authoring bug, NOT impl).**
`specs/spec.md` capability `pipeline-augmentation`, scenario
"Hook-derived Ready observation is rejected by transition if current is Starting"
asserts `transition(Starting, Ready)` MUST return `Starting` unchanged, claiming
"Starting → Idle is the only legal first step." This is internally inconsistent: the
named legal step IS `Starting → Idle`, and the actual M2 baseline core table has
`(Starting, Ready) => Idle` (`crates/core/src/entities/agent_status.rs:66`, pinned by core
test `((Starting, Ready), Idle)`). The implementation correctly preserves the UNCHANGED M2
behavior (per the design's hard invariant "core gains NOTHING"). The defect is in the spec
prose, which states an unsatisfiable expectation against unmodified core. No code change is
warranted; recommend correcting the spec scenario in a doc-only follow-up so the artifact
trail is accurate. Does NOT block archive.

### SUGGESTION

**S1 — Local-dev test gotcha is real and should stay visible.** `tauri build` clobbers
`target/debug/spectty-hook` with the RELEASE sidecar, breaking the WU-9 integration test
(release ignores `SPECTTY_RUNTIME_DIR`). Mitigation (`cargo build -p spectty-hook` before
`cargo test`) is documented in apply-progress + acceptance.md §11.5. Consider a Makefile/
cargo-xtask target wrapping the correct order to prevent future re-discovery. CI is unaffected.

**S2 — `(AwaitingInput, Working)` scraping leg intentionally open.** No hook fires on user
approval, so scraped Working is the only signal that resumes a turn from AwaitingInput. This
is deliberate and documented in the gate rustdoc; flagged only so a future hook-coverage
milestone (M4) revisits whether an approval hook can close it.

---

## Verdict

**M3 = PASS WITH WARNINGS.** Implementation is complete, fully tested (315 passing tests,
clippy/fmt/deny clean), and the manual acceptance gate passed on macOS. The single WARNING
(W1) is a spec-text inconsistency, not an implementation defect, and does not block archive.
Recommend a doc-only spec correction for W1 either before or alongside archive.

**Next recommended: sdd-archive.**
