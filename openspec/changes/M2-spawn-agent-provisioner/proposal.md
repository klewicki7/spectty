# M2 — Spawn Agent + Provisioner — Proposal

> SDD propose phase. Consumes `sdd/M2-spawn-agent-provisioner/explore` (obs #799) +
> `openspec/changes/M2-spawn-agent-provisioner/explore.md`, the AUTHORITATIVE roadmap M2
> scope/exit criteria, ADR-0004 (agent-agnostic core), ADR-0006 (agent protocol), and the
> agent-abstraction / agent-protocol / domain-model / pty-layer docs. Drives `sdd-spec` and
> `sdd-design`. Artifact store: HYBRID (engram `sdd/M2-spawn-agent-provisioner/proposal` +
> this file).

## Intent

**What problem.** M1 shipped a real terminal over a real PTY: a raw byte-pump (spawn / write /
resize / kill), hybrid-batched IPC, xterm rendering, scrollback. It supervises *nothing* — it
has no concept of an "agent", no lifecycle state, no protocol injection. M2 turns the terminal
into a **cockpit's first instrument**: launch a real AI CLI agent inside the PTY, detect its
lifecycle state (Starting → Idle → Running → AwaitingInput / Completed / Error), and inject the
Spectty Agent Protocol's MCP tools into the agent's config so cooperative agents can later speak
back. This is where the product's thesis — *agent-agnostic supervision* (ADR-0004) — first
becomes executable code.

**Why now.** M2 is the next vertical slice and the structural proof of the entire architecture.
It is where the `AgentRunner` port, the `AgentStatus` state machine, the `Session` aggregate +
`SessionRegistry`, and the `ProvisioningPort` all become real for the first time — and where the
agent-agnostic boundary either holds or leaks. Getting two adapters (ClaudeCode + Generic) to
work through ONE port, with ZERO agent names in the Core, is the differentiator from
Claude-Code-specific competitors (Conductor, Crystal, Claude Squad). If the abstraction is going
to be wrong, it is cheaper to discover it here, with two agents and no Spec pane, than in M3 when
the Living Spec depends on it.

**What success looks like (acceptance contract — verbatim from roadmap M2 exit criteria):**
1. Spawn a Claude Code session on a local git repo; it launches and reaches `Idle`.
2. Inspect the Claude Code config — Spectty's managed section with MCP tools is present.
3. Give it a task; status transitions to `Running`, then `AwaitingInput` when it hits a
   permission prompt, then back to `Running` after input is given.
4. Close the session; the PTY process terminates; the managed section is removed from the agent
   config.
5. Generic adapter: spawn `bash`; status reaches `Idle`; idle-timeout transitions to `Completed`
   after inactivity (configurable).

These five checks are the definition of DONE. (1)–(4) are real-Claude-Code manual-acceptance
checks (the `AwaitingInput`/permission-prompt patterns are empirical — see R5); (5) is the
Generic-tier baseline. `sdd-verify` MUST treat them as the pass/fail gate, on top of the
strict-TDD unit gate (`cargo test --workspace`; `pnpm -C ui test`).

## Scope

### In scope
- **`AgentRunner` port (Core trait)** + two adapters: `ClaudeCodeRunner` (Cooperative tier) and
  `GenericRunner` (Generic tier). M2 method subset: `launch_spec`, `detect_status`, `descriptor`,
  `tier` fully implemented; `parse_cost` and `quick_actions` as honest, tested skeletons. (Lock 1.)
- **`OutputSignal`** as a Core serde value type (ANSI-stripped rolling text window + activity +
  exit code + a serde-friendly time field) consumed by `detect_status`. (Lock 2.)
- **`OutputSignal` producer** (ANSI strip + rolling window assembler) in `crates/adapters`, driven
  from the `src-tauri` read loop on a path INDEPENDENT of the raw `pty_output` Channel (it must not
  block or throttle rendering). (Lock 2 / R6.)
- **`AgentStatus` state machine**: a pure Core `transition(current, observed) -> AgentStatus`
  enforcing legal transitions, plus the existing `AgentStatus` enum carried forward. Detector
  invocation orchestrated from `src-tauri` (which owns the read loop + the runner).
- **`Session` aggregate (Core)** grown toward domain-model.md: at minimum `id`, `workspace`,
  `agent: AgentSpec`, `status`, `title`, `created_at`. `Spec`, `CostMetrics`, `Worktree`,
  `last_diff` are stubs/skeletons or deferred (see Out of scope).
- **`SessionRegistry` in Core**: create / look up / close, owning `Session` aggregates with the
  `PersistencePort`-style `&self` interior-mutability convention, shared as `tauri::State`. Owns
  `SessionId` minting (migrates `next_pty_id`). (Lock 6.)
- **`ProvisioningPort` (Core trait) + `ProvisionerAdapter`** — kept SEPARATE from `AgentRunner`
  (Lock 1). M2 implements **Layer-1 MCP-tool registration + teardown only** (Lock 3): a JSON
  managed-namespace editor owning only `spectty_*` keys, writing `mcpServers` entries into
  `~/.claude.json` (GLOBAL) or `.mcp.json` (PROJECT), behind an atomic-write + `.spectty.bak`
  file-IO seam. `retract` removes the keys on session close.
- **GLOBAL-vs-PROJECT scope detection**: default GLOBAL; resolve to PROJECT when the agent's config
  file is git-tracked, via a minimal injected git-tracked predicate (NOT a full `GitPort`). (Lock 5 +
  scope strategy.)
- **`spectty_*` MCP server**: registered-but-stubbed in M2 (Lock 4) — the config entry points at a
  `spectty-mcp` binary that exists, starts over stdio, and advertises the tool schemas, but the
  tool EFFECTS (persist spec, trigger diff, resolve approval) are M3.
- **UI**: spawn a session (pick agent + workspace directory), a Pane-header `AgentStatus` indicator
  reacting to a new `status_changed { session_id, status, quick_actions }` Tauri event, and a named
  session title.
- **Strict-TDD seams**: pure units for `transition`, per-agent `detect_status`, `GenericRunner`
  idle-timeout (injected time via `ClockPort`-style seam), `launch_spec` mapping, the JSON
  managed-namespace editor, `AgentSpec` parsing, and scope resolution (injected git-tracked
  predicate); fakes/harness for file-IO, the `OutputSignal` producer wiring, and `SessionRegistry`
  command wiring; a `#[cfg(unix)]` real-PTY agent-spawn integration test.

### Out of scope (M3 / M4 / M5 — explicitly NOT built in M2)
- **Layer-2 (`additionalContext` hook) and Layer-3 (SKILL.md/rules) injection** — M3. They need the
  live Spec and the refresh/fingerprint loop. (Lock 3.)
- **The live Spec pane and the `Spec` aggregate's behavior** (plan-approval gate, structured task
  progress, `spec_updated` polling loop) — M3.
