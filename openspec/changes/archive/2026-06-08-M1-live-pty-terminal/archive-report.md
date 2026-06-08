# Archive Report: M1 — Live PTY Terminal

**Change**: M1-live-pty-terminal
**Project**: ai-terminal (Spectty)
**Artifact store**: hybrid (filesystem `openspec/` + Engram)
**Archived**: 2026-06-08
**Status**: ARCHIVED — SDD cycle complete, change CLOSED — with one explicitly
user-deferred known-open item (manual visual acceptance, see below).

## Traceability (Engram observation IDs)

| Phase / artifact | Topic key | Obs ID |
|---|---|---|
| Exploration | `sdd/M1-live-pty-terminal/explore` | 783 |
| Proposal | `sdd/M1-live-pty-terminal/proposal` | 784 |
| Spec (delta) | `sdd/M1-live-pty-terminal/spec` | 785 |
| Design | `sdd/M1-live-pty-terminal/design` | 786 |
| Tasks | `sdd/M1-live-pty-terminal/tasks` | 787 |
| Apply progress (PR1+PR2+PR3+PR5 merged) | `sdd/M1-live-pty-terminal/apply-progress` | 788 |
| R1 resolution (Channel `Vec<u8>` → `number[]`) | (discovery) | 792 |
| Verify report (PR1+PR2+PR3+PR5, all PASS) | `sdd/M1-live-pty-terminal/verify-report` | 790 |
| Bug record (R3 quiescent-flush, CRITICAL) | `sdd/m1-live-pty-terminal/bug-quiescent-flush` | 793 |
| Pattern (mpsc recv_timeout + pure forward_step) | `sdd/m1-live-pty-terminal/pattern-quiescent-flush` | 794 |
| Archive report | `sdd/M1-live-pty-terminal/archive-report` | (this) |

## What M1 delivered

A real terminal in the UI, backed by a real PTY, rendering live output — built entirely on
the M0 hexagonal skeleton with the Core left untouched and the engram quarantine intact.
Shipped across four merged PRs (PR4 = a manual-acceptance gate, no code):

