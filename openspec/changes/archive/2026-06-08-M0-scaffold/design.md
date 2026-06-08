# Design: M0 — Scaffold + Engram Wiring

> Status: design (architecture blueprint for `sdd-tasks` to slice)
> Scope: M0 only. PTY, AgentRunner, real engram HTTP, git, notifications, xterm.js
> are explicitly DEFERRED and referenced as such throughout.

This is the buildable architecture for M0. It turns the [proposal](./proposal.md)
into a concrete tree, dependency graph, module shapes, and test plan. It is grounded in
the locked decision docs: [project-structure](../../../docs/engineering/project-structure.md),
[stack-decisions](../../../docs/architecture/stack-decisions.md),
[ADR-0003 hexagonal](../../../docs/decisions/0003-hexagonal-architecture.md),
[domain-model](../../../docs/architecture/domain-model.md),
[data-flow](../../../docs/architecture/data-flow.md),
[conventions](../../../docs/engineering/conventions.md),
[testing-strategy](../../../docs/engineering/testing-strategy.md).

---

## 1. Architecture approach

**Pattern:** Hexagonal (Ports & Adapters), enforced from commit #1. The defining move of
M0 is that the boundary is real before any feature exists. We prove the skeleton holds by
wiring a single trivial flow (`ping → pong`) and a single port (`PersistencePort`) with a
real in-memory adapter and a deferred engram skeleton.

**Two enforcement mechanisms, layered:**

1. **PRIMARY — the Cargo dependency graph.** `spectty-core` lists ZERO outward deps
   (only `serde` + `thiserror`). Rust cannot resolve `use tauri::…` or `use spectty_adapters::…`
   inside `core` because those crates are not in `core`'s `[dependencies]`. The compiler
   rejects a boundary violation as an unresolved-import error. This is structural, not advisory.
2. **BACKSTOP — `cargo-deny`.** A `[bans]` config in `deny.toml` denies `tauri`, `tokio`,
   `reqwest` (and engram client crates when they exist) from appearing anywhere in
   `spectty-core`'s dependency closure. CI fails if someone tries to add a forbidden dep to
   `core`'s `Cargo.toml`. Belt-and-suspenders for the primary gate.

**Layer split (sync core / async adapters):** `spectty-core` is pure synchronous Rust —
no `tokio`, no `async fn` in the port trait for M0. Adapters are where async lives
(real engram HTTP in M3 will be async). For M0 the `PersistencePort` is **sync** so the
in-memory round-trip test needs no `#[tokio::test]`. See §9 for why this is deferred-safe.

---

## 2. Directory / workspace layout