- **`spectty_*` tool EFFECTS** (spec persistence, diff trigger, approval resolution/unblocking,
  cost ingestion) — M3. M2 ships the stub server + Layer-1 registration only. (Lock 4.)
- **VibeLens / `DiffExplainerPort` / `spectty_diff` wiring** — M3.
- **Cost-parsing depth**: real `parse_cost` regexes + `CostMetrics` accumulation + `cost_updated`
  + `spectty_cost` ingestion — M3. M2 ships only the skeleton method + struct.
- **`quick_actions` real prompt-answering** (sending `y\n` etc.) and the structured
  `spectty_approval` `AwaitingInput` path — M3. M2 `AwaitingInput` is PTY-scraped only (R5).
- **Worktrees / `GitPort` / Checkpoints / branch isolation** — M4. M2 uses only a minimal injected
  git-tracked predicate for scope detection, not a real `GitPort`.
- **Multi-session UI** (tabs, panes, switcher, the split tree) — M4. M2 grows session chrome on the
  single Pane only.
- **Per-agent format adapters beyond Claude Code JSON** (Cursor `.cursor/mcp.json`, Codex TOML, Aider
  YAML) — fast-follow / post-MVP. M2 = Claude Code JSON namespace editor only.
- **Provisioner refresh hook + SHA fingerprint cache** — belongs to Layer-2 dynamics, M3.

## Cross-cutting stance

