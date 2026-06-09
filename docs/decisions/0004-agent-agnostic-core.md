# ADR-0004: Agent-agnostic Core behind the `AgentRunner` port

- Status: Accepted
- Date: 2026-06-07
- Deciders: project owner

## Context

Spectty's stated requirement is to "specialize in code agents." Paradoxically, that requires
the Core to know **nothing** about any specific agent. Here is why:

- The market of AI CLI coding agents is young and fast-moving: Claude Code, Cursor CLI,
  Codex CLI, Aider are all plausible first-class targets, and new ones will appear.
- Competitors (Conductor, Crystal, Claude Squad) are **Claude-Code-specific**. That is
  the gap Spectty occupies: the agent-agnostic supervision layer. Hardcoding Claude Code
  would erase that differentiation.
- "Specializing" means excelling at supervision primitives (status detection, cost
  tracking, permission approval, diff explanation) for *any* agent, not duplicating the
  agent's own capabilities.
- The `AwaitingInput` detection — the most critical signal — is inherently agent-specific:
  Claude Code signals it differently from Aider, which signals it differently from a
  generic CLI. This is behavior, not config, and it must be encapsulated per agent without
  leaking into the Core.

## Decision

The Core defines a single `AgentRunner` port (Rust trait). Every agent-specific detail
lives behind it. The Core contains zero agent names, zero agent-specific regexes, and zero
`if agent == "..."` branches.

The `AgentRunner` contract (canonical definition in
[Agent Abstraction](../architecture/agent-abstraction.md)):

```rust
trait AgentRunner: Send + Sync {
    fn launch_spec(&self, ctx: &LaunchContext) -> LaunchSpec;
    fn detect_status(&self, signal: &OutputSignal) -> Option<AgentStatus>;
    fn parse_cost(&self, signal: &OutputSignal) -> Option<CostDelta>;
    fn quick_actions(&self, status: &AgentStatus) -> Vec<QuickAction>;
    fn descriptor(&self) -> AgentDescriptor;
}
```

`detect_status` is the critical method: given a normalized window of recent PTY output
(`OutputSignal`), each adapter returns the current `AgentStatus` or `None` (no change).
The `AwaitingInput` detection lives entirely here — the Core just acts on the returned
status.

**Planned adapters:**

| Agent | Adapter | Status signal | Cost source |
|---|---|---|---|
| Claude Code | `ClaudeCodeRunner` | Permission prompt patterns, "Do you want…" lines | Usage output lines |
| Generic CLI | `GenericRunner` | Idle-timeout heuristic only | None |
| Cursor CLI | `CursorRunner` (future) | Prompt markers (TBD) | TBD |
| Codex CLI | `CodexRunner` (future) | Approval prompt patterns | TBD |
| Aider | `AiderRunner` (future) | `>` prompt, confirmation lines | Token report |

The **Generic adapter** is the safety net: any CLI agent works at a baseline level (run +
idle timeout detection) before a first-class adapter exists.

**Extension path:**
- MVP: `ClaudeCodeRunner` + `GenericRunner`
- Phase 2: declarative `agent.toml` manifest (command, prompt regexes, cost regex)
  → auto-generates a runner without recompiling, covering ~80% of future agents
- Phase 3: WASM/plugin runners for agents needing custom logic

**Forbidden patterns (enforced by code review and architecture tests):**
- `if agent == "claude"` anywhere in the Core
- Agent-specific regex in the UI layer or session orchestration
- The PTY adapter knowing which agent it runs (it runs a `LaunchSpec`, nothing more)

## Consequences

**Positive**
- Adding a new agent type does not touch the Core or the session orchestration code —
  only a new `AgentRunner` implementation.
- The product's multi-agent positioning is structurally enforced, not just a policy.
- The Generic adapter means "new agent support" has a baseline on day one, with a
  progressive enhancement path to first-class support.
- `AgentDescriptor.capabilities` lets the UI degrade gracefully: if `reports_cost ==
  false`, the Dashboard shows "n/a" instead of wrong data.
- Agent-specific detection logic is co-located with the agent adapter and testable in
  isolation.

**Negative**
- Per-agent detection logic to implement and maintain: each new first-class adapter
  requires someone to study that agent's output patterns and write/test the detection.
- The Generic adapter's idle-timeout heuristic will produce false `AwaitingInput` signals
  for slow agents — acceptable for a generic fallback, not for a first-class experience.