Exact tree for M0. Paths match [project-structure.md](../../../docs/engineering/project-structure.md)
(crate package names are `spectty-core` / `spectty-adapters` per the proposal's locked decision).
Folders the structure doc lists for later milestones (`pty/`, `git/`, `state/`, `use_cases/`,
`components/Terminal/`, …) are intentionally NOT created in M0.

```
ai-terminal/                          # repo root (existing)
├── Cargo.toml                        # [workspace] — members: crates/core, crates/adapters, src-tauri
├── Cargo.lock                        # committed (binary workspace)
├── rust-toolchain.toml               # channel = "1.89.0"  (pins Rust)
├── rustfmt.toml                      # workspace fmt rules (referenced by conventions)
├── clippy.toml                       # workspace clippy rules
├── deny.toml                         # cargo-deny: [bans] boundary gate for spectty-core
├── package.json                      # pnpm workspace root (private, workspaces: ["ui"])
├── pnpm-workspace.yaml               # packages: ["ui"]
├── .gitignore                        # target/, node_modules/, dist/, .DS_Store
│
├── crates/
│   ├── core/                         # pkg: spectty-core
│   │   ├── Cargo.toml                # deps: serde (derive), thiserror.  NOTHING else.
│   │   └── src/
│   │       ├── lib.rs                # pub mod entities; pub mod ports; re-exports
│   │       ├── entities/
│   │       │   ├── mod.rs            # pub use session::*, workspace::*, agent_status::*
│   │       │   ├── session.rs        # Session, SessionId (placeholder)
│   │       │   ├── workspace.rs      # Workspace, WorkspaceId (placeholder)
│   │       │   └── agent_status.rs   # AgentStatus enum (variants only, no transitions)
│   │       └── ports/
│   │           ├── mod.rs            # pub use persistence::*
│   │           └── persistence.rs    # PersistencePort trait + PersistenceError (thiserror)
│   │
│   └── adapters/                     # pkg: spectty-adapters
│       ├── Cargo.toml                # deps: spectty-core, serde_json, anyhow, thiserror
│       └── src/
│           ├── lib.rs                # pub mod persistence;
│           └── persistence/
│               ├── mod.rs            # pub use in_memory::*, engram::*
│               ├── in_memory.rs      # InMemoryPersistenceAdapter (HashMap) + round-trip test
│               └── engram.rs         # EngramAdapter skeleton — impl with todo!() (real HTTP = M3)
│
├── src-tauri/                        # Tauri v2 shell — the Bridge
│   ├── Cargo.toml                    # deps: spectty-core, spectty-adapters, tauri v2, serde, tokio
│   ├── tauri.conf.json              # v2 config; frontendDist points at ../ui/dist; devUrl :1420
│   ├── build.rs                      # tauri_build::build()
│   └── src/
│       ├── main.rs                   # builder, .invoke_handler(ping), .run()
│       ├── lib.rs                    # (optional) run() entrypoint per Tauri v2 convention
│       └── commands/
│           ├── mod.rs               # pub use ping::*
│           └── ping.rs              # #[tauri::command] ping() → emits "pong" via AppHandle::emit
│
├── ui/                               # React 19 + Vite
│   ├── package.json                  # react 19, vite, @tauri-apps/api v2, vitest, @testing-library/react
│   ├── vite.config.ts                # port 1420, react plugin, vitest config block
│   ├── tsconfig.json
│   ├── index.html
│   ├── src/
│   │   ├── main.tsx                  # React root
│   │   ├── App.tsx                   # renders PingPong demo
│   │   └── hooks/
│   │       └── usePingPong.ts        # invoke("ping") + listen("pong") (Tauri calls live in hooks)
│   └── tests/
│       └── unit/
│           └── usePingPong.test.ts   # Vitest: mocks @tauri-apps/api/core + /event
│
└── .github/
    └── workflows/
        └── ci.yml                    # macOS runner: fmt, clippy, test, deny, pnpm test (+ sccache)
```

**Placement rules (what goes where):**
- Root `Cargo.toml` = workspace manifest only (`[workspace]` + `members` + `[workspace.dependencies]`
  for shared pins). It is NOT a crate itself.
- `src-tauri` is a **member crate** of the Cargo workspace but a **binary** (`main.rs`); it is
  the only crate allowed to depend on `tauri`.
- `ui/` is the ONLY pnpm workspace package in M0. Root `package.json` is private and just
  hosts the `tauri` dev/build scripts + the workspace declaration.
- Tooling configs (`rustfmt.toml`, `clippy.toml`, `deny.toml`) live at repo root so every
  crate inherits them — conventions doc forbids per-crate overrides.

---

## 3. Crate dependency graph (the quarantine)

### Allowed edges (and ONLY these)

```
            ┌────────────────────────────────────────────────┐
            │                   src-tauri                     │   (binary, the Bridge)
            │   deps: tauri v2, tokio, serde,                 │
            │         spectty-core, spectty-adapters          │
            └───────────────┬───────────────────┬────────────┘
                            │                   │
                 ┌──────────▼─────────┐         │
                 │  spectty-adapters  │         │
                 │  deps: spectty-core│         │
                 │        serde_json  │         │
                 │        anyhow      │         │
                 │        thiserror   │         │
                 └──────────┬─────────┘         │
                            │                   │
                            ▼                   ▼
                 ┌────────────────────────────────────┐
                 │            spectty-core             │   (pure domain)
                 │   deps: serde (derive), thiserror   │
                 │   NO tauri · NO tokio · NO reqwest  │
                 │   NO engram · NO adapters · NO std net/fs │
                 └────────────────────────────────────┘
```

- `spectty-core` → `serde`, `thiserror`. Full stop.
- `spectty-adapters` → `spectty-core` (to implement its ports) + `serde_json`, `anyhow`, `thiserror`.
- `src-tauri` → `spectty-core` + `spectty-adapters` + `tauri` v2 + `tokio` + `serde`.

### Forbidden edges (compiler-enforced)

| Forbidden | Why it cannot happen |
|---|---|
| `spectty-core` → `spectty-adapters` | not in core's `[dependencies]` → `use spectty_adapters::…` fails to resolve |
| `spectty-core` → `tauri` / `tokio` / `reqwest` | not declared → unresolved import; also denied in `deny.toml` |
| `spectty-core` → any engram client | not declared → unresolved import; engram is invisible to Core (ADR-0003, domain-model §Ports) |
| `spectty-adapters` → `tauri` | adapters must not know the bridge exists |
| `spectty-adapters` → `spectty-adapters` self-cycle / adapter↔adapter | each adapter module standalone (project-structure rule); enforced by review + module layout |
| anything → `src-tauri` | a binary crate; nothing depends on it |

### How Cargo physically prevents the violation

Rust crates can only name items from crates listed in their own `[dependencies]`. Because
`crates/core/Cargo.toml` declares only `serde` and `thiserror`, the symbols `tauri`,
`tokio`, `spectty_adapters`, and any engram client simply **do not exist** in `core`'s
namespace. A developer writing `use tauri::AppHandle;` in `core` gets a hard
`error[E0432]: unresolved import` at compile time — `cargo build` fails before any test runs.
There is no "discipline" to forget; the dependency is absent, so the code does not compile.

### `cargo-deny` backstop (`deny.toml`)

```toml
# deny.toml — boundary backstop (PRIMARY gate is the Cargo graph above)
[bans]
multiple-versions = "warn"

# Forbid these crates from spectty-core's dependency closure.
# If someone adds tauri/tokio/reqwest to crates/core/Cargo.toml, CI fails here
# in addition to (and as documentation of) the compiler rejection.
deny = [
  { name = "tauri" },
  { name = "tokio" },
  { name = "reqwest" },
  # engram client crate name added in M3 when it exists
]

# Scope the ban to the core crate. cargo-deny runs per-crate in CI:
#   cargo deny --manifest-path crates/core/Cargo.toml check bans
```

> Open question (§10): cargo-deny's `[bans].deny` is global by default. The clean per-crate
> scoping is to run `cargo deny check bans` with `--manifest-path crates/core/Cargo.toml`
> in CI (so the ban applies to core's closure only, while `src-tauri` is allowed `tauri`).
> Confirm exact invocation at apply time.

---

## 4. Core module design (`spectty-core`)

Pure sync Rust. No async, no tokio. Entities are **behaviorless placeholders** — M0 proves
the skeleton, not domain logic. The state machine, invariants, and use-cases (domain-model.md)
are DEFERRED to M2.

### `entities/agent_status.rs`

```rust
use serde::{Deserialize, Serialize};

/// Agent lifecycle state. M0: variants only — NO transition logic (deferred to M2).
/// Variants match the state machine in docs/architecture/domain-model.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Starting,
    Idle,
    Running,
    AwaitingInput,
    Completed,
    Error,
}
```

### `entities/workspace.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceId(pub String);

/// A git repository the user works in. M0 placeholder: identity + root only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub root: String, // PathBuf deferred — keep core free of fs-flavored types in M0 placeholder
}
```

### `entities/session.rs`

```rust
use serde::{Deserialize, Serialize};
use crate::entities::{AgentStatus, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId(pub String);

/// The aggregate root (domain-model.md). M0 placeholder: minimal fields, no behavior.
/// Worktree, Spec, CostMetrics, DiffExplanation, Checkpoint are DEFERRED (M2+).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub workspace: WorkspaceId,
    pub status: AgentStatus,
    pub title: String,
}
```

### `ports/persistence.rs`

The port contract is the **only** behavior-bearing thing in M0 core. It mirrors the
data-flow.md persistence contract (`upsert(topic_key, payload)` / `get(topic_key)`) but
kept **sync** and **string-keyed + JSON-string-valued** for M0 to avoid pulling serde_json
into core (core stays serde-only; serialization of payloads happens in the adapter layer).

```rust
use thiserror::Error;

/// Typed error for persistence operations. thiserror in Core (conventions: libraries use thiserror).
/// A missing key is NOT an error — `get` returns `Ok(None)`. This enum is reserved for
/// genuine backend failures (network, serialization, IO, ...).
#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("persistence backend error: {0}")]
    Backend(String),
}

