# M2 — Spawn Agent + Provisioner: Technical Design

> Status: design (the HOW at architectural level). Consumed by `sdd-tasks`.
> Reads: proposal (obs 800 / `proposal.md`), explore (obs 799 / `explore.md`), the **real**
> merged M0+M1 code (verified on disk: `crates/core/src/entities/{session,agent_status,workspace}.rs`,
> `crates/core/src/ports/persistence.rs`, `crates/adapters/src/pty/{adapter,transport,coalescer,config}.rs`,
> `src-tauri/src/{lib.rs,pty_state.rs,commands/pty.rs}`), ADR-0004, ADR-0006, and the
> `agent-abstraction` / `agent-protocol` / `domain-model` / `pty-layer` / `data-flow` docs.
> ADR/D-series continues from M1 (M1 used D1–D6); M2 introduces **D7–D20**.

## 0. Design Goals & Non-Goals

**Goal**: turn the M1 raw terminal into a *supervised agent session*. Spawn a real AI CLI agent in
the PTY through a Core `AgentRunner` port (`ClaudeCodeRunner` + `GenericRunner`, ZERO agent names in
Core), detect its `AgentStatus` via a pure transition machine fed by an ANSI-stripped `OutputSignal`,
own the `Session` aggregate in a Core `SessionRegistry`, and inject/retract the Spectty Agent
Protocol's Layer-1 MCP-tool registration through a separate Core `ProvisioningPort`. A stub
`spectty-mcp` binary makes the registered config inspectable and the agent startable.

**Non-Goals (M3/M4)**: Layer-2 `additionalContext` hook + Layer-3 SKILL.md injection; live `Spec`
aggregate behavior + `spec_updated` polling; `spectty_*` tool *effects* (persist/diff/approval/cost
ingestion); real `parse_cost` depth + `CostMetrics` accumulation; structured `spectty_approval`
`AwaitingInput`; `quick_actions` real prompt-answering; Worktrees / `GitPort` / Checkpoints;
multi-session UI / split tree; non-Claude-Code format adapters; Provisioner refresh hook + SHA cache.

**Hard invariant (the gate)**: `crates/core/Cargo.toml` gains **NOTHING** (stays `serde` +
`thiserror`). `AgentRunner`, `ProvisioningPort`, `OutputSignal`, `AgentSpec`, `AgentTier`,
`AgentDescriptor`, the `transition` fn, `ClockPort`, and `SessionRegistry` are Core-pure: no
`portable-pty`, no `serde_json`, no `tauri`, no ANSI/regex, no file-IO. `cargo deny --manifest-path
crates/core/Cargo.toml check bans` stays green because the scoped closure never sees those crates.
All agent names (`claude`, `bash`), config-format knowledge, ANSI/regex parsing, and file-IO live in
`crates/adapters` / `src-tauri`.

---

## 1. Architecture Approach

**Pattern**: hexagonal, exactly as M0/M1 established. M2 *promotes* the supervision concepts into Core
ports + an aggregate while keeping `src-tauri` as the single composition root that owns the live
process, the read loop, and the Tauri state.

```
ui (xterm.js + React 19): spawn dialog, Pane-header status badge, session title
   │  invoke(spawn_session/close_session/list_sessions/get_session)  +  Channel<Vec<u8>>  +  listen(status_changed/session_created/session_closed/pty_exit)
   ▼
src-tauri  (composition root — the ONLY tauri-aware + the ONLY OS-handle-owning layer)
   │  SessionRegistry (Core, tauri::State) + PtyRegistry (OS handles, tauri::State) + ProvisioningPort (tauri::State)
   │  on spawn: resolve runner → runner.launch_spec(ctx) → provisioner.inject(scope) → PtyAdapter::spawn
   │  read loop: raw coalesce → pty_output Channel (M1, UNCHANGED)  ‖  OutputSignal producer → runner.detect_status → Core transition → registry.update → status_changed
   │  on close: PtyAdapter kill (M1) + provisioner.retract(scope)
   ▼
crates/adapters
   │  ClaudeCodeRunner / GenericRunner (impl AgentRunner) ; OutputSignalProducer (ANSI strip + rolling window) ;
   │  ClaudeJsonProvisioner (impl ProvisioningPort) over the pure JSON namespace editor + atomic-write file-IO seam ;
   │  SystemClock (impl ClockPort) ; is_git_tracked probe
   ▼
crates/core  (UNTOUCHED Cargo: serde + thiserror)
   │  ports: AgentRunner, ProvisioningPort, ClockPort  ;  entities: Session (grown), AgentStatus + transition(),
   │  SessionRegistry, AgentSpec, AgentTier, AgentDescriptor, OutputSignal, QuickAction, CostDelta
```

The dependency arrows are unchanged: `src-tauri` and `adapters` depend on `core`; nothing new points
*into* core. New crate: `crates/spectty-mcp` (a binary, depends on serde/serde_json/stdio only — NOT
on core, NOT on tauri; it is a child process the agent launches, not part of the bridge graph).

---

## 2. Module / File Layout

### `crates/core/src/` — new + grown (the quarantine boundary)
```
crates/core/src/
├── entities/
│   ├── agent_status.rs       # GROW: add `transition(current, observed) -> AgentStatus` (pure fn) + ObservedStatus
│   ├── agent_spec.rs         # NEW: AgentSpec, AgentKind(serde string), AgentTier, AgentDescriptor, AgentCapabilities
│   ├── output_signal.rs      # NEW: OutputSignal (serde, non-Instant time field) + QuickAction + CostDelta
│   ├── session.rs            # GROW: Session gains agent: AgentSpec, created_at: Timestamp; keep id/workspace/status/title
│   └── session_registry.rs   # NEW: SessionRegistry (&self interior mutability, Send+Sync), SessionSummary
├── ports/
│   ├── agent_runner.rs       # NEW: AgentRunner trait + LaunchSpec + LaunchContext
│   ├── provisioning.rs       # NEW: ProvisioningPort trait + ProvisioningScope + ProvisioningError + ProvisioningHandle
│   └── clock.rs              # NEW: ClockPort trait + Timestamp newtype
└── (lib.rs re-exports the new types)
```

### `crates/adapters/src/` — new
```
crates/adapters/src/
├── agent/
│   ├── mod.rs                # pub use; re-exports the runners + producer
│   ├── generic.rs            # GenericRunner: launch_spec(user cmd) + idle-timeout detect_status (injected time)
│   ├── claude_code.rs        # ClaudeCodeRunner: launch_spec("claude") + scrape detect_status; PATTERNS as data
│   └── output_signal.rs      # OutputSignalProducer: ANSI strip state machine + rolling window assembler (pure step)
├── provision/
│   ├── mod.rs
│   ├── json_namespace.rs     # PURE String->String managed-namespace editor (owns only spectty_* keys)
│   ├── scope.rs              # resolve_scope(default Global; Project when is_git_tracked) — injected predicate
│   ├── file_io.rs            # AtomicConfigFile seam trait + RealConfigFile (tmp+fsync+rename, .spectty.bak)
│   └── claude_provisioner.rs # ClaudeJsonProvisioner: impl ProvisioningPort over editor + scope + file_io
└── clock.rs                  # SystemClock: impl ClockPort
```
`crates/adapters/src/lib.rs` re-exports `AgentRunnerRegistry` factory, `ClaudeCodeRunner`,
`GenericRunner`, `OutputSignalProducer`, `ClaudeJsonProvisioner`, `SystemClock`.

