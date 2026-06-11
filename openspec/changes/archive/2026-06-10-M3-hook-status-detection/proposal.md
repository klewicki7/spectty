# M3 — Hook-Based Status Detection — Proposal

> SDD propose phase. Consumes `sdd/M3-hook-status-detection/explore` (obs #827) and builds
> directly on the M2 provisioner (`crates/adapters/src/provision/{claude_provisioner.rs,
> json_namespace.rs}`, the Core `ProvisioningPort`) plus the M2 status pipeline
> (`run_signal_loop` / `observe_and_diff` / `transition`). Continues the M2 D-series ADRs at D21.
> Drives `sdd-spec` and `sdd-design`. Artifact store: HYBRID
> (engram `sdd/M3-hook-status-detection/proposal` + this file).

## Intent

**What problem.** Spectty's only source of agent lifecycle state is TUI scraping:
`ClaudeCodeRunner::detect_status` scans the PTY `text_window` for a hand-rolled pattern table,
falling back to a quiescence stopgap (`is_active == false → Observed::Ready`) shipped in M2.
This is brittle by construction, and it has already produced a **real, user-hit bug**: when
Claude Code runs with bypass-permissions (`--dangerously-skip-permissions` style) the TUI no
longer renders the prompt/permission text the scraper keys on, so the status badge gets
**stuck on Running** and never returns to Idle after a turn ends. Scraping a rich TUI for
machine state is fighting the tool — every Claude Code version bump (v2.x reflows the UI), every
permission mode, every theme can silently break the pattern table.

Anthropic ships an **official, structured lifecycle mechanism for exactly this**: hooks
(docs.claude.com/docs/en/hooks). Hooks fire deterministic events (`UserPromptSubmit`, `Stop`,
`Notification`, `SessionEnd`, `StopFailure`) with a stable stdin JSON payload, independent of how
the TUI renders. This is the same fix cmux adopted in PR #1306, which resolved the identical
bypass-mode "stuck status" bug by reading hook events instead of scraping the terminal. M3 makes
hooks Spectty's authoritative status source while keeping scraping as the fallback.

**Why now.** M2 proved the provisioner pattern (managed-namespace JSON editor + atomic write +
`.spectty.bak` backup + foreign-key preservation R7) against `~/.claude.json` `mcpServers`. That
exact machinery generalizes — with no Core-port change — to a SECOND managed surface,
`~/.claude/settings.json` `hooks`. Doing this now (a) fixes the primary status regression users
are hitting today, (b) reuses the freshly-shipped M2 provisioning seam at its cheapest point, and
(c) lands the sidecar-bundling work (`externalBin`) that M2 left as a known gap (M2 L2: sidecar
bundling is still unconfigured) for BOTH `spectty-hook` and retroactively `spectty-mcp`. If the
augment-not-replace seam is wrong, it is far cheaper to discover here — with two hook events on a
parallel input channel — than after M3's Living Spec depends on accurate status.

**What success looks like (acceptance contract):**
1. Spawn a Claude Code session (including with bypass-permissions enabled). Submit a task →
   the badge reaches `Running`; when the turn ends → it returns to `Idle` **without** depending on
   any scraped TUI text. (This is the bypass-mode regression, fixed.)
2. Inspect `~/.claude/settings.json` (Global) or `{project}/.claude/settings.json` (Project) —
   Spectty's managed `hooks` entries are present, and every foreign hook/key round-trips untouched
   (R7 carried into settings.json).
3. Hit a permission prompt → `Notification` with the permission matcher drives `AwaitingInput`;
   answering returns to `Running`. End the session cleanly → `SessionEnd` drives `Completed`. Force
   an API failure → `StopFailure` drives `Error`.
4. Close the session → the PTY terminates, the managed `hooks` section is removed from
   settings.json, and the per-session state file is deleted.
5. Both `spectty-mcp` (retroactive) and `spectty-hook` are bundled as Tauri sidecars
   (`externalBin`) and resolve at runtime in a packaged build, not just `cargo run`.

(1)–(4) are real-Claude-Code manual-acceptance checks (the hook payloads/matchers are empirical,
see R-Hooks); `sdd-verify` MUST treat them as the pass/fail gate on top of the strict-TDD unit gate
(`cargo test --workspace`; `pnpm -C ui test`). Slice 1 satisfies (1), (2 for Stop+UserPromptSubmit),
(4), and (5); Slice 2 completes (3) and full (2).

## Scope

### In scope (full set — both slices)
- **`ClaudeSettingsProvisioner`** — a SECOND `ProvisioningPort` impl (the explore's Option B)
  managing the `hooks` section of `~/.claude/settings.json` (Global) / `{project}/.claude/settings.json`
  (Project). Reuses the M2 `ConfigFile` atomic-write seam, the `.spectty.bak` one-time backup, and the
  foreign-key-preservation invariant (R7) — owning ONLY the managed Spectty hook entries. No
  `ProvisioningPort` trait change: it is another `inject`/`retract` impl, registered alongside the
  existing `ClaudeJsonProvisioner` in the composition root.
- **`inject_spectty_hooks` / `retract_spectty_hooks`** — new pure `String -> String` functions in
  `json_namespace.rs`, mirroring `inject_spectty_mcp`/`retract_spectty_mcp` but operating on the
  top-level `hooks` key (with its `EventName → [{ matcher, hooks: [{ type, command, args }] }]`
  shape) instead of `mcpServers`. Same idempotent inject-only-adds / retract-only-removes contract.
- **All five status sources** (the locked full scope):
  - `UserPromptSubmit` (no matcher) → `Observed::Working` — Idle/Starting → Running.
  - `Stop` (no matcher) → `Observed::Ready` — Running → Idle. **Primary regression fix.**
  - `Notification` (matcher: permission prompt) → `Observed::NeedsInput` — Running → AwaitingInput.
  - `SessionEnd` (no matcher) → `Observed::Finished` — Running/Idle → Completed.
  - `StopFailure` (matcher: API failure) → `Observed::Failed` — any → Error.
- **`spectty-hook` standalone sidecar binary** — a NEW dedicated binary crate (NOT a sub-command of
  `spectty-mcp`; Lock B). It reads `$SPECTTY_SESSION_ID` from its inherited env and the target status
  from its args (`--status <STATUS>`), and atomically writes the per-session state file. Statically
  compiled, no shell-quoting fragility, Windows-capable. Provisioned as a Tauri sidecar exactly like
  `spectty-mcp`.
- **State-file IPC + `run_signal_loop` watcher input** — the hook binary writes an atomic
  per-session JSON state file (`{ "status": "...", "ts": <unix> }`) under a Spectty app runtime dir
  (NOT bare `/tmp`; Lock-defaults). `run_signal_loop` reads it on the existing QUIESCE(200ms) tick;
  on a new event it injects an `Observed` into the SAME `observe_and_diff → transition()` pipeline as
  a PARALLEL source alongside the PTY signal, then marks the event consumed. `detect_status` stays a
  pure PTY-only function (Lock — D24).
- **Sidecar bundling (`externalBin`)** for BOTH sidecars — add `spectty-hook` AND retroactively
  `spectty-mcp` to `tauri.conf.json` `bundle.externalBin`, with target-triple-suffixed binaries and
  runtime path resolution (the existing `spectty_mcp_command()` pattern in `lib.rs` extends to
  `spectty_hook_command()`). Closes M2 L2.
- **Lifecycle wiring** — `spawn_session_impl` injects hooks (BEFORE PTY spawn, same ordering as the
  M2 mcp inject) for the resolved scope; `close_session_impl` retracts hooks AND deletes the state
  file (`.state` + `.state.tmp`), in the existing kill-then-retract teardown order.
- **`SPECTTY_SESSION_ID` correlation** — already injected into Claude's launch env via
  `LaunchSpec.env`; the hook command inherits it and keys the state file by it. NO parsing of
  Claude's internal `session_id` from the hook stdin payload (D23).
- **Project scope included** — the hook provisioner resolves `ProvisioningScope::Project(root)` to
  `{root}/.claude/settings.json`, reusing the M2 injected `is_git_tracked` predicate (Lock-defaults).
- **Strict-TDD seams** — pure units: `inject_spectty_hooks`/`retract_spectty_hooks` round-trip with
  foreign hooks untouched (R7), settings.json scope path resolution, the hook-event → `Observed`
  mapping table, the state-file parse (status string → `Observed`, malformed → ignored), and the
  watcher's "newer event → emit once, then consume" logic over an injected clock/fake file. Fakes:
  `ClaudeSettingsProvisioner` over a `FakeConfigFile`; the `run_signal_loop` watcher over a fake
  state-file reader. Integration (`#[cfg(unix)]`): `spectty-hook` writes a valid state file from env
  + args; end-to-end inject → fake hook fire → state file → `Observed::Ready` through the real loop.

### Out of scope (deferred — explicitly NOT built in M3)
- **`notify`-crate filesystem watching** (kqueue/FSEvents/inotify) — the QUIESCE(200ms) poll is the
  M3 mechanism; the state-file format is chosen so a later `notify` upgrade needs no hook-command or
  provisioner change. (Explore Q2.)
- **HTTP callback to the `spectty-mcp` sidecar** as the IPC transport — requires ephemeral-port
  negotiation at provision time; deferred to M4. (Explore Q2.)
- **`SessionStart` → Starting** and **`Notification(idle_prompt)` → Idle** (the secondary nudge) —
  not needed for the five locked transitions; can ride a later additive change.
- **Removing or rewriting the scraping path** — `detect_status` and the quiescence stopgap stay as
  the fallback (D24). M3 AUGMENTS; it does not replace.
- **Windows hardening** — best-effort only; the M2 macOS-gating ADR holds. The `spectty-hook` binary
  + atomic-rename state file is cross-platform by design (no FIFO/socket), so Windows is closer than
  M2's PTY path, but it is NOT CI-gated.

## Carried-forward deferral decision (M2 L5 / R8 — orphan reconciliation)

M2 consciously DEFERRED boot-time orphan reconciliation (R8 / L5): if Spectty crashes between
inject and retract, managed `spectty_*` keys leak. M3 **widens** that leak surface (now also leaked
`hooks` entries in settings.json AND orphaned `.state` files in the runtime dir) but **does NOT
build full reconciliation** — that stays deferred to keep both slices tight. M3's concrete
mitigation: (a) the existing `.spectty.bak` restore is the manual escape hatch for settings.json
too, and (b) orphaned state files are harmless (a stale `.state` is simply never read once its
session id is gone) and are swept opportunistically when a fresh spawn reuses the runtime dir.
`sdd-design` should record this as the explicit M3 stance and re-flag a proper boot sweep for M4.

## Approach

`src-tauri` stays the composition root. On spawn, in addition to the M2 `ClaudeJsonProvisioner.inject`
(mcpServers into `~/.claude.json`), it now also calls `ClaudeSettingsProvisioner.inject` to write the
managed `hooks` entries into settings.json for the resolved scope — pointing each hook's `command` at
the bundled `spectty-hook` sidecar with `--status <STATUS>` args. Both injects happen BEFORE
`PtyAdapter::spawn`, so hooks are present at agent startup; `SPECTTY_SESSION_ID` is already in
`LaunchSpec.env` and is inherited by every hook command Claude spawns.

At runtime, when Claude fires a lifecycle hook, the `spectty-hook` binary reads
`$SPECTTY_SESSION_ID` + `--status`, and atomically writes `{runtime_dir}/spectty-{id}.state`
(tmp + rename). `run_signal_loop`, already ticking every 200ms (QUIESCE), reads that state file on
each tick: if it carries an event newer than the last consumed one, the loop maps the status string
to an `Observed` variant and feeds it into the SAME `observe_and_diff → transition()` authority the
PTY signal uses, then marks it consumed. The PTY scraping path is untouched and converges on the
same `transition()` — hooks are simply a second, authoritative input. On close, `src-tauri` kills the
PTY (M2 path), retracts BOTH provisioners, and deletes the state file.

```
React (Pane badge)         src-tauri (composition root, run_signal_loop, State)        crates/core (ports + transition)        crates/adapters
  spawn(agent, cwd) ─────▶ resolve runner ─ runner.launch_spec(ctx) ─────────────────▶ AgentRunner::launch_spec ───────────▶ ClaudeCodeRunner (env has SPECTTY_SESSION_ID)
                           mcpProv.inject(scope)  ──────────────────────────────────▶ ProvisioningPort::inject ──────────▶ ClaudeJsonProvisioner (~/.claude.json mcpServers)
                           hookProv.inject(scope) ──────────────────────────────────▶ ProvisioningPort::inject ──────────▶ ClaudeSettingsProvisioner (settings.json hooks → spectty-hook)
                           PtyAdapter::spawn(LaunchSpec) ───────────────────────────────────────────────────────────────▶ openpty + claude process

  (Claude fires hook) ─────────────────────────────────────────────────────────────────────────────────────────────────▶ spectty-hook --status X  →  {runtime}/spectty-$ID.state (atomic)
  term.write ◀─ Channel ◀─ read loop ─┬─ raw coalesce ─▶ pty_output Channel (UNCHANGED)
                                       ├─ OutputSignal producer ─▶ detect_status (PURE, scrape) ─┐
                                       └─ QUIESCE tick: read .state ─▶ status→Observed ───────────┼─▶ observe_and_diff ─▶ Core transition ─▶ SessionRegistry update
  status badge ◀─ status_changed ◀──── emit on change { session_id, status }
  (close) ───────────────▶ PtyAdapter kill + mcpProv.retract + hookProv.retract + delete .state file
```

## Proposed ADRs (D21–D24+) — to be RATIFIED in `sdd-design`

These continue the M2 D-series (D7–D20 consumed). Stated here as proposals; `sdd-design` pins exact
type signatures, the settings.json `hooks` value shape, and the watcher threading model.

- **D21 — Hooks live in `~/.claude/settings.json`, a DIFFERENT file from `~/.claude.json`; a SECOND
  `ProvisioningPort` impl (`ClaudeSettingsProvisioner`) manages them.** Rationale: MCP servers and
  hooks are separate Claude-managed surfaces in separate files; one impl per file keeps the R7
  foreign-key invariant independently testable and avoids bloating `ClaudeJsonProvisioner`. The trait
  is unchanged — `inject`/`retract` already generalize.
- **D22 — State file + QUIESCE(200ms) poll is the IPC mechanism, upgradeable to `notify` later.**
  Rationale: the loop already ticks at 200ms; a per-tick `fs::read_to_string` is negligible; works
  cross-platform with zero new deps and no FIFO/socket lifecycle hazards. State-file format (JSON +
  timestamp under a Spectty runtime dir) is chosen so a `notify` upgrade is non-breaking.
- **D23 — `SPECTTY_SESSION_ID` (already in `LaunchSpec.env`) is the correlation key; Claude's
  internal hook `session_id` is NOT parsed.** Rationale: the hook command inherits Spectty's env, so
  it can key the state file directly; correlating Claude's internal id would add a fragile mapping for
  zero benefit.
- **D24 — Hooks AUGMENT, not replace, TUI scraping; both inputs converge on the single `transition()`
  authority, and `detect_status` stays a pure PTY-only function.** Rationale: keeping the transition
  policy in one place (Core `transition`) means hooks become just another `Observed` source; the
  scraping stopgap remains the fallback for the async gap between hook delivery and the watcher
  consuming it. Injecting file I/O into `detect_status` would break its pure, table-tested seam.
- **D25 (proposed) — `spectty-hook` is a SEPARATE sidecar binary, not a `spectty-mcp` sub-command;
  both are bundled via `externalBin`.** Rationale: hook commands run on every lifecycle event with
  tight timeouts; a dedicated minimal binary (no JSON-RPC server boot) is faster and simpler, and
  bundling both as sidecars closes the M2 L2 gap uniformly. `sdd-design` should confirm whether the
  two binaries share a workspace crate or stay fully independent.

## Slice / delivery plan (chained PRs, stacked-to-main)

This repo ships SDD changes as stacked-to-main chained PRs. M3 splits along the locked two-slice
boundary; `sdd-tasks` will refine these into 400-line-budget work units (Slice 1 alone likely needs
2–3 PRs: pure namespace/provisioner, sidecar + bundling, loop watcher wiring).

| Slice | What lands | Status sources | Notes |
|---|---|---|---|
| **Slice 1 (primary regression fix)** | `ClaudeSettingsProvisioner` + `inject/retract_spectty_hooks` (pure, R7) + `spectty-hook` sidecar + `externalBin` for BOTH sidecars + state-file IPC + `run_signal_loop` watcher + spawn/close wiring (inject/retract/cleanup) | `Stop` → Idle, `UserPromptSubmit` → Running | Self-contained; fixes the bypass-mode "stuck Running" bug end-to-end. No-matcher hooks fire every turn. |
| **Slice 2 (completeness)** | The three matcher/terminal events added to the hook injection set + their `Observed` mappings + tests | `Notification(permission)` → AwaitingInput, `SessionEnd` → Completed, `StopFailure` → Error | Builds on Slice 1's machinery; only new hook entries + mapping rows. Lower payoff-to-risk than Slice 1. |

**Budget verdict**: Slice 1's provisioner + sidecar + bundling + loop wiring will exceed the 400-line
review budget as one PR; `sdd-tasks` should plan splittable sub-boundaries (pure JSON namespace +
provisioner / sidecar binary + `externalBin` / loop watcher + lifecycle wiring). Slice 2 is small
(additive hook rows + mapping table + tests) and likely fits one PR.

## Risks / open questions (for `sdd-spec` & `sdd-design`)
- **R-Settings (settings.json shape)** — the `hooks` value is more nested than `mcpServers`
  (`EventName → [{ matcher?, hooks: [{ type:"command", command, args }] }]`) and settings.json carries
  far more diverse foreign keys (permissions, env, model). `sdd-design` must pin the exact managed
  value shape and confirm R7 foreign-key preservation holds for this nested structure; `sdd-spec`
  should make "foreign hooks/keys round-trip untouched" an explicit tested property.
- **R-Hooks (empirical payloads/matchers)** — the `Notification` permission-prompt matcher and the
  `StopFailure` event/matcher are empirical (docs.claude.com/docs/en/hooks + real-session
  observation). `sdd-spec` should capture them as DATA (a mapping table in the adapter/binary), not
  Core logic, and the real-session checks are manual-acceptance gates.
- **R-Live-reload** — Claude Code reloads settings.json dynamically; injecting hooks mid-spawn should
  be safe given inject-BEFORE-spawn ordering (matches M2), but `sdd-design` should confirm Claude
  picks up hooks injected immediately before launch.
- **R-Async-gap / consume-once** — hooks are delivered asynchronously by Claude's hook subsystem; the
  watcher must read a written-once state file and emit each event EXACTLY once (no re-emit on every
  tick) while staying robust to a partially-written file (atomic rename mitigates). `sdd-design` must
  pin the "newer-than-last-consumed" detection (timestamp vs file mtime vs monotonic counter) and the
  bounded staleness before scraping fallback re-asserts.
- **R-Runtime-dir** — exact Spectty app runtime dir for state files (Tauri `app_data_dir`/`app_cache_dir`
  vs an explicit `~/.spectty/run`), and its creation/permissions. `sdd-design` to pin (Lock-defaults
  says "a Spectty app runtime dir, not bare /tmp").
- **R-Bundling (externalBin)** — M2 left sidecar bundling unconfigured (L2). `externalBin` requires
  target-triple-suffixed binaries and a build step that produces both `spectty-mcp` and `spectty-hook`
  for the host triple; `sdd-design`/`sdd-tasks` must define the build wiring and runtime path
  resolution for packaged vs `cargo run` builds.
- **R8-carried (orphan leak)** — settings.json `hooks` and `.state` files widen the M2 R8 leak
  surface; M3 defers full reconciliation (see the carried-forward deferral section) and relies on
  `.spectty.bak` + harmless-stale-state-file. Re-flag a boot sweep for M4.

### Open questions distilled (from the exploration)
1. Do `spectty-hook` and `spectty-mcp` share a workspace crate or stay fully independent binaries?
   (Affects D25 + bundling.)
2. State-file event identity: timestamp field vs file mtime vs monotonic counter for "consume once"?
3. Does Slice 1 ship Project scope immediately, or Global-only first with Project in Slice 2?
   (Locked default is Project included; `sdd-spec` confirms the slice boundary.)
4. Exact runtime dir + cleanup ownership (close vs opportunistic sweep on next spawn).