/// Store/retrieve serialized domain payloads keyed by topic_key.
/// M0 contract: SYNC, value is an already-serialized JSON string (adapter owns (de)serialization
/// of concrete entities). Typed payloads + async transport arrive with the real EngramAdapter (M3).
///
/// Both methods take `&self` so a single adapter can be shared across multiple concurrent
/// Sessions behind an `Arc<dyn PersistencePort>` without an exclusive mutable borrow. Any
/// mutability is encapsulated INSIDE the adapter (interior mutability). `Send + Sync` makes
/// that sharing safe across threads. Core defines this trait and depends on NOTHING outward.
pub trait PersistencePort: Send + Sync {
    /// Insert or replace the value at `topic_key`.
    fn upsert(&self, topic_key: &str, payload: String) -> Result<(), PersistenceError>;

    /// Retrieve the value at `topic_key`. Returns `Ok(None)` when the key is absent.
    fn get(&self, topic_key: &str) -> Result<Option<String>, PersistenceError>;
}
```

> **Corrected at M0 apply time.** The port was changed from `&mut self` / `Result<String, NotFound>`
> to `&self` / `Result<Option<String>, _>` so a single adapter is shareable across concurrent
> Sessions behind an `Arc<dyn PersistencePort>` (per stack-decisions: concurrent Sessions share
> adapters), and so a missing key maps to `Ok(None)` exactly as the spec's negative/guard scenario
> requires. The `NotFound` variant was removed (now unused); `Backend` remains for real failures.

Design notes:
- `&self` (not `&mut self`) makes the port shareable as `Arc<dyn PersistencePort>` across
  Sessions without an exclusive mutable borrow. Mutability is encapsulated INSIDE the adapter
  via interior mutability (the in-memory adapter uses a `Mutex<HashMap<..>>`).
- `get` returns `Ok(None)` for a missing key — a normal, expected outcome, not an error. This
  aligns with the spec's "read of missing key returns None/empty, no error" guard scenario.
- Payload is `String` (serialized JSON), not a typed generic, to keep `serde_json` OUT of core.
  The adapter serializes/deserializes concrete entities. This is the minimal contract that
  still proves a round-trip.
- `topic_key` naming aligns with data-flow.md (`spectty/sessions/{id}` etc.) so M3 slots in
  without renaming.

---

## 5. Adapters design (`spectty-adapters`)

### `persistence/in_memory.rs` — REAL adapter (used in tests)

```rust
use std::collections::HashMap;
use std::sync::Mutex;
use spectty_core::ports::{PersistencePort, PersistenceError};