M2 stays **macOS-first** (inherits M1's stance): the real-PTY agent-spawn integration test is
`#[cfg(unix)]`; Windows agent spawn is best-effort, not CI-gated. The Core quarantine is the hard
invariant: `crates/core` remains **serde + thiserror only**, cargo-deny-enforced. `AgentRunner`,
`ProvisioningPort`, `OutputSignal`, `AgentSpec`, the `transition` fn, and `SessionRegistry` live in
Core and import nothing from adapters/tauri/engram. All agent names (`claude`, `bash`), all
config-format knowledge, all ANSI/regex parsing, and all file-IO live in `crates/adapters` /
`src-tauri`. **Zero new deps in `crates/core`** (the `ClockPort`-style time seam is a Core trait, its
concrete clock lives outside Core).

## Approach

`src-tauri` remains the composition root and owns the live process + read loop, exactly as in M1.
On spawn, `src-tauri` asks the chosen `AgentRunner` for a `LaunchSpec` (program/args/env/cwd),
runs the Provisioner's `inject` for the resolved scope (writing `spectty_*` MCP entries into the
agent config), then spawns the PTY via the existing `PtyAdapter`. The M1 raw-byte path is untouched:
the read loop still coalesces raw bytes into the `pty_output` Channel for xterm. A SECOND,
independent consumer of the same read stream feeds the **`OutputSignal` producer** (ANSI strip +
rolling window); on each updated `OutputSignal`, `src-tauri` calls
`runner.detect_status(&signal) -> Option<AgentStatus>`, runs the observed status through the pure
Core `transition`, updates the `Session` in the `SessionRegistry`, and — on change — emits
`status_changed`. On close, `src-tauri` kills the PTY (M1 path) and calls the Provisioner's
`retract` to remove the managed `spectty_*` keys. The Core sees only ports; it never touches a PTY,
a file, or an ANSI byte.

```
React (Pane + spawn UI)              src-tauri (composition root, read loop, State)         crates/core (ports + aggregate)            crates/adapters
  spawn(agent, cwd) ──────────────▶  resolve runner ─ runner.launch_spec(ctx) ───────────▶  AgentRunner::launch_spec (per-agent) ───▶  ClaudeCodeRunner / GenericRunner
                                     provisioner.inject(scope) ───────────────────────────▶  ProvisioningPort::inject ───────────────▶  JSON namespace editor + atomic write
                                     PtyAdapter::spawn(LaunchSpec) ─────────────────────────────────────────────────────────────────▶  openpty + agent process
  term.write(bytes) ◀── Channel ◀──  read loop ─┬─ raw coalesce ─▶ pty_output Channel (M1, UNCHANGED)
                                                └─ OutputSignal producer ─▶ runner.detect_status ─▶ Core transition ─▶ SessionRegistry update
  status badge ◀── status_changed ◀─ emit on status change { session_id, status, quick_actions }
  (close) ────────────────────────▶  PtyAdapter kill (M1) + provisioner.retract(scope) ──▶  ProvisioningPort::retract ─────────────▶  remove spectty_* keys, restore
```

## Architectural decisions — the six locks (each RESOLVED with one-line rationale)

**Lock 1 — `AgentRunner` M2 method subset + `ProvisioningPort` kept SEPARATE from `AgentRunner`.
RESOLVED: separate port; `launch_spec`/`detect_status`/`descriptor`/`tier` full, `parse_cost`/
`quick_actions` skeletons.**
Rationale: domain-model.md already lists `ProvisioningPort` as its OWN Core port and provisioning is a
session-lifecycle concern (inject on create, retract on close) with a different lifetime than per-output
status detection — coupling it as `AgentRunner::provisioner() -> Box<dyn>` (the shape sketched in
ADR-0004/agent-abstraction) would force every runner, including Generic which needs no injection, to
carry the seam; **this overrides the trait-method shape in ADR-0004 in favor of domain-model.md's
separate-port listing for M2** (Generic returns no provisioner simply by not being wired to one).

