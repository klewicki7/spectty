# Explore — M2: Spawn Agent + Provisioner

> SDD explore phase. Investigation only (no code, no proposal). Maps the current
> M0+M1 scaffold onto roadmap M2, ADR-0004/0006, and the
> agent-abstraction / agent-protocol / domain-model / pty-layer docs.
> Mirrored in engram: topic `sdd/M2-spawn-agent-provisioner/explore` (obs 799).

## Why

M2 must add: `AgentRunner` port + ClaudeCode/Generic adapters; `AgentStatus` state
machine + status detector; `Session` aggregate + `SessionRegistry` in Core;
`ProvisioningPort` + Provisioner (Claude Code MCP config injection); and
spawn/status/title UI — all WITHOUT breaking the Core quarantine
(`spectty-core` = serde + thiserror only, enforced by cargo-deny on `crates/core`).

## Current codebase seams

- `crates/core/src/entities/session.rs`: `Session { id, workspace, status, title }` —
  NO `AgentSpec` / `CostMetrics` / worktree / `created_at` yet (domain-model.md has a
  richer target).
- `crates/core/src/entities/agent_status.rs`: `enum AgentStatus { Starting, Idle,
  Running, AwaitingInput, Completed, Error }` — `Copy`, variants only, transition rules
  "deferred to M2".
- `crates/core/src/ports/persistence.rs`: `PersistencePort: Send + Sync`, `&self`
  interior-mutability, upsert/get with a pre-serialized `String` payload — THE pattern
  every new M2 port must follow.
- `crates/adapters/src/pty/config.rs`: `PtySpawnConfig` — doc explicitly "NOT an
  AgentSpec/LaunchSpec yet, agent typing is M2+" → seam for `AgentRunner::launch_spec`.
- `crates/adapters/src/pty/{adapter,transport,coalescer}.rs`: `PtyAdapter::spawn` returns
  `(adapter, Box<dyn Read>)`; `PtyTransport` write/resize/kill seam; `Coalescer` batches
  RAW bytes for the UI, does NO ANSI strip (it is NOT `OutputSignal`).
- `src-tauri/src/pty_state.rs`: `PtyRegistry(Mutex<HashMap<PtyId, PtyState>>)` — doc says
  "M2 introduces the real SessionRegistry, this is NOT it". `PtyId = String`.
- `src-tauri/src/commands/pty.rs`: `pty_spawn` / `send_input` / `pty_resize` / `pty_kill`
  via `*_impl` free fns (fake-tested); `spawn_read_thread` runs a read-thread (mpsc) + a
  forwarder-thread (Coalescer + Channel + AppHandle), `forward_step()` pure decision;
  ONLY emits `pty_exit { id, code }`. `next_pty_id()` — comment says M2 SessionRegistry
  owns id minting.
- `src-tauri/src/lib.rs`: `generate_handler!` — new M2 commands MUST be registered or they
  silently fail.
- `ui/`: React 19 + xterm; `useTerminal.ts` spawns one PTY, listens `pty_exit`
  ("future status UI" = M2); `App.tsx` single `<Terminal/>`, no pane header / session
  chrome.
- **OutputSignal seam**: pty-layer.md defines `OutputSignal { text_window (ANSI-stripped
  String), is_active, exit_code, last_byte_at: Instant }` for `AgentRunner::detect_status`.
  **TODAY ONLY THE RAW PATH EXISTS** — the `OutputSignal` producer (ANSI strip + rolling
  window) is NEW M2 code with no current home.

## Approaches / leans

- **2A — AgentRunner shape**: lean = `ProvisioningPort` SEPARATE from `AgentRunner`
  (not `AgentRunner::provisioner() -> Box<dyn>`); implement
  `launch_spec` / `detect_status` / `descriptor` / `tier` fully, `parse_cost` /
  `quick_actions` as honest skeletons. Idle-timeout lives in
  `GenericRunner::detect_status` with injected time (Coalescer-style), NOT in Core.
- **2B — status detection**: lean = `OutputSignal` is a Core serde struct; per-agent
  `detect_status(&OutputSignal) -> Option<AgentStatus>` is PURE; PLUS a pure Core
  `transition(current, observed) -> AgentStatus` enforcing legal transitions. Detector
  invoked from `src-tauri` (owns read loop + runner). Reject "detector in adapter"
  (violates ADR-0004). New Tauri event `status_changed { session_id, status,
  quick_actions }` (data-flow.md).