### `src-tauri/src/` — new + grown
```
src-tauri/src/
├── commands/
│   ├── mod.rs                # + pub mod session;
│   └── session.rs            # spawn_session / close_session / list_sessions / get_session (+ *_impl free fns) + StatusChanged payload
├── pty_state.rs              # GROW: PtyId becomes the SessionId string; next_pty_id retired (registry mints)
├── session_runtime.rs        # NEW: the read-loop's SECOND consumer wiring — OutputSignal producer thread + detect/transition/emit (pure detect_step)
└── lib.rs                    # register new commands + .manage(SessionRegistry/Provisioner)
```

### `crates/spectty-mcp/` — new binary crate (stub server)
```
crates/spectty-mcp/
├── Cargo.toml                # serde, serde_json only; NO core, NO tauri
└── src/
    └── main.rs               # stdio JSON-RPC loop: initialize handshake + tools/list (5 schemas) + tools/call (ack, no effects)
```

### New UI files — `ui/src/`
```
ui/src/
├── components/
│   ├── SpawnDialog.tsx       # agent picker (Claude Code | Generic+cmd) + cwd picker → spawn_session
│   └── PaneHeader.tsx        # session title + AgentStatus badge (reacts to status_changed)
├── hooks/
│   └── useSession.ts         # spawn/close orchestration + status_changed/session_* listeners (mirrors useTerminal)
└── session/
    └── ipc.ts                # typed wrappers: spawnSession/closeSession/listSessions/getSession + event types
ui/tests/unit/useSession.test.ts   # vitest: mock invoke + listen; assert spawn/status-badge/close
```

### Manifest / config deltas
| Manifest | Add | Note |
|---|---|---|
| `crates/core/Cargo.toml` | **nothing** | the gate |
| `crates/adapters/Cargo.toml` | `serde_json` (JSON editor), `regex` or hand-rolled scanners (scraping; prefer hand-rolled to avoid regex dep — see D11) | already has `portable-pty` from M1 |
| `crates/spectty-mcp/Cargo.toml` | `serde`, `serde_json` | new workspace member |
| `Cargo.toml` (workspace) | `+ "crates/spectty-mcp"` member | |
| `src-tauri/Cargo.toml` | (none beyond M1) | wires Core + adapters only |
| `capabilities/default.json` | expected NONE (custom commands + Channel + events ride `core:default`/`core:event:default`); verify-only contingency | same reasoning as M1 §7 |

---

## 3. Core types & ports (signatures — code-shaped, Core-pure)

### 3.1 `crates/core/src/entities/agent_spec.rs` (NEW)
```rust
use serde::{Deserialize, Serialize};

/// Which agent a Session runs and at what cooperation tier. PURE data — no agent
/// behavior, no command strings live here; the runner adapter maps `kind` to a launch.
/// `AgentKind` is a serde STRING (not a closed enum) so adding an agent never edits Core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    /// Opaque agent identifier the runner registry resolves to a concrete runner,
    /// e.g. "claude-code" | "generic". The Core NEVER branches on this value.
    pub kind: AgentKind,
    /// For the Generic agent: the user-supplied program + args. `None` for first-class
    /// agents that derive their launch from `kind`.
    pub command: Option<Vec<String>>,
    pub tier: AgentTier,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentKind(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentTier { Cooperative, Generic }

/// UI-facing identity + capabilities so the UI degrades gracefully (ADR-0004).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDescriptor {
    pub kind: AgentKind,
    pub display_name: String,
    pub tier: AgentTier,
    pub capabilities: AgentCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub reports_cost: bool,
    pub structured_permissions: bool,
    pub emits_diff_signals: bool,
    pub requires_provisioning: bool, // Generic = false → no ProvisioningPort wired (Lock 1)
}
```
> `requires_provisioning` is how the composition root decides whether to call `inject`/`retract` for a
> Session WITHOUT the Generic runner having to carry a `provisioner()` method (the R9/D7 separation).

### 3.2 `crates/core/src/ports/clock.rs` (NEW — the non-`Instant` time seam, R2)
```rust
use serde::{Deserialize, Serialize};

/// Monotonic-ish elapsed time in milliseconds since an opaque process epoch. PURE,
/// serde-safe, and comparable across the port boundary (unlike std::time::Instant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Timestamp(pub u64); // millis since the ClockPort's epoch

/// Time source, injected for testability (domain-model.md lists ClockPort). The Core
/// reads time ONLY through this; the concrete `SystemClock` lives in adapters.
pub trait ClockPort: Send + Sync {
    fn now(&self) -> Timestamp;
}
```

### 3.3 `crates/core/src/entities/output_signal.rs` (NEW — Lock 2, R2)
```rust
use serde::{Deserialize, Serialize};
use crate::ports::clock::Timestamp;

/// A normalized, decoded view of recent PTY output for `AgentRunner::detect_status`.
/// Core serde type (crosses the port boundary). NO Instant, NO raw ANSI — the producer
/// in adapters strips ANSI and windows the text before constructing this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputSignal {
    /// ANSI-stripped printable text of the last N chars (bounded rolling window, ~4 KB).
    pub text_window: String,
    /// True while bytes are arriving within the quiesce window; drives idle heuristics.
    pub is_active: bool,
    /// Child exit code once the process has exited (None while running).
    pub exit_code: Option<i32>,
    /// Wall-of-clock timestamp of the most recent byte (ClockPort-derived, serde-safe).
    pub last_byte_at: Timestamp,
    /// Elapsed millis since the last byte AS OF signal construction. This is the field
    /// `GenericRunner` reads for idle-timeout — precomputed so `detect_status` stays a
    /// pure function of the signal (no clock access inside the Core port impl).
    pub idle_ms: u64,
}

/// A pre-canned answer the UI can offer for a known prompt (skeleton in M2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickAction { pub id: String, pub label: String, pub bytes: Vec<u8> }

/// A token/cost delta (skeleton in M2 — parse_cost returns None).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CostDelta { pub input_tokens: u64, pub output_tokens: u64, pub estimated_usd: f64 }
```
> **R2 PINNED.** `OutputSignal` carries BOTH `last_byte_at: Timestamp` (for ordering/auditing) AND a
> precomputed `idle_ms: u64`. `detect_status` reads `idle_ms` only — it is a PURE function of the
> signal, never touches a clock. The clock is injected at the PRODUCER (src-tauri read loop owns the
> `Arc<dyn ClockPort>`); the producer stamps `last_byte_at` on each byte and computes `idle_ms = now -
> last_byte_at` at signal-emit time. This keeps the only impurity (reading the clock) in the adapter
> layer and makes every `detect_status` test a table of `(idle_ms, text_window) -> Option<AgentStatus>`.