/// Real, working PersistencePort backed by an in-process HashMap.
/// This is the M0 proof that the port round-trips. NOT a mock — it fully implements the contract.
/// The map lives behind a `Mutex` (interior mutability) so the adapter honors the `&self`
/// contract and stays `Send + Sync`, making it shareable as `Arc<dyn PersistencePort>`.
#[derive(Debug, Default)]
pub struct InMemoryPersistenceAdapter {
    store: Mutex<HashMap<String, String>>,
}

impl InMemoryPersistenceAdapter {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PersistencePort for InMemoryPersistenceAdapter {
    fn upsert(&self, topic_key: &str, payload: String) -> Result<(), PersistenceError> {
        self.store
            .lock()
            .expect("in-memory persistence mutex poisoned")
            .insert(topic_key.to_owned(), payload);
        Ok(())
    }

    fn get(&self, topic_key: &str) -> Result<Option<String>, PersistenceError> {
        Ok(self
            .store
            .lock()
            .expect("in-memory persistence mutex poisoned")
            .get(topic_key)
            .cloned())
    }
}
```

### `persistence/engram.rs` — SKELETON (real HTTP is M3)

```rust
use spectty_core::ports::{PersistencePort, PersistenceError};

/// Skeleton adapter for engram (HTTP :7437). M0 = signature only; bodies are todo!().
/// Real implementation (reqwest POST/GET to :7437, the 2s poll loop, subscribe) lands in M3.
/// See docs/architecture/data-flow.md "The event-stream gap and the polling layer".
#[derive(Debug, Default)]
pub struct EngramAdapter {
    // base_url: String,           // ":7437" — wired in M3
    // http: reqwest::Client,      // M3
}

impl EngramAdapter {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PersistencePort for EngramAdapter {
    fn upsert(&self, _topic_key: &str, _payload: String) -> Result<(), PersistenceError> {
        todo!("M3: POST to engram :7437 /api/observations")
    }