- **PR1 — `crates/adapters` PTY slice** (merged #2 / `f190233`). `portable-pty` 0.9.0
  landed in the adapter layer; pure `Coalescer` (size-OR-time batcher, injected `Instant`,
  5 tests); `PtySpawnConfig` + per-OS `default_shell`; `PtyTransport` fake seam (NOT a Core
  port); `PtyAdapter` over `portable-pty`.
- **PR2 — `src-tauri` PTY bridge** (merged #3 / `329abfb`). `PtyState` holding
  `Box<dyn PtyTransport>`; idempotent one-shot-latch `shutdown()`; the four lifecycle
  commands wired into `generate_handler!` + `.manage(PtyRegistry)`; output over a per-spawn
  `ipc::Channel<Vec<u8>>`; `pty_exit { code }` via the v2 `Emitter`. A real-PTY
  `#[cfg(unix)]` test closed PR1's W1.
- **PR3 — terminal UI** (merged #4 / `47d6746`). `@xterm/xterm` 6 + `addon-fit` 0.11 +
  `addon-clipboard` 0.2; `useTerminal` hook (spawn-on-mount, `onData`→`send_input`,
  fit→`pty_resize`, Channel→`term.write`, dispose+`pty_kill` on unmount, React 19 named
  imports, no manual memo); `decodeChannelBytes` (the R1 fix); `ipc.test.ts` (7) +
  `useTerminal.test.ts` (5).
- **PR5 — R3 quiescent-flush fix** (branch `fix/m1-pty-quiescent-flush`, base `47d6746`;
  committed `b5114f9`). ONE file: `src-tauri/src/commands/pty.rs` (+302/-40). See the
  notable bug section below.

## R1 resolution (carried risk, closed in PR3) — obs 792

R1 (the exact JS payload shape of a Tauri v2 `ipc::Channel<Vec<u8>>`) was definitively
resolved by reading the installed `tauri` 2.11.2 source: a bare `Vec<u8>` goes through
`serde_json::to_string` and arrives on the JS side as a **`number[]`**, NOT a `Uint8Array`
and NOT an `ArrayBuffer`. The fix shipped in `ui/src/pty/ipc.ts`: `decodeChannelBytes`
handles `number[]` (the actual M1 path), `ArrayBuffer` (the raw-`Response` fallback), and
`Uint8Array` (passthrough) defensively, then feeds the decoded `Uint8Array` straight into
xterm's `term.write`. No base64 fallback was needed. This wire-shape note is now folded
into the `pty-bridge` and `terminal-ui` baseline specs.

## Notable bug caught and fixed — R3 (quiescent flush)

A design-flagged risk (R3) that apply DEFERRED silently shipped, and only a human running
the real app at the PR4 manual-acceptance gate caught it. Recorded as obs 793 (bugfix) +
obs 794 (pattern).

- **The bug**: `spawn_read_thread` called `Coalescer::drain_due` only inside the `Ok(n)`
  read-return branch. When the child wrote a small burst and then blocked on stdin
  (a quiescent PTY), `reader.read()` blocked indefinitely, the time-based flush never
  fired, and the buffered bytes were STRANDED until the next read unblocked. Symptoms (all
  one root cause): (1) atuin/fancy prompts erroring on the `ESC[6n` DSR/CPR cursor query
  ("cursor position could not be read"); (2) prompt rendering "shifting"; (3) tab-completion
  output not appearing.
- **The fix (PR5)**: decoupled reading from coalescing via `std::sync::mpsc` + `recv_timeout`.
  A read thread does blocking `read()` and forwards each slice over the channel; a forwarder
  thread owns the `Coalescer` + `Channel<Vec<u8>>` + `AppHandle` and loops on
  `rx.recv_timeout(FLUSH_INTERVAL)` — `Ok(bytes)`→push (size flush); `Err(Timeout)`→`drain_due`
  (THE fix — flushes stranded bytes within the interval while the PTY is silent);
  `Err(Disconnected)`→`drain_all` + emit `pty_exit` + break. The per-message decision was
  extracted into a pure `forward_step(...) -> ForwardAction` so the time-flush is
  deterministically unit-testable with NO PTY and NO sleep. Constants (8KB/8KB/8ms)
  unchanged; `pty_state.rs` unchanged (the read thread owns+joins the forwarder, preserving
  the single-`JoinHandle` shutdown path).
- **R3 status: CLOSED** by a non-tautological regression test
  (`quiescent_timeout_flushes_stranded_bytes_within_interval`) plus a real-PTY e2e
  (`real_pty_lone_small_write_is_not_stranded_while_quiescent`, `printf 'Q'; exec cat`,
  bounded 5s deadline so it cannot hang CI). This is now a SHIPPED requirement captured in
  the `pty-adapter` baseline ("Buffered output MUST flush within a bounded interval even when
  the PTY is quiescent").
- **Process lesson**: a design-flagged risk deferred at apply MUST be closed by an automated
  test OR explicitly re-surfaced before manual acceptance. Treat deferred design risks as
  must-verify items.

## Gates and verification (final verdict — obs 790)

Four adversarial sdd-verify passes (PR1, PR2, PR3, PR5), each re-run from source, all
**PASS**. PR5 verdict: **0 CRITICAL / 0 WARNING / 2 SUGGESTION**. All gates green on a
forced clean rebuild:

- `cargo fmt --all -- --check` → PASS
- `cargo clippy --workspace --all-targets -- -D warnings` → PASS (forced clean to defeat
  content-hash cache)
- `cargo test --workspace` → PASS (22 tests: 10 spectty + 12 adapters; real-PTY tests do not
  hang)
- `cargo deny --manifest-path crates/core/Cargo.toml check bans` → PASS (`bans ok`)
- `cargo build -p spectty` → PASS

The quarantine held: `git diff --name-only main...HEAD` for PR5 = `src-tauri/src/commands/pty.rs`
ONLY; `core/Cargo.toml` = `serde` + `thiserror`; `portable-pty` only in adapters + src-tauri.
CI green on all merged PRs.

Carried-forward suggestions (non-blocking): S-PR5-1 (hot-path `Vec` alloc per read to cross
the mpsc — negligible for M1 interactive use; revisit only under a future high-throughput
repaint storm); S-PR5-2 (VibeLens `show_diff_explanation` was not available in the apply
context — run on `git diff HEAD` before push or record exception; carried from PR1/PR2/PR3).

## KNOWN-OPEN ITEM (user-deferred — NOT silently omitted)

**PR4 / WU-7 manual visual acceptance was NOT re-confirmed in the running app before this
archive.** The user explicitly chose to archive M1 on the strength of the automated gates +
4 adversarial sdd-verify passes + green CI, and will run the manual re-test separately. The
outstanding manual acceptance, to be run via `pnpm tauri dev` on macOS, covers:

- The three roadmap exit criteria: open a shell and run `vim`, `htop`, and
  `git log --oneline --graph` (each renders + behaves); resize the window (PTY + rendering
  track the new size via SIGWINCH); scrollback retained beyond one screen.
- The three R3 symptoms confirmed fixed: atuin UP-arrow no longer errors (DSR/CPR `ESC[6n`
  now flushes), prompt renders without "shift", typing echoes promptly.
- Tab-completion now appears.

**Residual caveat (flagged, unconfirmed)**: if Tab completion STILL fails after the R3 flush
fix, it is a SEPARATE secondary FRONTEND issue — the webview/xterm capturing the Tab keydown
for focus navigation (`attachCustomKeyEventHandler` / `preventDefault`) before it reaches the
PTY — NOT the flush. The R3 fix delivers the shell's completion output once produced but does
not stop the browser from swallowing the Tab keydown. This would need a separate FE focus
investigation if observed. It is recorded here as possible, not confirmed.

## Specs promoted to / extended in the living baseline

The M1 delta spec (obs 785) was merged into the project's living baseline at
`openspec/specs/`. Three new capability specs created, one baseline extended:

| Capability | Baseline spec file | Action |
|---|---|---|
| pty-adapter | `openspec/specs/pty-adapter/spec.md` | Created (6 requirements, incl. the R3 bounded-quiescent-flush requirement) |
| pty-bridge | `openspec/specs/pty-bridge/spec.md` | Created (3 requirements; R1 wire-shape note folded in) |
| terminal-ui | `openspec/specs/terminal-ui/spec.md` | Created (6 requirements; R1 decode note folded in) |
| hexagonal-core | `openspec/specs/hexagonal-core/spec.md` | Extended (1 new guard requirement: Core gains no PTY/runtime dependency) |

The original M1 delta spec is preserved in this archive folder under `spec.md` as the
historical record.

## What M2 inherits

1. The **registry-shaped `tauri::State`** (`Mutex<HashMap<PtyId, PtyState>>`, one entry in
   M1) is the seam that becomes the M2 `SessionRegistry` without a rewrite.
2. The **`PtyTransport` fake seam** in adapters (NOT a Core port) is the extraction point for
   a future `PtyPort` trait when M2's `AgentRunner` needs it — deferred per YAGNI.
3. The **pure `Coalescer` + pure `forward_step`** pattern (deterministic time-flush testing,
   no PTY / no sleep) is the template for any future time-based output logic, including the
   M2 `OutputSignal` quiesce window.
4. The **read-thread / forwarder lifecycle** (single `JoinHandle`, kill-precedes-join,
   `drop(tx)`→Disconnected→`pty_exit`) is the established teardown pattern for per-session
   threads.

## Deferred items (remain deferred to M2+)

`AgentRunner` + per-agent runner → M2; `AgentStatus` state machine → M2; `OutputSignal`
ANSI-strip-for-status decode path (`text_window`, quiesce window) → M2; `SessionRegistry`
in Core → M2; `Provisioner` / `ProvisioningPort` → M2; `PtyPort` trait in Core → M2;
`LaunchSpec` as a named agent type → deferred; multi-session UI (tabs, panes, switcher) →
M4; real engram HTTP client → M3; `GitPort` → M4; `NotifierPort` → M5.

## Archive contents

- `explore.md`
- `proposal.md`
- `spec.md` (original M1 delta — historical record)
- `design.md`
- `tasks.md` (7 work units; WU-1..WU-6 complete, WU-7 manual acceptance user-deferred)
- `verify-report.md` (PR1 + PR2 + PR3 + PR5 sections, all PASS)
- `archive-report.md` (this file)

## SDD cycle complete

M1-live-pty-terminal was explored → proposed → specified → designed → tasked → applied
(PR1/PR2/PR3 + PR5 R3 fix, Strict TDD throughout) → verified (4 adversarial passes, all
PASS, 0 CRITICAL) → archived. The change is CLOSED, with the single manual-visual-acceptance
item explicitly recorded as user-deferred. Next: **M2 — Agent Runner & Status**.