### 3.4 `crates/core/src/entities/agent_status.rs` (GROW — the pure state machine)
```rust
// existing enum kept verbatim: Starting, Idle, Running, AwaitingInput, Completed, Error

/// What the per-agent detector observed from one OutputSignal. Distinct from AgentStatus
/// so the legal-transition policy lives in ONE pure place, not smeared across detectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// Agent reached a ready/quiescent prompt.
    Ready,
    /// Agent is actively producing task output.
    Working,
    /// Agent is blocked on a human prompt (permission / question).
    NeedsInput,
    /// Agent finished cleanly (exit 0 OR Generic idle-timeout elapsed).
    Finished,
    /// Agent failed (non-zero exit / crash).
    Failed,
}

/// PURE legal-transition policy. Given the current status and an observed signal,
/// return the next status. Illegal/no-op observations return `current` unchanged.
/// This is the SINGLE authority for the state machine in domain-model.md:
/// Starting ─ready→ Idle ─task→ Running ─prompt→ AwaitingInput ─input→ Running
///                                       └ done→ Completed ; any ─fail→ Error
#[must_use]
pub fn transition(current: AgentStatus, observed: Observed) -> AgentStatus {
    use AgentStatus::*;
    use Observed::*;
    match (current, observed) {
        // terminal states are absorbing (no resurrection without a new Session)
        (Completed | Error, _)            => current,
        (_, Failed)                       => Error,
        (_, Finished)                     => Completed,
        (Starting, Ready)                 => Idle,
        (Idle, Working)                   => Running,
        (Running, NeedsInput)             => AwaitingInput,
        (AwaitingInput, Working)          => Running,
        (AwaitingInput, Ready)            => Running, // input consumed, output resumed
        (Idle, NeedsInput)                => AwaitingInput,
        (Running, Ready)                  => Running, // still running, quiescent burst
        (Starting, Working)               => Running, // skipped Idle (immediate task)
        _                                 => current, // no legal change → no event
    }
}
```
**Legal-transition table (the policy `transition` encodes):**

| current \ observed | Ready | Working | NeedsInput | Finished | Failed |
|---|---|---|---|---|---|
| Starting | →Idle | →Running | →AwaitingInput | →Completed | →Error |
| Idle | Idle | →Running | →AwaitingInput | →Completed | →Error |
| Running | Running | Running | →AwaitingInput | →Completed | →Error |
| AwaitingInput | →Running | →Running | AwaitingInput | →Completed | →Error |
| Completed | Completed | Completed | Completed | Completed | Completed |
| Error | Error | Error | Error | Error | Error |

> Only cells that CHANGE the status produce a `status_changed` event (the caller diffs old vs new).
> Terminal states (`Completed`/`Error`) are absorbing.

### 3.5 `crates/core/src/ports/agent_runner.rs` (NEW — Lock 1, D7/D8)
```rust
use crate::entities::agent_spec::AgentDescriptor;
use crate::entities::agent_status::AgentStatus;
use crate::entities::output_signal::{CostDelta, OutputSignal, QuickAction};

/// Per-agent context for launching. PURE: holds the resolved workspace cwd + size +
/// the session id (so the agent can be told its session_id via env for the MCP tools).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContext {
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub session_id: String,
    /// Optional user command for the Generic agent (ignored by first-class runners).
    pub user_command: Option<Vec<String>>,
}

/// What to spawn in a PTY. Core-pure mirror of PtySpawnConfig (which lives in adapters).
/// The composition root maps this 1:1 onto `PtySpawnConfig`. env is a sorted Vec of
/// (key, value) pairs (deterministic for tests; no HashMap ordering noise).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
}

/// The single agent-agnostic port (ADR-0004). The Core has ZERO agent names; each impl
/// encapsulates one agent. M2 subset: launch_spec/detect_status/descriptor/tier FULL;
/// parse_cost/quick_actions are honest, tested SKELETONS (return None / empty).
///
/// NOTE (R9/D7): ADR-0004 sketched a `provisioner(&self) -> Option<Box<dyn Provisioner>>`
/// method on this trait. M2 OVERRIDES that: provisioning is a separate `ProvisioningPort`
/// (domain-model.md). This trait carries NO provisioner method.
pub trait AgentRunner: Send + Sync {
    fn launch_spec(&self, ctx: &LaunchContext) -> LaunchSpec;
    fn detect_status(&self, signal: &OutputSignal) -> Option<Observed>;
    fn parse_cost(&self, _signal: &OutputSignal) -> Option<CostDelta> { None } // skeleton
    fn quick_actions(&self, _status: &AgentStatus) -> Vec<QuickAction> { Vec::new() } // skeleton
    fn descriptor(&self) -> AgentDescriptor;
}
```
> **DESIGN REFINEMENT (D8): `detect_status` returns `Option<Observed>`, not `Option<AgentStatus>`.**
> The proposal/ADR-0004 wrote `detect_status -> Option<AgentStatus>`. We split it: the per-agent
> detector returns `Observed` (what it SAW), and the pure Core `transition` decides the resulting
> `AgentStatus`. This keeps the legal-transition policy in ONE place (not duplicated across every
> runner) and means a runner can never illegally jump (e.g. directly to `Completed` from a scrape).
> A runner returning `None` means "no observation this tick" → no transition, no event.

### 3.6 `crates/core/src/ports/provisioning.rs` (NEW — Lock 1/3/5, D7)
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProvisioningError {
    #[error("provisioning io error: {0}")] Io(String),
    #[error("config parse error: {0}")] Parse(String),
}

/// Where to inject the Spectty Agent Protocol for an agent. PathBuf would pull `std::path`
/// (fine in Core) but we keep it a String to mirror the rest of the Core's String-path
/// convention and avoid OsString serde edge cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisioningScope {
    Global,            // ~/.claude.json top-level mcpServers
    Project(String),   // <repo_root>/.mcp.json
}

/// Opaque handle returned by inject, carried by the Session so close() can retract the
/// exact scope that was injected (avoids re-resolving scope at teardown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisioningHandle { pub scope: ProvisioningScope }

/// Core port for injecting/retracting the Spectty Agent Protocol. SEPARATE from AgentRunner
/// (Lock 1). M2 = Layer-1 MCP-tool registration + teardown only (Lock 3). `&self` interior
/// mutability + Send+Sync, exactly like PersistencePort, so it shares as `tauri::State`.
pub trait ProvisioningPort: Send + Sync {
    /// Inject the spectty_* MCP server entry at `scope`, idempotently. Returns a handle.
    fn inject(&self, scope: ProvisioningScope) -> Result<ProvisioningHandle, ProvisioningError>;

    /// Remove the spectty_* keys at the handle's scope on session close. Idempotent:
    /// retracting an already-clean config is Ok(()).
    fn retract(&self, handle: &ProvisioningHandle) -> Result<(), ProvisioningError>;
}
```
> M2 deliberately OMITS `refresh()` (Layer-2 dynamics, M3) from the trait. Adding it later is additive.

### 3.7 `crates/core/src/entities/session.rs` (GROW) + `session_registry.rs` (NEW — Lock 6)
```rust
// session.rs — grown toward domain-model.md (Spec/Cost/Worktree/last_diff still deferred):
pub struct Session {
    pub id: SessionId,
    pub workspace: WorkspaceId,
    pub agent: AgentSpec,          // NEW
    pub status: AgentStatus,
    pub title: String,
    pub created_at: Timestamp,     // NEW (ClockPort-derived)
}