    fn get(&self, _topic_key: &str) -> Result<Option<String>, PersistenceError> {
        todo!("M3: GET engram :7437 /api/observations?topic_key=...")
    }
}
```

Why both? The proposal's "engram wired" = **port + stub + skeleton**, not a running daemon.
`InMemoryPersistenceAdapter` proves the contract works (round-trip test). `EngramAdapter`
proves the seam where the real backend plugs in, with `todo!()` documenting the M3 boundary.
`anyhow` is in `spectty-adapters` deps for adapter-edge error context (conventions: adapters
use anyhow), though M0's in-memory adapter only needs the typed `PersistenceError`.

---

## 6. Bridge design (`src-tauri` + `ui`)

### Rust side — `commands/ping.rs`

Tauri **v2**: events are emitted from `AppHandle` (`AppHandle::emit`), NOT v1's
`Window::emit`. The command receives `AppHandle` via Tauri's command injection.

```rust
use tauri::{AppHandle, Emitter}; // v2: Emitter trait brings `emit` into scope

/// Minimal Bridge proof: UI invokes "ping", backend emits "pong" event back.
/// Mirrors the command/event pattern in docs/architecture/data-flow.md (request → push).
#[tauri::command]
pub fn ping(app: AppHandle) -> Result<(), String> {
    // v2 API: AppHandle::emit (v1 used Window::emit — see proposal risk).
    app.emit("pong", "pong from spectty backend")
        .map_err(|e| e.to_string())?;
    Ok(())
}
```

`main.rs` registers it:

```rust
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::ping::ping])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

For M0 the command does NOT touch `PersistencePort` — the port round-trip is proven by the
Rust unit test (§7). The Bridge proves invoke/emit; the port proves persistence. Keeping them
independent keeps each proof minimal. (Wiring a real use-case through the bridge is M2.)

### React side — `hooks/usePingPong.ts`

Tauri calls live in hooks (conventions + project-structure rule). v2 import paths:
`invoke` from `@tauri-apps/api/core`, `listen` from `@tauri-apps/api/event`.

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

export function usePingPong() {
  const [pong, setPong] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen<string>("pong", (event) => {
      setPong(event.payload);
      console.log("[spectty] received pong:", event.payload);
    });
    return () => {
      unlisten.then((off) => off());
    };
  }, []);

  const sendPing = async () => {
    await invoke("ping");
  };

  return { pong, sendPing };
}
```

`App.tsx` renders a button calling `sendPing` and shows `pong` — satisfies the success
criterion "ping → pong visible in web console of running app."

---

## 7. Testing strategy for M0

Two tests only — one per side of the boundary. M0 proves wiring, not behavior.

### Rust — PersistencePort round-trip

- **Location:** inline `#[cfg(test)]` module in `crates/adapters/src/persistence/in_memory.rs`
  (the adapter is the thing under test; testing-strategy.md places adapter tests in the
  adapters crate).
- **Shape:**

```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use super::*;

    #[test]
    fn test_in_memory_persistence_round_trips() {
        let adapter = InMemoryPersistenceAdapter::new();
        let key = "spectty/sessions/s1";
        let payload = r#"{"id":"s1","status":"Idle"}"#.to_owned();

        adapter.upsert(key, payload.clone()).unwrap();
        let got = adapter.get(key).unwrap();

        assert_eq!(got, Some(payload));
    }

    #[test]
    fn test_get_missing_key_returns_none() {
        let adapter = InMemoryPersistenceAdapter::new();
        assert_eq!(adapter.get("nope").unwrap(), None);
    }

    #[test]
    fn test_usable_behind_arc_dyn_port() {
        // Proves the &self contract: shareable across Sessions behind Arc<dyn _>.
        let port: Arc<dyn PersistencePort> = Arc::new(InMemoryPersistenceAdapter::new());
        port.upsert("k", "v".to_owned()).unwrap();
        assert_eq!(port.get("k").unwrap(), Some("v".to_owned()));
    }
}
```

  No `#[tokio::test]` — the port is sync. This is the payoff of the sync-core decision.

### UI — Vitest mocking Tauri

- **Location:** `ui/tests/unit/usePingPong.test.ts`.
- **Shape:** mock `@tauri-apps/api/core` (so `invoke` is a spy) and `@tauri-apps/api/event`
  (so `listen` is controllable), render the hook with `@testing-library/react`'s
  `renderHook`, assert `sendPing()` calls `invoke("ping")` and that a fired `pong` event
  updates `pong`.