**Lock 2 — `OutputSignal` is a Core serde type with a NON-`Instant` time field; producer lives in
`crates/adapters`, driven independently of the raw render path. RESOLVED.**
Rationale: `OutputSignal` must cross the Core port boundary into `detect_status`, so it must be `serde`
+ Core-pure — `Instant` is neither serde-serializable nor wall-clock-comparable across the boundary, so
time is modeled as elapsed-millis-since-last-byte (or an injected `ClockPort`-derived `Timestamp`,
domain-model.md lists `ClockPort`); the producer (ANSI strip + rolling window) is impure adapter code
that must run on a SECOND consumer of the read stream so it can never throttle the M1 `pty_output`
render path (R6).

**Lock 3 — M2 = Layer-1 MCP registration + teardown ONLY; Layers 2/3 → M3. RESOLVED (CONFIRMED).**
Rationale: the roadmap M2 scope names only "writes MCP tool registrations … managed-section markers,
atomic writes, backup-before-write … teardown on close", and Layers 2 (`additionalContext` hook) and 3
(SKILL.md) exist to push the *live Spec* — which does not exist until M3 — so shipping them now would be
injecting empty context (YAGNI + wasted refresh/fingerprint machinery).

**Lock 4 — `spectty_*` MCP server ships REGISTERED-BUT-STUBBED in M2. RESOLVED.**
Rationale: exit-criterion (2) requires the managed section "with MCP tools" to be present and inspectable,
and a Claude Code MCP entry that points at a missing binary breaks the agent's startup — so a real
`spectty-mcp` binary must exist and start over stdio advertising the tool schemas, but its EFFECTS
(persist spec, trigger diff, resolve approval — R4) depend on the Spec/VibeLens machinery that is M3, so
the tools are honest stubs that accept calls and return acknowledgements without side effects.

**Lock 5 — JSON managed-NAMESPACE editor for `~/.claude.json` (GLOBAL/user scope) + `.mcp.json`
(PROJECT scope). RESOLVED.**
Rationale: `~/.claude.json` is ONE big nested JSON document (verified, code.claude.com 2026-06) where
GLOBAL MCP servers live at top-level `mcpServers` and PROJECT servers live in `.mcp.json` at repo root —
text managed-markers would corrupt structured JSON, and a `claude mcp add` subprocess is neither atomic
nor unit-testable, so M2 ships a pure `String -> String` JSON editor that owns ONLY `spectty_*` keys
(coexisting safely with gentle-ai and user keys, R7) behind an atomic-write (tmp + fsync + rename) +
`.spectty.bak` impure file-IO seam.

**Lock 6 — TWO coexisting registries; Core `SessionRegistry` owns the `Session` aggregate, `src-tauri`
`PtyRegistry` owns OS handles; `SessionId == PtyId`. RESOLVED.**
Rationale: the `Session` aggregate root MUST live in Core (domain-model.md) with the `PersistencePort`-
style `&self` interior-mutability convention, while OS-level writer/child/stop handles are inherently
non-Core and stay in the M1 `PtyRegistry` — unifying `SessionId == PtyId` (migrating `next_pty_id` into
`SessionRegistry` minting) keeps the two registries in lockstep with no cross-mapping table, and avoids
collapsing `Session` into `src-tauri` (which would violate the aggregate-root-in-Core rule).

**Scope-detection strategy (supporting Lock 5) — default GLOBAL; PROJECT when the agent config file is
git-tracked. RESOLVED.**
Rationale: the roadmap says "GLOBAL by default; PROJECT when the file is committed" — a full `GitPort` is
M4, so M2 resolves scope through a single injected `is_git_tracked(path) -> bool` predicate (testable as a
pure function with a fake predicate; the real probe is a minimal `git ls-files --error-unmatch`-style check
in the adapter), defaulting to GLOBAL when the probe is unavailable or false.

## Provisional PR / slice boundaries (chained PRs, stacked-to-main)

M2 is large (AgentRunner + 2 adapters + state machine + OutputSignal producer + Provisioner + SessionRegistry
+ stub MCP server + UI). The M1 split was 4 PRs by work-unit; M2 needs ~6 slices. These are a STARTING POINT
for `sdd-tasks` — rough changed-line estimates flag which exceed the 400-line review budget. Each slice is
independently shippable and leaves the app green.