- The `OutputSignal` normalization layer (stripping ANSI, windowing output) is shared
  infrastructure that must be robust; bugs there affect all agent adapters.

**Neutral**
- The `AgentRunner` trait is a Port in the Hexagonal sense ([ADR-0003](0003-hexagonal-architecture.md));
  this decision is the specific application of that pattern to agents.
- The declarative manifest path (Phase 2) does not change the trait contract — manifests
  compile down to a generic `ManifestRunner` that implements `AgentRunner`.

## Alternatives considered

### Hardcode Claude Code first, generalize later

Implement session management, status detection, and cost parsing with Claude Code
hard-wired. Add an abstraction layer when a second agent is needed.

**Why not chosen:** "Generalize later" is the correct call when the abstraction is
speculative. Here it is not — the abstraction IS the product. The entire differentiation
from Claude Squad, Conductor, and Crystal is that Spectty is not Claude-Code-specific. If we
ship a Claude-Code-hardcoded MVP and then try to extract the abstraction under live
product pressure, we will have a harder refactor and a harder story to tell. The
`AgentRunner` trait costs almost nothing to define upfront; not defining it costs the
product's positioning.

### Configuration-only approach (no trait, just TOML)

Define agents entirely through a config file (command, env, prompt regexes, cost regex)
with no code. No Rust trait; just a config-driven runner.

**Why not chosen:** `detect_status` is the hardest problem and it is often heuristic —
matching a known prompt string is easy, but detecting "`AwaitingInput` because the agent
went idle after outputting a question" requires stateful logic. A pure config file cannot
express stateful behavior. We retain the trait as the foundation and add a declarative
layer on top in Phase 2 for the 80% of cases that are regex-matchable.

## Amendment — M3 hook-based status detection (D21–D25)

- Date: 2026-06-09
- Driver: change `M3-hook-status-detection`

M3 adds Anthropic's official Claude Code hook system as the **authoritative** status
source, fixing the bypass-permissions "stuck Running" bug without changing any Core type.
The five architectural decisions below (D21–D25) continue the M2 D-series (M2 used D7–D20).

### D21 — Hooks live in `~/.claude/settings.json`; second `ClaudeSettingsProvisioner`; trait unchanged

`settings.json` is a **different file** from `~/.claude.json`. A second
`ClaudeSettingsProvisioner<F: ConfigFile>` manages only the `hooks` key in that file; it
is a second `ProvisioningPort` impl, not an extension of `ClaudeJsonProvisioner`. The
composition root holds it as a second `Arc<dyn ProvisioningPort>`, injected and retracted
alongside the MCP one. `ProvisioningPort` itself is **unchanged**.

R7 foreign-key preservation is GENERALIZED: `mcpServers` owned a NAMED key (`"spectty"`);
`hooks` owns ROWS whose inner `hooks[].command` equals the sidecar path. Retract removes
only those rows; a user's own hook on the same event survives. Same `preserve_order` /
VALUE+ORDER contract as M2.

**Risk R-Settings — RESOLVED.** The round-trip test
(`inject_spectty_hooks_round_trip_preserves_foreign_keys_and_order`) pins foreign-hook
survival on the same event. WU-9 path-agreement integration test pins D25 independently.

**Implementing files**:
- `crates/adapters/src/provision/json_namespace.rs` — `inject_spectty_hooks` / `retract_spectty_hooks` / `HookCommandEntry`
- `crates/adapters/src/provision/settings_provisioner.rs` — `ClaudeSettingsProvisioner<F: ConfigFile>`
- `crates/adapters/src/provision/scope.rs` — `settings_path_for_scope` (Global→`~/.claude/settings.json`, Project→`{root}/.claude/settings.json`)
- `src-tauri/src/lib.rs` — composes second provisioner as `HooksProvisionerState`

---

### D22 — Consume-once via sidecar-owned monotonic counter `ts`; 200ms QUIESCE poll

The `spectty-hook` sidecar owns a monotonic integer counter `ts`. On each invocation it
reads the prior `.state` file for the current `ts` (default 0) and writes `ts + 1`.
`StateFileReader` emits an event ONLY when `state.ts > self.last_ts.unwrap_or(0)`, then
advances `last_ts`. This gives unambiguous consume-once semantics with no clock dependency
and survives sub-200ms event bursts.