// session_registry.rs — the Core aggregate-root registry (Lock 6):
use std::collections::HashMap;
use std::sync::Mutex;

/// UI-facing projection for list_sessions (data-flow.md SessionSummary).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary { pub id: SessionId, pub title: String, pub status: AgentStatus, pub agent_kind: AgentKind }

/// Owns Session aggregates. `&self` interior mutability (PersistencePort convention) so a
/// single registry shares across command handlers + the read-loop thread as `tauri::State`
/// behind `Arc`/`State`. The Mutex is the ONLY mutability, encapsulated here.
#[derive(Default)]
pub struct SessionRegistry { inner: Mutex<RegistryInner> }
struct RegistryInner { sessions: HashMap<SessionId, Session>, next_id: u64 }

impl SessionRegistry {
    /// Mint a fresh SessionId (migrates M1's next_pty_id counter into the aggregate root).
    pub fn mint_id(&self) -> SessionId { /* fetch_add on inner.next_id, render as String */ }

    /// Insert a fully-formed Session (created after launch_spec + provisioning succeed).
    pub fn insert(&self, session: Session);

    /// Apply an observed status through the pure `transition`; returns Some(new) on CHANGE
    /// (so the caller emits status_changed) or None when unchanged. Keeps the diff in Core.
    pub fn apply_observed(&self, id: &SessionId, observed: Observed) -> Option<AgentStatus>;

    pub fn get(&self, id: &SessionId) -> Option<Session>;
    pub fn summaries(&self) -> Vec<SessionSummary>;

    /// Remove on close; returns the removed Session (carrying its agent/title for events).
    pub fn remove(&self, id: &SessionId) -> Option<Session>;
}
```
> **`SessionId == PtyId` unification (Lock 6, D13).** `SessionRegistry::mint_id` becomes the SOLE id
> minter; `src-tauri`'s `next_pty_id()` is RETIRED. `pty_state.rs`'s `PtyId` becomes a type alias for
> the SessionId string (`pub type PtyId = String;` stays, but the value now comes from
> `registry.mint_id()`). The two registries (`SessionRegistry` Core-aggregate, `PtyRegistry` OS-handle)
> are keyed by the SAME string → no cross-mapping table, they move in lockstep. `apply_observed` is the
> ONLY place `transition` is called in production (the registry diffs old vs new under its own lock,
> avoiding a check-then-act race between the read-loop thread and a concurrent close).

---

## 4. Adapter design (impure — agent names, ANSI, file-IO live here)

### 4.1 `GenericRunner` (`crates/adapters/src/agent/generic.rs`)
```rust
pub struct GenericRunner { idle_timeout_ms: u64 } // configurable; e.g. 3000

impl AgentRunner for GenericRunner {
    fn launch_spec(&self, ctx: &LaunchContext) -> LaunchSpec {
        // user_command or fall back to the per-OS default shell (reuse config::default_shell)
    }
    fn detect_status(&self, s: &OutputSignal) -> Option<Observed> {
        if s.exit_code == Some(0) { return Some(Observed::Finished); }
        if matches!(s.exit_code, Some(_)) { return Some(Observed::Failed); }
        if s.is_active { return Some(Observed::Working); }
        if s.idle_ms >= self.idle_timeout_ms { return Some(Observed::Finished); } // exit-criterion 5
        Some(Observed::Ready) // quiescent but not yet timed out → Idle on first, no-op after
    }
    fn descriptor(&self) -> AgentDescriptor { /* Generic tier, requires_provisioning: false */ }
}
```
> Idle-timeout lives HERE with INJECTED time (via `OutputSignal.idle_ms`, the ClockPort-derived field),
> NOT in Core — exactly the proposal's lean. Pure-testable: feed `OutputSignal { idle_ms, is_active,
> exit_code }` and assert the `Observed`.

### 4.2 `ClaudeCodeRunner` (`crates/adapters/src/agent/claude_code.rs`) — R5
```rust
/// Empirical scraping PATTERNS as DATA (R5) — co-located with the agent, never in Core.
/// A pattern is matched against the ANSI-stripped text_window. Hand-rolled `contains`/
/// line-scan matchers (D11) keep `regex` out of the dep graph where possible.
struct ClaudePatterns {
    awaiting_input: &'static [&'static str], // e.g. ["Do you want to", "❯ 1. Yes", "(y/n)", "Press Enter to continue"]
    ready:          &'static [&'static str], // e.g. ["? for shortcuts", the idle prompt box]
}

pub struct ClaudeCodeRunner { patterns: ClaudePatterns }

impl AgentRunner for ClaudeCodeRunner {
    fn launch_spec(&self, ctx: &LaunchContext) -> LaunchSpec {
        // program: "claude"; args: []; env: [("SPECTTY_SESSION_ID", ctx.session_id)]; cwd: ctx.cwd
    }
    fn detect_status(&self, s: &OutputSignal) -> Option<Observed> {
        if s.exit_code == Some(0) { return Some(Observed::Finished); }
        if matches!(s.exit_code, Some(_)) { return Some(Observed::Failed); }
        if self.patterns.awaiting_input.iter().any(|p| s.text_window.contains(p)) { return Some(Observed::NeedsInput); }
        if self.patterns.ready.iter().any(|p| s.text_window.contains(p)) { return Some(Observed::Ready); }
        if s.is_active { return Some(Observed::Working); }
        None // no confident observation → no transition
    }
    fn descriptor(&self) -> AgentDescriptor { /* Cooperative, requires_provisioning: true, structured_permissions: false (M2) */ }
}
```
> The pattern LIST is the unit-test surface (R5): each pattern → asserted `Observed`. Patterns are
> placeholders refined against a real Claude Code session at acceptance (exit-criterion 3 is a manual
> check). Refining a pattern is a one-line data edit + a new unit test, never a Core change.

### 4.3 `OutputSignalProducer` (`crates/adapters/src/agent/output_signal.rs`) — Lock 2, R6
```rust
/// Stateful ANSI-stripper + rolling-window assembler. PURE step (no clock, no thread):
/// it folds raw byte chunks into a bounded printable text window. The caller (src-tauri
/// read loop) supplies the clock-derived timestamps when it constructs the OutputSignal.
pub struct OutputSignalProducer {
    window: String,        // bounded ring (~WINDOW_BYTES, e.g. 4096)
    ansi_state: AnsiState, // tiny state machine: Ground | Esc | Csi | Osc
    window_bytes: usize,
}

impl OutputSignalProducer {
    pub fn new(window_bytes: usize) -> Self { /* ... */ }

    /// Fold one raw chunk in: strip ANSI (CSI/OSC/ESC sequences), append printable text,
    /// truncate the window from the front to `window_bytes`. PURE — returns nothing; state
    /// accumulates. Mirrors the Coalescer's "inject time, no I/O" discipline.
    pub fn ingest(&mut self, raw: &[u8]);