- **2C — Provisioner**: lean = format-aware editors. JSON managed-NAMESPACE editor (owns
  only `spectty_*` keys) for `~/.claude.json` + `.mcp.json`; text managed-marker editor
  for markdown/SKILL if in scope. Pure `String -> String` editors; atomic-write
  (tmp + fsync + rename) + `.spectty.bak` backup is the impure shell behind a file-IO
  seam. Reject text-markers-for-JSON (corrupts) and `claude mcp add` subprocess (not
  atomic/testable; the ADR designs a file provisioner).
- **2D — registries**: lean = TWO coexisting registries. Core `SessionRegistry` owns
  Session aggregates (`PersistencePort`-style `&self` interior mutability, shared as
  `tauri::State`); `src-tauri` `PtyRegistry` keeps OS handles. `SessionId == PtyId` unify
  (`next_pty_id` migrates to `SessionId` minting). Reject collapsing Session into
  `src-tauri` (violates "aggregate root in Core").

## Verified external facts (code.claude.com, 2026-06)

- Claude Code MCP stdio entry: `{ "mcpServers": { "<name>": { "command", "args", "env" } } }`.
- SCOPE → FILE: `user` (= roadmap GLOBAL) = TOP-LEVEL `mcpServers` in `~/.claude.json`;
  `project` (= roadmap PROJECT) = `.mcp.json` at repo root; `local` = `~/.claude.json`
  under `projects["/abs/path"].mcpServers`.
- GOTCHA: `~/.claude.json` is ONE big nested JSON file (not a dir, not
  `~/.claude/.mcp.json`) → MUST use a JSON-structural editor; text markers corrupt it.
- Hooks (Layer-2 `additionalContext`): `SessionStart` + `UserPromptSubmit` emit
  `hookSpecificOutput.additionalContext`, configured in `.claude/settings.json` (project)
  or `~/.claude/settings.json` (user) — a DIFFERENT file from MCP config.

## Risks / unknowns (for propose/design to resolve)

- **R2**: `OutputSignal.last_byte_at: Instant` is NOT serde-serializable + monotonic-only;
  if `OutputSignal` is a Core serde type, model time as elapsed-millis or via a
  `ClockPort` (domain-model.md lists `ClockPort`). Decide in design.
- **R3**: scope — does M2 ship all 3 injection layers or just Layer-1 MCP registration?
  Roadmap reads as Layer-1 primarily. Recommend M2 = Layer-1 + teardown; Layers 2/3 (need
  the live Spec) defer to M3. CONFIRM in propose.
- **R4**: the `spectty_*` MCP server binary must exist + run (stdio per session); its
  EFFECTS (persist spec, trigger diff) are M3. M2 likely registers the entry but the
  server is a stub / M3. Flag.
- **R5**: Claude Code `AwaitingInput` in M2 = same PTY-scraping as Generic (the
  Cooperative `spectty_approval` path is M3); patterns are empirical/brittle, validate
  against a real session.
- **R6**: the `OutputSignal` producer (ANSI strip + window) is new code, belongs in the
  adapter / read-loop, and must not block raw `pty_output` (independent streams). Decide
  placement.
- **R7**: gentle-ai marker coexistence (OPEN in agent-abstraction.md) — mitigate via
  `spectty_*`-key-only ownership.
- **GLOBAL-vs-PROJECT detection**: roadmap "PROJECT when file committed" → a minimal
  git-tracked probe (a proper `GitPort` is M4); default to GLOBAL / user scope.

## Strict-TDD seams

- **PURE (no fakes)**: `AgentStatus` transition fn; per-agent `detect_status`;
  `GenericRunner` idle-timeout (injected time); `launch_spec` mapping; JSON
  managed-namespace editor; text managed-marker editor; `AgentSpec` parsing; scope
  resolution (injected git-tracked predicate).
- **NEED FAKES / seams**: file-IO (atomic write + backup behind a `PtyTransport`-style
  trait); `OutputSignal` producer wiring (src-tauri thread harness); `SessionRegistry`
  command wiring (Tauri State + `*_impl` split); real-PTY `AgentRunner` spawn integration
  test (`#[cfg(unix)]` template already in `commands/pty.rs`).

## Recommendation

Proceed to `sdd-propose`. Lock:
1. `AgentRunner` method subset + separate `ProvisioningPort`.
2. `OutputSignal` Core serde type with non-`Instant` time + producer placement.
3. M2 = Layer-1 MCP registration only.
4. `spectty_*` MCP server ships vs stubs in M2.
5. JSON managed-namespace editor for `~/.claude.json` / `.mcp.json`.
6. Two registries, Session aggregate in Core.