Wall-clock timestamps were rejected (clock skew, same-tick collisions); `mtime` was
rejected (coarse, FS-dependent, no Windows guarantee). The counter format is
notify-upgradeable: a future M4 FS-watch can replace the poll loop without changing the
sidecar protocol.

**Implementing files**:
- `crates/spectty-hook/src/main.rs` — increments counter per write
- `crates/adapters/src/hook/reader.rs` — `StateFileReader` with `last_ts: Option<u64>`
- `crates/adapters/src/hook/state.rs` — `HookState { event, ts, session_id }`

---

### D23 — `SPECTTY_SESSION_ID` is the correlation key; sidecar ignores Claude's stdin

`SPECTTY_SESSION_ID` is already set in `LaunchSpec.env` (M2 established this). Claude
Code inherits it; every hook command inherits Claude's env, so the sidecar receives it
for free without a lookup table. The sidecar names the state file `spectty-{id}.state`
and embeds the id in the JSON payload. `StateFileReader` checks that the `session_id`
field matches `self.session_id` (D23 guard); a stale file from a crashed prior session
returns `None` on every poll tick.

Claude's internal hook `session_id` (passed on stdin) is a DIFFERENT identifier.
Correlating it would require a lookup table and would tightly couple the sidecar to
Claude Code's internal protocol, contradicting ADR-0004's agent-agnostic intent.

The sidecar reads (drains) and ignores Claude's stdin JSON. This keeps the binary
sub-millisecond and well within the sync-hook timeout.

**Implementing files**:
- `crates/spectty-hook/src/main.rs` — drains stdin, reads `SPECTTY_SESSION_ID`
- `crates/adapters/src/hook/reader.rs` — `session_id` field + D23 guard in `poll`
- `src-tauri/src/commands/session.rs` — passes `SPECTTY_SESSION_ID` via `PtySpawnConfig.env`

---

### D24 — Hooks AUGMENT scraping; hook-first per tick; `detect_status` unchanged; EOF arm ungated

Hook events are fed through the SAME `observe_and_diff → transition()` authority as PTY
bytes. On each Ingest and Quiesce tick, `run_signal_loop` polls `StateFileReader` FIRST;
a `Some(event)` runs `observe_and_diff(event_to_observed(event))` before the PTY
observation that tick. Double-emit is impossible: `observe_and_diff` only emits on an
ACTUAL status change (a second same-tick observation that doesn't change status returns
`None`).

`detect_status` is **not touched** — it stays a pure PTY-scraping fn. Routing hooks INTO
`detect_status` would have introduced file I/O into a pure function and broken its
table-test seam.

**EOF arm exception**: the EOF-driven `Ready` scraping emission is intentionally NOT
gated behind `hooks_active`. Process exit (EOF) must still drive `Running → Idle` for
sessions where no `Stop` hook fires (e.g. a Generic session, or a Claude session that
exits before the hook reaches the sidecar). The `hooks_active` gate applies only to
mid-stream PTY-scraping-derived `Ready` emissions, preventing the scraping fallback from
racing against the hook-sourced result.

**Risk R-Settings — RESOLVED** (see D21). **Risk R-Async-gap**: ≤200ms latency; scraping
covers the gap; consume-once prevents replay.

**Implementing files**:
- `src-tauri/src/session_runtime.rs` — `run_signal_loop` with `hook_reader` param; `emit_scraping_guarded`; EOF arm unchanged
- `crates/adapters/src/hook/state.rs` — `event_to_observed` pure mapping table

---

### D25 — `spectty-hook` is a standalone binary crate; both sidecars bundled via `externalBin`; runtime-dir resolver duplicated

`spectty-hook` is an independent binary crate (`crates/spectty-hook/`), mirroring the
proven `spectty-mcp` shape (serde + serde_json only; no `spectty-core`, no Tauri). A
`spectty-mcp` subcommand was rejected: it would couple two unrelated sidecars' release
and versioning cycles. Independent binaries keep each minimal and independently replaceable.

The runtime-dir resolver (`spectty_runtime_dir()`) is ~10 lines duplicated in both
`crates/spectty-hook/src/runtime_dir.rs` and `src-tauri/src/lib.rs`. A shared crate was
rejected: it would require `spectty-hook` to depend on the `spectty` lib (which has a
Tauri dependency), violating the serde-only constraint. The duplication is small enough
that the path agreement is asserted by a load-bearing integration test.