    /// Snapshot the current window into an OutputSignal. `last_byte_at`/`idle_ms`/`is_active`/
    /// `exit_code` are supplied by the caller (it owns the ClockPort + the child state).
    #[must_use]
    pub fn snapshot(&self, last_byte_at: Timestamp, idle_ms: u64, is_active: bool, exit_code: Option<i32>) -> OutputSignal;
}
```
> **R6 placement — the SECOND consumer that cannot back-pressure render (D9).** The M1 read pipeline
> is: read thread → `mpsc` → forwarder thread (Coalescer → `pty_output` Channel). We add the
> OutputSignal path as a THIRD thread fed by a SEPARATE BOUNDED `mpsc` (drop-oldest on overflow), tee'd
> off the SAME read thread:
>
> ```
> read thread ──┬─ tx_render.send(slice)      → forwarder → Coalescer → pty_output Channel   (M1, UNCHANGED, never blocked)
>               └─ tx_signal.try_send(slice)   → signal thread → OutputSignalProducer.ingest → detect/transition/emit
> ```
>
> The render `tx` keeps its M1 unbounded behavior so rendering is NEVER throttled. The signal `tx` is a
> BOUNDED `sync_channel` (e.g. capacity 64); the read thread uses `try_send` and DROPS the slice on a
> full buffer (status detection only needs the LATEST window — pty-layer.md explicitly says "older
> signals are dropped"). Dropping a signal slice never blocks the read thread, so it can never
> back-pressure the render path. The signal thread ticks on `recv_timeout(QUIESCE)` so `is_active`/
> `idle_ms` update even when the PTY is silent (the same quiescent-flush insight as M1's R3).

### 4.4 Provisioner (`crates/adapters/src/provision/`) — Lock 5, R7, R8

**`json_namespace.rs` — the PURE String→String editor (R7 — the core TDD unit):**
```rust
/// Insert/replace ONLY the spectty_* keys under `mcpServers`, leaving every foreign key
/// (user entries, gentle-ai entries) byte-for-byte intact on round-trip. PURE: takes the
/// current file text + the desired spectty entry, returns the new file text. No I/O.
pub fn inject_spectty_mcp(current_json: &str, server_name: &str, entry: &McpServerEntry)
    -> Result<String, ProvisioningError>;
pub fn retract_spectty_mcp(current_json: &str, server_name: &str)
    -> Result<String, ProvisioningError>;

/// The mcpServers value Spectty writes: { "command": "...", "args": [...], "env": {...} }.
pub struct McpServerEntry { pub command: String, pub args: Vec<String>, pub env: Vec<(String,String)> }
```
> Implementation: parse with `serde_json::Value`, mutate `obj["mcpServers"]["spectty"]` (GLOBAL: the
> top-level object; PROJECT: the `.mcp.json` root object), re-serialize pretty. The "owns only
> `spectty_*` keys" property (R7) is enforced by ONLY ever touching the `spectty`-prefixed sub-keys and
> is the headline tested invariant: `inject` then `retract` round-trips a file with foreign keys back to
> byte-identical (modulo serde's stable pretty formatting — the test fixture is pre-formatted to match).

**`scope.rs` — scope resolution (injected predicate):**
```rust
/// Default Global; Project(repo_root) when the agent config file is git-tracked. The
/// predicate is injected so this is a pure, table-tested function (proposal scope-detection).
pub fn resolve_scope(
    repo_root: Option<&str>,
    config_path: &str,
    is_git_tracked: impl Fn(&str) -> bool,
) -> ProvisioningScope;
```
The real `is_git_tracked` (adapter) shells `git ls-files --error-unmatch <path>` and maps exit-0 →
true. A full `GitPort` is M4.

**`file_io.rs` — the atomic-write seam (R8):**
```rust
/// Substitutable file-IO so the provisioner is testable without touching the real FS.
pub trait ConfigFile: Send + Sync {
    fn read(&self, path: &str) -> std::io::Result<Option<String>>; // None if absent
    /// Atomic: write `<path>.tmp` → fsync → rename over `path`. Backs up to `<path>.spectty.bak`
    /// on the FIRST write if no backup exists yet.
    fn write_atomic(&self, path: &str, contents: &str) -> std::io::Result<()>;
}
pub struct RealConfigFile;       // production: real tmp+fsync+rename+.bak
// tests use an in-memory FakeConfigFile (HashMap<String,String>) — the provisioner seam.
```

**`claude_provisioner.rs` — wires it together (impl `ProvisioningPort`):**
```rust
pub struct ClaudeJsonProvisioner<F: ConfigFile> {
    files: F,
    home_claude_json: String, // "~/.claude.json" resolved
    mcp_entry: McpServerEntry, // points at the spectty-mcp stub binary
}
impl<F: ConfigFile> ProvisioningPort for ClaudeJsonProvisioner<F> {
    fn inject(&self, scope) -> ... {
        let path = match &scope { Global => &self.home_claude_json, Project(root) => &format!("{root}/.mcp.json") };
        let current = self.files.read(path)?.unwrap_or_else(default_doc_for_scope);
        let next = inject_spectty_mcp(&current, "spectty", &self.mcp_entry)?;
        self.files.write_atomic(path, &next)?; // backs up first
        Ok(ProvisioningHandle { scope })
    }
    fn retract(&self, handle) -> ... { /* read → retract_spectty_mcp → write_atomic; absent file = Ok(()) */ }
}
```

> **R8 decision (D14): DEFER startup reconciliation; ship the `.spectty.bak` manual escape hatch.**
> If Spectty crashes between `inject` and `retract`, the `spectty` MCP key leaks in the agent config. We
> DEFER an automatic "retract orphans on boot" sweep to M3 because (a) the leaked key points at a real
> (stub) `spectty-mcp` binary, so it does NOT break the agent's startup — the failure mode is a stale
> harmless entry, not a corrupt config; (b) `retract` is idempotent, so the NEXT clean session close
> removes it; (c) a boot-time sweep needs a registry of "what we injected where", which is exactly the
> persistence-backed Session restore that M3 builds. The `.spectty.bak` backup is the documented manual
> reset ("Reset to pre-Spectty config"). M2 records the injected scope in the Session (via
> `ProvisioningHandle`) so a future M3 sweep has the data it needs. **This is a conscious, documented
> deferral, flagged for sdd-verify.**

### 4.5 `SystemClock` (`crates/adapters/src/clock.rs`)
```rust
pub struct SystemClock { epoch: std::time::Instant }
impl ClockPort for SystemClock { fn now(&self) -> Timestamp { Timestamp(self.epoch.elapsed().as_millis() as u64) } }
```

---

## 5. `spectty-mcp` stub server (`crates/spectty-mcp/`) — Lock 4, R4

A standalone binary the agent launches as its MCP server (stdio). It speaks the minimal MCP/JSON-RPC
subset Claude Code requires to enumerate tools, and **accepts** `tools/call` with an acknowledgement
but performs **no effects** (no persist, no diff, no approval) — effects are M3.

**Crate placement**: new workspace member `crates/spectty-mcp` (a `[[bin]]`). It depends on
`serde`/`serde_json` only — NOT on `spectty-core` (it must not pull Core into a child-process binary)
and NOT on tauri. The provisioner's `McpServerEntry.command` points at this binary's installed path.

**stdio handshake (JSON-RPC 2.0 over stdin/stdout, line-delimited or Content-Length framed — match
Claude Code's MCP stdio transport):**
1. `initialize` → respond `{ protocolVersion, capabilities: { tools: {} }, serverInfo: { name: "spectty-mcp", version } }`.
2. `notifications/initialized` → no-op.
3. `tools/list` → respond with the **5 advertised tool schemas** (verbatim shapes from agent-protocol.md):
   `spectty_spec`, `spectty_diff`, `spectty_approval`, `spectty_status`, `spectty_cost`.
4. `tools/call` → **ack/error contract (R4 — the forward-compat seam):**
   - Known tool name → JSON-RPC `result` with `{ content: [{ type: "text", text: "spectty-mcp (M2 stub): acknowledged <tool>; effects land in M3" }], isError: false }`.
   - Unknown tool name → JSON-RPC `error` `{ code: -32601, message: "unknown tool" }` (method-not-found).
   - Malformed params → JSON-RPC `error` `{ code: -32602, message: "invalid params" }`.

> **R4 forward-compat (D15).** The REGISTERED ENTRY + the 5 TOOL SCHEMAS are the stable contract M3
> must not change. M3 swaps the `tools/call` BODY (ack → real effect: persist spec, trigger diff,
> resolve approval) WITHOUT touching the schema or the config registration. The stub returning a
> non-error `result` (not an error) for known tools is deliberate: a cooperative agent calling
> `spectty_status` in M2 gets a clean success and keeps working; it simply has no UI-visible effect yet.

---

## 6. Tauri command + event surface (`src-tauri`)

### 6.1 New commands (`commands/session.rs`) — data-flow.md aligned
```rust
#[tauri::command]
pub async fn spawn_session(
    app: AppHandle,
    agent: AgentSpec,            // {kind, command?, tier}
    workspace_path: String,
    title: String,
    cols: u16, rows: u16,
    on_output: Channel<Vec<u8>>, // M1 raw render Channel (UNCHANGED)
    sessions: State<'_, SessionRegistry>,
    ptys: State<'_, PtyRegistry>,
    runners: State<'_, AgentRunnerRegistry>,     // resolves agent.kind → &dyn AgentRunner
    provisioner: State<'_, Arc<dyn ProvisioningPort>>,
    clock: State<'_, Arc<dyn ClockPort>>,
) -> Result<SessionId, String> {
    // 1. id = sessions.mint_id()
    // 2. runner = runners.resolve(&agent.kind)
    // 3. spec = runner.launch_spec(&LaunchContext { cwd: workspace_path, cols, rows, session_id: id, user_command: agent.command })
    // 4. if runner.descriptor().capabilities.requires_provisioning:
    //       scope = resolve_scope(...); handle = provisioner.inject(scope)?   (store handle in Session)
    // 5. (adapter, reader) = PtyAdapter::spawn(&PtySpawnConfig::from(spec))
    // 6. sessions.insert(Session { id, agent, status: Starting, title, created_at: clock.now(), ... })
    // 7. start_session_runtime(...) → render forwarder (M1) + OutputSignal signal thread (new) wired to runner+registry+app
    // 8. ptys.insert(id, PtyState { transport, stop, reader_thread })
    // 9. app.emit("session_created", SessionSummary)
    // 10. Ok(id)
}