| # | Slice (work-unit) | What lands | Est. changed lines | Budget |
|---|---|---|---|---|
| **PR1** | Core agent contracts | `AgentRunner` trait, `OutputSignal` serde type, `AgentSpec`, `AgentTier`/`AgentDescriptor`, pure `transition(current, observed)` + tests. Core-only, no wiring. | ~280–360 | OK (TDD-pure) |
| **PR2** | Runner adapters | `GenericRunner` (idle-timeout `detect_status` w/ injected time) + `ClaudeCodeRunner` (`launch_spec` + scrape `detect_status`) + `parse_cost`/`quick_actions` skeletons + tests. | ~350–450 | **RISK: likely >400** → may split into PR2a Generic / PR2b ClaudeCode |
| **PR3** | OutputSignal producer + status wiring | ANSI-strip + rolling-window producer in adapters; second read-stream consumer + detector orchestration in `src-tauri`; `status_changed` event; `*_impl` fake tests. | ~300–400 | Borderline |
| **PR4** | SessionRegistry + Session aggregate | Core `SessionRegistry` (create/lookup/close, `&self`), grown `Session`, `SessionId==PtyId` minting migration, `tauri::State` wiring, spawn/close commands. | ~350–450 | **RISK: likely >400** → may split registry (Core) vs command wiring (tauri) |
| **PR5** | Provisioner (Layer-1) | `ProvisioningPort`, JSON managed-namespace editor (pure), scope resolver (injected predicate), atomic-write + `.spectty.bak` file-IO seam, `inject`/`retract` wiring on spawn/close + tests. | ~380–460 | **RISK: likely >400** → may split pure editor/scope vs file-IO+wiring |
| **PR6** | Stub MCP server + spawn/status UI | `spectty-mcp` stub binary (stdio, advertises 5 tool schemas, no effects) + React spawn dialog (agent + cwd picker), Pane-header status badge, session title, `useSession` vitest. | ~300–420 | Borderline (binary + UI) |

**Budget verdict**: PR2, PR4, PR5 are the three most likely to exceed the 400-line review budget; `sdd-tasks`
should plan splittable sub-boundaries for each (noted inline). PR1, PR3, PR6 are borderline-OK. With splits,
M2 realistically lands as **6–9 chained PRs stacked-to-main**.

## Risks / open questions (for sdd-spec & sdd-design)
- **R2 (time representation)** — `sdd-design` must PIN whether `OutputSignal` carries
  `idle_ms: u64`/`last_byte_elapsed_ms` vs an injected `Timestamp` from a `ClockPort`-style seam, and where
  the clock is injected (read loop vs producer). Lock 2 fixes "non-`Instant`, serde"; the exact field is a
  design call.
- **R4 (stub MCP server boundary)** — `sdd-design` must define exactly how the stub responds (ack vs error)
  so M3 can swap effects in without changing the registered schema. The registered entry/schema is the
  forward-compatible contract; effects are M3.
- **R5 (Claude Code `AwaitingInput` scraping)** — the permission-prompt / "Do you want…" patterns are
  empirical and brittle; exit-criterion (3) is a real-session manual check. `sdd-spec` should capture the
  observed patterns as data (a pattern list), not as Core logic, so they live testably in `ClaudeCodeRunner`.
- **R6 (producer placement)** — `sdd-design` must confirm the `OutputSignal` producer runs on a path that
  cannot back-pressure the M1 `pty_output` render Channel (independent consumer of the read stream; bounded
  buffer; drop-oldest on overflow).
- **R7 (marker coexistence)** — the JSON namespace editor must own ONLY `spectty_*` keys and never touch
  user or gentle-ai keys; `sdd-spec` should make "round-trips foreign keys untouched" an explicit, tested
  property of the editor.
- **R8 (Provisioner lifecycle on crash)** — if Spectty crashes between `inject` and `retract`, the managed
  `spectty_*` keys leak into the agent config. `sdd-design` should decide whether M2 needs startup
  reconciliation ("retract orphans on boot") or defers it; the `.spectty.bak` restore is the manual escape
  hatch.
- **R9 (ADR-0004 trait shape drift)** — Lock 1 overrides the `AgentRunner::provisioner()` method shown in
  ADR-0004/agent-abstraction. `sdd-design` should note the ADR text as superseded-for-M2 (separate
  `ProvisioningPort`) so future readers do not re-introduce the coupling.