```ts
import { vi, describe, it, expect } from "vitest";
import { renderHook, act } from "@testing-library/react";

const invoke = vi.fn();
let pongHandler: ((e: { payload: string }) => void) | null = null;

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, cb: (e: { payload: string }) => void) => {
    pongHandler = cb;
    return Promise.resolve(() => {});
  },
}));

import { usePingPong } from "../../src/hooks/usePingPong";

describe("usePingPong", () => {
  it("invokes ping and stores pong payload", async () => {
    const { result } = renderHook(() => usePingPong());
    await act(async () => { await result.current.sendPing(); });
    expect(invoke).toHaveBeenCalledWith("ping");

    await act(async () => { pongHandler?.({ payload: "pong from spectty backend" }); });
    expect(result.current.pong).toBe("pong from spectty backend");
  });
});
```

### Exact test commands

| Layer | Command |
|---|---|
| Rust (all crates) | `cargo test --workspace` |
| UI | `pnpm --filter ui test` (or `pnpm test` if root script proxies to ui) — runs `vitest run` |

---

## 8. CI pipeline design

Single job, **macOS runner** (proposal risk: Linux runner breaks Tauri deps → pin
`macos-latest`). Steps in order:

```yaml
# .github/workflows/ci.yml (shape — exact pins resolved at apply)
name: ci
on: [push, pull_request]
jobs:
  build-and-test:
    runs-on: macos-latest
    env:
      SCCACHE_GHA_ENABLED: "true"
      RUSTC_WRAPPER: "sccache"
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.89.0
        with: { components: "rustfmt, clippy" }
      - uses: mozilla-actions/sccache-action@v0.0.x   # keeps build under 30-min onboarding bar
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with: { node-version: 23, cache: "pnpm" }
      - uses: EmbarkStudios/cargo-deny-action@v2       # or `cargo install cargo-deny`

      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo build --workspace
      - run: cargo test --workspace
      - run: cargo deny --manifest-path crates/core/Cargo.toml check bans   # boundary backstop
      - run: pnpm install --frozen-lockfile
      - run: pnpm --filter ui test
```

Step purposes:
- `cargo fmt --all --check` — formatting gate (conventions).
- `cargo clippy --workspace --all-targets -- -D warnings` — lint gate, warnings are errors.
- `cargo build --workspace` — compiles all crates; this is ALSO where a boundary violation
  in `core` would fail (primary gate).