#[tauri::command]
pub async fn close_session(id: SessionId, /* states */) -> Result<(), String> {
    // 1. ptys: kill_impl (M1 path — shutdown read+forwarder+signal threads, child kill)
    // 2. if session had a ProvisioningHandle → provisioner.retract(&handle)  (best-effort, logged)
    // 3. sessions.remove(id)
    // 4. app.emit("session_closed", id)
}

#[tauri::command]
pub fn list_sessions(sessions: State<'_, SessionRegistry>) -> Result<Vec<SessionSummary>, String> { Ok(sessions.summaries()) }

#[tauri::command]
pub fn get_session(id: SessionId, sessions: State<'_, SessionRegistry>) -> Result<Option<SessionSummary>, String> { /* ... */ }
```
> `send_input` / `pty_resize` from M1 stay as-is (they operate on `PtyRegistry` by id, and the id is now
> the SessionId). `pty_kill` is superseded by `close_session` for agent sessions but kept for raw PTYs.

### 6.2 The status pipeline (`session_runtime.rs`) — the pure `detect_step`
```rust
/// PURE decision for one signal tick (mirrors M1's `forward_step` testability discipline):
/// given the runner's observation and the registry's current status, decide whether to emit.
/// Returns Some(new_status) when the status changed (→ emit status_changed), else None.
/// In production this is `sessions.apply_observed(id, observed)` (which calls Core `transition`
/// under the registry lock); the free fn shape makes the detect→transition→emit wiring unit-testable.
fn observe_and_diff(runner: &dyn AgentRunner, sessions: &SessionRegistry, id: &SessionId, signal: &OutputSignal) -> Option<AgentStatus> {
    let observed = runner.detect_status(signal)?;       // None → no observation → no event
    sessions.apply_observed(id, observed)               // None → legal no-op → no event
}
```
The signal thread loops on its bounded `mpsc`: on each slice `producer.ingest(slice)`, stamp time via
`clock.now()`, build `signal = producer.snapshot(...)`, call `observe_and_diff(...)`, and on `Some(new)`
`app.emit("status_changed", StatusChanged { session_id, status: new, quick_actions })`. On EOF it builds
a final signal with `exit_code` set so the terminal status (`Completed`/`Error`) is emitted.

### 6.3 Events + payload
```rust
#[derive(Clone, serde::Serialize)]
pub struct StatusChanged {
    pub session_id: SessionId,
    pub status: AgentStatus,
    pub quick_actions: Vec<QuickAction>, // empty in M2 except future AwaitingInput
}
// also: session_created (SessionSummary), session_closed (SessionId). pty_exit kept from M1.
```

### 6.4 `lib.rs` registration
```rust
.manage(PtyRegistry::default())
.manage(SessionRegistry::default())
.manage(AgentRunnerRegistry::with_builtin())        // claude-code + generic
.manage::<Arc<dyn ProvisioningPort>>(Arc::new(ClaudeJsonProvisioner::new(RealConfigFile, ...)))
.manage::<Arc<dyn ClockPort>>(Arc::new(SystemClock::new()))
.invoke_handler(tauri::generate_handler![
    commands::ping::ping,
    commands::pty::pty_spawn, commands::pty::send_input, commands::pty::pty_resize, commands::pty::pty_kill,
    commands::session::spawn_session, commands::session::close_session,
    commands::session::list_sessions, commands::session::get_session,
])
```
> **Tauri-skill gate**: every new command is registered in `generate_handler!` (else silent IPC
> failure); `spawn_session`/`close_session` are `async` with OWNED arg types only (no `&str`); errors
> map to `String` (M0/M1 convention); `State<Arc<dyn Trait>>` uses the EXACT managed type (no mismatch
> panic). No new capability expected.

### 6.5 UI (high level)
- **`SpawnDialog.tsx`**: agent radio (Claude Code | Generic + free-text command), a cwd picker
  (`@tauri-apps/plugin-dialog` open-directory OR a text field — text field avoids a plugin/permission
  in M2), title field → `spawnSession(...)`.
- **`PaneHeader.tsx`**: renders `title` + an `AgentStatus` badge (color/label per status: Starting=grey,
  Idle=blue, Running=green-pulse, AwaitingInput=amber-pulse, Completed=grey-check, Error=red). Subscribes
  to `status_changed` filtered by `session_id`. The UI NEVER computes status locally (data-flow.md
  principle 1 — backend authoritative).
- **`useSession.ts`**: owns spawn/close + the `status_changed`/`session_created`/`session_closed`
  listeners (mirrors `useTerminal`). Single vitest target.

---

## 7. ADR-style decisions (D7–D20, continuing M1's D-series)

- **D7 — `ProvisioningPort` is a SEPARATE Core port, NOT an `AgentRunner` method (R9).** Resolves the
  ADR-0004 ⟷ domain-model.md disagreement toward domain-model.md. *Rationale*: provisioning is a
  session-lifecycle concern (inject-on-create / retract-on-close) with a different lifetime than
  per-output `detect_status`; `AgentRunner::provisioner() -> Option<Box<dyn>>` would force Generic
  (needs no injection) to carry the seam, and would couple two unrelated cadences in one trait.
  *Capability flag* `requires_provisioning: bool` on `AgentDescriptor` lets the composition root skip
  injection for Generic WITHOUT a trait method. *Rejected*: the ADR-0004 `provisioner()` method shape.
  **ADR-0004 text amendment**: YES — add a short "Superseded for M2+" note to ADR-0004 and
  agent-abstraction.md pointing at the separate `ProvisioningPort` (the separation is sound; the ADR's
  intent — agent-agnostic Core — is fully preserved, only the *mechanism* moved from a trait method to
  a sibling port). A tasks-level doc step appends that note; the code is the source of truth.

- **D8 — `detect_status` returns `Option<Observed>`; the pure Core `transition` owns the state
  machine.** *Rationale*: keeps the legal-transition policy in ONE tested place; a runner can never
  emit an illegal jump; `None` = "no observation". *Rejected*: `detect_status -> Option<AgentStatus>`
  (duplicates the transition policy across every runner and lets a scrape illegally jump to terminal).

- **D9 — OutputSignal producer runs on a THIRD thread fed by a BOUNDED, drop-oldest `sync_channel`
  tee'd off the M1 read thread (R6).** *Rationale*: render path keeps its unbounded M1 behavior and is
  NEVER throttled; the signal path uses `try_send` + drop so it can never back-pressure rendering;
  status detection only needs the latest window (pty-layer.md). *Rejected*: a single shared consumer
  (couples render cadence to detection); an unbounded signal channel (a slow detector would grow memory
  unboundedly).

- **D10 — Time is a Core `ClockPort` yielding a serde `Timestamp(u64 millis)`; `OutputSignal` carries a
  precomputed `idle_ms` (R2).** *Rationale*: `Instant` is neither serde nor cross-boundary comparable;
  precomputing `idle_ms` at the producer keeps `detect_status` a PURE function of the signal (no clock
  access inside a Core port impl) and makes idle-timeout a table test. *Rejected*: passing `Instant`
  (not serde, leaks `std::time` semantics across the port); reading the clock inside `detect_status`
  (makes the Core port impl impure/untestable).

- **D11 — Scrape with hand-rolled substring/line scanners, not `regex`, where feasible.** *Rationale*:
  keeps a heavy dep out of `crates/adapters` and the patterns trivially data-driven + table-tested; the
  M2 patterns are literal substrings. *Rejected*: pulling `regex` now (revisit in M3 if a pattern needs
  it). Patterns live as `&'static [&'static str]` DATA in `ClaudeCodeRunner` (R5), never in Core.

- **D12 — `AgentKind` is a serde `String` newtype, not a closed Core enum.** *Rationale*: adding an
  agent must never edit Core (ADR-0004); the `AgentRunnerRegistry` (adapters) maps the string to a
  runner. *Rejected*: `enum AgentKind { ClaudeCode, Generic }` (every new agent edits a Core enum —
  the exact coupling ADR-0004 forbids).

- **D13 — `SessionId == PtyId`; `SessionRegistry::mint_id` is the sole minter; `next_pty_id` retired
  (Lock 6).** *Rationale*: one id space, two registries in lockstep, no cross-map table; the aggregate
  root owns identity (domain-model.md). *Rejected*: separate id spaces with a mapping table (needless
  bookkeeping + drift risk).

- **D14 — DEFER Provisioner startup reconciliation; ship `.spectty.bak` + idempotent `retract` (R8).**
  *Rationale*: a leaked key points at a real stub binary (harmless, not corrupting); next clean close
  retracts it; a boot sweep needs the M3 persistence-backed Session restore. *Rejected*: building a
  boot-time orphan sweep in M2 (premature; needs infra M3 owns). **Flagged for verify.**

- **D15 — Stub `spectty-mcp` returns a non-error ack for known `tools/call`; schema + registration are
  the frozen forward-compat contract (R4).** *Rationale*: M3 swaps the call BODY (ack → effect) without
  touching the schema or config; a clean ack keeps a cooperative agent working in M2. *Rejected*:
  returning an error for known tools (would make a cooperative agent treat the tool as broken).

- **D16 — `spectty-mcp` is a standalone binary crate depending on serde only, NOT on `spectty-core`.**
  *Rationale*: it is a child process the agent launches, not part of the Tauri/Core graph; pulling Core
  into it would blur the quarantine and bloat the child binary. *Rejected*: a `spectty-mcp` subcommand
  inside `src-tauri` (couples the MCP server lifetime to the desktop app; the agent needs a plain
  executable path in its config).

- **D17 — JSON managed-NAMESPACE editor (own only `spectty_*` keys), pure String→String, behind an
  atomic-write `ConfigFile` seam (Lock 5, R7).** *Rationale*: `~/.claude.json` is one nested JSON doc;
  text markers corrupt it; `claude mcp add` is non-atomic/untestable. Owning only the `spectty` sub-key
  guarantees foreign keys (user + gentle-ai) round-trip untouched (the headline tested property).
  *Rejected*: text managed-markers (corrupt JSON); `claude mcp add` subprocess (not atomic, not unit-
  testable).

- **D18 — Scope resolution via an injected `is_git_tracked(path) -> bool` predicate; default Global.**
  *Rationale*: a full `GitPort` is M4; the predicate makes scope a pure table-tested function; the real
  probe is a minimal `git ls-files --error-unmatch`. *Rejected*: a real `GitPort` now (M4 scope creep);
  always-Global (misses the committed-config → Project case the roadmap requires).

- **D19 — `SessionRegistry` uses `&self` interior mutability (Mutex) like `PersistencePort`; status
  transition happens INSIDE the registry under its own lock (`apply_observed`).** *Rationale*: shares as
  one `tauri::State` across command handlers + the signal thread; doing the `transition` diff under the
  lock avoids a check-then-act race between a signal tick and a concurrent `close_session`. *Rejected*:
  `&mut self` registry (cannot share as State); transition outside the lock (TOCTOU race).

- **D20 — `LaunchSpec.env` is a sorted `Vec<(String,String)>`, not a `HashMap`.** *Rationale*:
  deterministic ordering for one-assertion `launch_spec` tests; mirrors the Core's String-path
  convention; `PtySpawnConfig` (M1) takes plain Strings already. *Rejected*: `HashMap`
  (non-deterministic iteration breaks exact-equality tests).

---

## 8. Strict-TDD Plan (RED first)

Test runner: **`cargo test --workspace`** (Rust) and **`pnpm -C ui test`** (vitest). RED first: write
the failing test, then the minimal impl. One behavior per test, descriptive names (M1 convention).

### 8.1 PURE units (NO fakes — the primary TDD surface)
**Core (`crates/core`):**
1. `transition` — the full legal-transition TABLE (every `(current, observed)` cell), absorbing
   terminals, no-op cells return `current`. (`agent_status.rs`)
2. `Session`/`SessionSummary`/`AgentSpec` serde round-trips (serde-safe across the IPC boundary).
3. `SessionRegistry::mint_id` monotonic + unique; `apply_observed` returns `Some` only on change,
   `None` on legal no-op + on terminal-absorbing.

**Adapters (`crates/adapters`):**
4. `GenericRunner::detect_status` — table: `(idle_ms, is_active, exit_code) -> Observed`
   (idle-timeout → Finished; active → Working; exit 0 → Finished; exit≠0 → Failed). INJECTED time via
   `OutputSignal.idle_ms`.
5. `ClaudeCodeRunner::detect_status` — each pattern in `awaiting_input`/`ready` → asserted `Observed`;
   no-match + inactive → `None` (R5 pattern list as data).
6. `launch_spec` mapping — Generic (user cmd / default shell) and ClaudeCode (`claude`, env carries
   `SPECTTY_SESSION_ID`, cwd) → exact `LaunchSpec` (D20 sorted env makes equality clean).
7. `inject_spectty_mcp` / `retract_spectty_mcp` ROUND-TRIPS — inject-then-retract returns a file with
   foreign (user + gentle-ai) keys byte-identical (R7 headline property); inject is idempotent (double
   inject == single); retract on a clean doc is a no-op.
8. `resolve_scope` — table with a fake `is_git_tracked`: tracked → `Project(root)`; untracked/absent →
   `Global` (D18).
9. `OutputSignalProducer::ingest` — ANSI CSI/OSC/ESC sequences stripped; printable text accumulated;
   window truncated from the front at `window_bytes` (pure, deterministic).

**UI (vitest):**
10. `useSession` — `spawn_session` invoked on submit; `status_changed` updates the badge; `close_session`
    on unmount; mock `invoke` + `listen`.

### 8.2 Units that NEED fakes / harness (the seams)
- **File-IO seam** (`ConfigFile`): `ClaudeJsonProvisioner` tested against an in-memory `FakeConfigFile`
  (HashMap) — asserts `inject` writes the right path per scope + backs up on first write; `retract`
  removes; absent file = `Ok(())`. NO real FS.
- **OutputSignal producer thread harness** (`session_runtime.rs`): the pure `observe_and_diff` free fn
  tested with a fake `AgentRunner` + a real `SessionRegistry` — feeds observations, asserts emitted
  status deltas (mirrors M1's `forward_step` test discipline). The bounded-channel drop-oldest behavior
  is tested by filling the channel and asserting the read side never blocks.
- **`SessionRegistry` command wiring** (`commands/session.rs`): `spawn_session_impl`/`close_session_impl`
  free fns tested against a `FakePtyTransport` (M1) + a fake `ProvisioningPort` (records inject/retract)
  + a fake `AgentRunnerRegistry` — asserts: id minted, provisioner.inject called ONLY when
  `requires_provisioning`, retract called on close, registry insert/remove, no real PTY opened.

### 8.3 Integration / real-PTY (`#[cfg(unix)]`, CI-safe)
- **Real-PTY agent spawn**: reuse M1's `#[cfg(unix)]` real-PTY template — spawn a deterministic Generic
  command (`/bin/sh -c "printf ...; sleep"`), drive the real read thread → OutputSignal producer →
  `detect_status`, assert the status reaches `Running` then `Completed` on exit (Generic baseline,
  exit-criterion 5 in miniature without wall-clock idle).
- **Stub `spectty-mcp` handshake**: spawn the built `spectty-mcp` binary, send `initialize` + `tools/list`
  over stdio, assert the 5 tool names come back and an unknown `tools/call` returns `-32601` (R4 contract).

### 8.4 Manual acceptance (the roadmap exit-criteria gate — `sdd-verify` pass/fail)
- [ ] (1) Spawn Claude Code on a git repo → reaches `Idle` (badge blue).
- [ ] (2) Inspect `~/.claude.json` (or `.mcp.json`) → `spectty` managed MCP entry present + inspectable.
- [ ] (3) Give a task → `Running`; hit a permission prompt → `AwaitingInput`; give input → `Running`
      (R5 empirical patterns validated against the real session; refine pattern data + add unit test).
- [ ] (4) Close → PTY dies + the `spectty` managed key is removed from the config.
- [ ] (5) Generic `bash` → `Idle` → idle-timeout → `Completed` (configurable timeout).
- [ ] cargo-deny core-scope stays green (`crates/core` gained nothing).

---

## 9. Risks / open questions (carried forward)
- **R2 — RESOLVED (D10)**: `OutputSignal` carries `last_byte_at: Timestamp` + precomputed `idle_ms: u64`;
  clock injected at the producer; `detect_status` pure.
- **R4 — RESOLVED (D15)**: stub ack contract pinned; schema + registration frozen for M3 swap.
- **R5 — captured (D11)**: patterns as `&'static [&'static str]` data in `ClaudeCodeRunner`; exit-crit 3
  is the manual validation that refines them.
- **R6 — RESOLVED (D9)**: bounded drop-oldest `sync_channel` tee'd off the read thread; render path never
  throttled.
- **R7 — RESOLVED (D17)**: foreign-key round-trip is the headline tested property of the JSON editor.
- **R8 — DEFERRED with rationale (D14)**: no boot sweep in M2; `.spectty.bak` + idempotent retract;
  injected scope recorded for a future M3 sweep. **Open**: M3 must add the persistence-backed orphan
  reconciliation.
- **R9 — RESOLVED (D7)**: ADR-0004 `provisioner()` method superseded-for-M2 by a separate
  `ProvisioningPort`; a tasks step appends a "Superseded for M2+" note to ADR-0004 + agent-abstraction.md.
- **Open (new)**: exact `~/.claude.json` MCP `mcpServers` shape vs the per-project `.mcp.json` root shape
  — confirm GLOBAL writes top-level `mcpServers` and PROJECT writes the `.mcp.json` root object during
  apply against a real Claude Code install (the explore VERIFIED both, but the pretty-print fixture for
  the round-trip test must match Claude Code's tolerated formatting).
- **Open (new)**: MCP stdio framing (line-delimited vs Content-Length) Claude Code expects from its
  stdio servers — pin during the `spectty-mcp` handshake apply step against a real `claude` launch.
```