Both sidecars are bundled via the **TAURI_CONFIG overlay** mechanism:
`src-tauri/tauri.bundle.conf.json` carries the `externalBin` declaration and is merged
only when the Tauri CLI is invoked with `--config src-tauri/tauri.bundle.conf.json`. This
prevents `cargo build --workspace` from failing when the sidecar binaries are absent
(avoids the W3 footgun). The `scripts/build-sidecars.sh` script builds both release
sidecar binaries and copies them to `src-tauri/binaries/`. It is wired as
`beforeBuildCommand` in `tauri.conf.json`.

Bundle command: `pnpm tauri build --debug --config src-tauri/tauri.bundle.conf.json`

**Risk R-PathAgreement — RESOLVED.** The integration test
`spectty_hook_end_to_end_monotonic_ts_and_path_agreement` (`src-tauri/tests/hook_integration.rs`,
`#[cfg(unix)]`) asserts that `spectty_lib::spectty_runtime_dir()` and
`spectty_hook::spectty_runtime_dir()` resolve to the exact same path. If they diverge,
status never updates — silent failure prevented by this load-bearing test.

**Implementing files**:
- `crates/spectty-hook/src/runtime_dir.rs` — sidecar-side resolver
- `crates/spectty-hook/src/lib.rs` — thin lib re-exporting `spectty_runtime_dir`
- `src-tauri/src/lib.rs` — `spectty_hook_command()` + `spectty_runtime_dir()`
- `scripts/build-sidecars.sh` — shellcheck-clean build script
- `src-tauri/tauri.bundle.conf.json` — `externalBin` overlay (closes M2 L2)
- `src-tauri/tests/hook_integration.rs` — D25 path-agreement integration test (load-bearing)

---

### M3 deferred items (L-settings-orphan)

**L-settings-orphan — deferred to M4 boot-sweep.** A crashed session can leak hook rows
in `settings.json` and/or an orphaned `.state` file. Mitigations shipped in M3:
`.spectty.bak` (atomic-write backup before first write); stale-state harmlessness via
`session_id` guard (D23); opportunistic pre-spawn sweep (`remove_stale_state_file` +
`remove_stale_tmp_files`). Full boot-time orphan reconciliation requires the
persistence-backed session registry and is deferred to M4. See also
`openspec/changes/M3-hook-status-detection/acceptance.md` deferred items section.

---

## Amendment — Superseded for M2+ (provisioning is a sibling Core port, not a runner method)

- Date: 2026-06-08
- Driver: change `M2-spawn-agent-provisioner` (design ADR **D7 / risk R9**)

The original sketch of the `AgentRunner` contract (see
[Agent Abstraction](../architecture/agent-abstraction.md)) carried a provisioning
method on the runner trait — `fn provisioner(&self) -> Option<Box<dyn Provisioner>>`.
**M2 supersedes that mechanism.** Provisioning now lives behind a **separate Core port,
`ProvisioningPort`** (`crates/core/src/ports/provisioning.rs`), NOT on `AgentRunner`.

Rationale:

- **Provisioning is a session-lifecycle concern, not a per-output-tick concern.** Inject
  happens once on session create, retract once on session close — it does not belong next
  to `detect_status`/`parse_cost`, which run per `OutputSignal`. Keeping it off the runner
  trait keeps `AgentRunner` cohesive (launch + observe + describe).
- **Generic agents skip it cleanly without a trait method.** `AgentDescriptor` carries
  `requires_provisioning: bool`; the composition root decides whether to inject. A Generic
  agent simply has `requires_provisioning == false`, so no `Option`/`None` ceremony leaks
  into the runner contract.
- **The agent-agnostic intent of this ADR is preserved.** The Core still contains zero
  agent names and zero `if agent == "..."` branches. Only the MECHANISM moved: from a
  method on `AgentRunner` to a sibling port. The actual `AgentRunner` trait shipped in M2
  has five methods (`launch_spec`, `detect_status`, `parse_cost`, `quick_actions`,
  `descriptor`) and **no `provisioner()`**.

**The code is the source of truth** — `crates/core/src/ports/agent_runner.rs` and
`crates/core/src/ports/provisioning.rs`. The `provisioner()` shape shown in
`agent-abstraction.md` is historical; that doc carries the same amendment note.