- `cargo test --workspace` — runs the persistence round-trip test.
- `cargo deny … check bans` — the cargo-deny boundary backstop (scoped to core's manifest).
- `pnpm install` + `pnpm … test` — installs UI deps and runs Vitest.
- **sccache** wraps `rustc` to cache compilation across CI runs, mitigating the 5–15 min
  first-build risk against the <30-min onboarding goal.

---

## 9. Dev tooling

**Canonical run command (single entry):**

```sh
pnpm tauri dev
```

This is wired via the root `package.json` `tauri` script (`@tauri-apps/cli`). It:
1. Starts Vite dev server on `:1420` (HMR for the React UI).
2. Builds + runs `src-tauri` in dev mode, pointing the webview at `:1420`.
3. Rebuilds the Rust shell on change.

**Hot-reload split:**
- **UI (React/Vite):** Vite HMR — instant, no Rust rebuild. This is the fast inner loop.
- **Backend (Rust):** `pnpm tauri dev` rebuilds `src-tauri` on file change. For a tighter
  Rust-only loop, `cargo watch -x 'build --workspace'` in a second terminal gives fast
  feedback on `core`/`adapters` without launching the webview. `cargo-watch` is a documented
  optional dev dependency, NOT required to run the app.

Documented in a getting-started note so a new contributor's path is: clone → `cargo build`
→ `pnpm install` → `pnpm tauri dev` → see ping/pong. (<30 min success criterion.)

---

## 10. Key technical decisions & tradeoffs (ADR-style)

### D1 — Tauri v2 `AppHandle::emit` (not v1 `Window::emit`)
- **Decision:** Emit `pong` via `AppHandle::emit` with the `Emitter` trait in scope.
- **Why:** Tauri v2 moved event emission off `Window` onto the `Emitter` trait, reachable
  from `AppHandle`. v1 idioms (`window.emit`) will not compile.
- **Rejected:** Targeting v1 patterns — stack-decisions locks Tauri v2.
- **Risk/mitigation:** v2 API churn → verify the exact `Emitter` import + signature against
  current Tauri v2 docs at apply time (proposal risk, Med).

### D2 — Sync Core / Async Adapters
- **Decision:** `PersistencePort` and all M0 core code are synchronous. Async (tokio) lives
  only in `src-tauri` and future adapters.
- **Why:** stack-decisions explicitly says "the Hexagonal Core is pure synchronous Rust."
  Sync core means the round-trip test needs no async runtime and no `#[tokio::test]`.
- **Rejected:** Async port trait in M0 — would force `async-trait` or tokio into core's test
  path for zero M0 benefit.
- **Tradeoff:** The real EngramAdapter (M3) is async; the port signature will gain `async`
  (or move to an async-capable shape) then. Safe because M0 has no production caller bound
  to the sync signature — only the in-memory test adapter uses it.
- **Note (apply-time correction):** the port already takes `&self` (with interior mutability
  in the adapter) so it is shareable as `Arc<dyn PersistencePort>` across concurrent Sessions
  today. Only the async transition remains for M3 — the `&self` shape is final.

### D3 — In-memory stub satisfies M0's "engram requirement"
- **Decision:** `InMemoryPersistenceAdapter` (real HashMap impl) is the M0 persistence proof;
  `EngramAdapter` is a `todo!()` skeleton.
- **Why:** Proposal defines "engram wired" = port + stub + skeleton, NOT a running daemon.
  The contract (round-trip) is what M0 must prove; the transport (HTTP :7437) is M3.
- **Rejected:** Standing up engram HTTP in M0 — out of scope, adds a daemon dependency to
  onboarding, contradicts the <30-min goal.

### D4 — Error type strategy: thiserror in core, anyhow at the edge
- **Decision:** `PersistenceError` (thiserror) in `spectty-core`; `anyhow` available in
  `spectty-adapters` for edge context; Tauri commands return `Result<_, String>` (serializable).
- **Why:** conventions.md — libraries use thiserror (typed), edges use anyhow; Tauri bridge
  needs a serializable error so a plain `String` (or a serde error type) crosses to JS.
- **Rejected:** anyhow in core — forbidden by conventions for library crates.

### D5 — Payload as serialized `String`, not typed generic, in the M0 port
- **Decision:** `upsert(topic_key, payload: String)` where payload is JSON text.
- **Why:** keeps `serde_json` OUT of `spectty-core` (core stays serde-derive only). The
  adapter owns (de)serialization. Minimal contract that still proves a round-trip.
- **Tradeoff:** Less type-safe than `upsert<T: Serialize>`. Acceptable for M0; M3 can
  introduce typed methods on the adapter side without changing the core boundary rule.

### D6 — `spectty-core` deps frozen at serde + thiserror
- **Decision:** core declares exactly two deps.
- **Why:** the dependency list IS the primary boundary gate (§3). Every dep added to core is
  a potential boundary hole, so the list is deliberately minimal and reviewed.

---

## 11. Open questions to flag (NOT resolved in M0)

1. **Exact semver pins.** stack-decisions has an open item to lock all crate versions after
   the prototype validates integration. M0 design defers precise pins (serde, thiserror,
   tauri v2.x, vitest, etc.) to first successful compile — pin to whatever resolves green,
   record in `Cargo.lock` / `pnpm-lock.yaml`, tighten later.
2. **sccache in CI — keep or drop?** Included in the §8 design to protect the <30-min bar,
   but it adds CI config surface and a cache action dependency. Decision: enable for M0; if
   first-build time on `macos-latest` is already comfortably under budget without it,
   consider removing to simplify. Flag for the apply/verify phase to measure.
3. **cargo-deny scope invocation.** `[bans].deny` is global; the clean per-crate scoping is
   running `cargo deny --manifest-path crates/core/Cargo.toml check bans`. Confirm this is the
   right incantation (vs. a `[bans]` workspace-graph filter) at apply time.
4. **Root `pnpm test` proxy vs. `--filter ui`.** Decide whether the root `package.json`
   exposes a `test` script that proxies to the ui package, or CI calls `pnpm --filter ui test`
   directly. Cosmetic; resolve at apply.

---

## Deferred (explicitly out of M0 — do not design here)

PTY / xterm.js (M1) · AgentStatus state machine + transitions, AgentRunner, SessionRegistry,
use_cases (M2) · real engram HTTP + 2s poll loop + `subscribe` (M3) · GitPort/GitAdapter (M4) ·
NotifierPort (M5) · Playwright E2E + headless-webview integration (M3+). These are referenced
as seams (the `EngramAdapter` skeleton, the empty module slots) but carry NO implementation
in M0.
