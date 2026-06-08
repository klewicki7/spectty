# Conventions

Code style, naming, tooling, and commit rules for the Spectty codebase. These are not
suggestions — they are the defaults enforced by CI.

---

## Rust naming

| Thing | Convention | Example |
|---|---|---|
| Modules | `snake_case` | `agent_runner`, `diff_explainer` |
| Types (structs, enums, traits) | `PascalCase` | `Session`, `AgentStatus`, `GitPort` |
| Functions and methods | `snake_case` | `spawn_session`, `transition_to` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_SESSIONS` |
| Lifetimes | single letter, lowercase | `'a`, `'sess` |
| Feature flags | `kebab-case` | `mock-adapters`, `e2e` |

**Domain terms must match the [Glossary](../glossary.md) exactly.** If the Glossary says
`AgentStatus`, the Rust type is `AgentStatus` — not `AgentState`, not `SessionStatus`.

---

## TypeScript naming

| Thing | Convention | Example |
|---|---|---|
| Variables and functions | `camelCase` | `sendInput`, `currentSession` |
| React components | `PascalCase` | `VibeLensPanel`, `SessionSidebar` |
| TypeScript interfaces/types | `PascalCase` | `Session`, `DiffExplanation` |
| CSS modules / class names | `kebab-case` | `session-sidebar`, `status-dot` |
| Tauri command names | `snake_case` (matches Rust) | `spawn_session` |
| Tauri event names | `snake_case` | `session_update`, `pty_data` |

Types in `ui/src/types/` mirror Rust domain structs. Keep them in sync — a mismatch
is a runtime bug. When the Rust type changes, update the TS type in the same PR.

---

## Module organization

### Rust

Follow the domain boundary strictly (see [Project Structure](project-structure.md)):

- `crates/core` — entities, ports, state machines, use cases. Zero I/O.
- `crates/adapters` — one sub-module per adapter. Each adapter only implements ports.
- `src-tauri` — commands and events. No business logic.

Inside each crate, prefer flat module files over deep nesting. If a module grows past
~300 lines, split it by concern, not by type.

### TypeScript / React

Use **atomic design** as a rough guide:
- `components/atoms/` — stateless, no Tauri interaction (Button, StatusDot, FileBadge)
- `components/molecules/` — composed atoms (FileChangeRow, SessionListItem)
- `components/organisms/` — full panel sections (SessionSidebar, VibeLensPanel, Dashboard)
- `hooks/` — all Tauri `invoke` / `listen` calls live here, not in components
- `store/` — client-side state only; no business rules

Components are **dumb presenters**. They receive props and call callbacks. Tauri
communication happens exclusively in hooks.

---

## Error handling

### Rust (recommended pattern)

- **Libraries (`crates/core`, `crates/adapters`):** define typed errors with
  [`thiserror`](https://docs.rs/thiserror). One `Error` enum per module with
  `#[error("...")]` annotations. No `unwrap()` in library code.
- **Edges (`src-tauri` commands):** convert to `anyhow::Error` or a serializable
  error type for the Tauri bridge. `anyhow` is fine at the edge; it is not fine in
  library code.
- **Panics:** only acceptable for truly unrecoverable programmer errors (violated
  invariants). Prefer `expect("reason")` over bare `unwrap()`.

### TypeScript

- Use `Result`-style discriminated unions for operations that can fail
  (`{ ok: true, data } | { ok: false, error }`).
- Never silently swallow errors. Every `.catch()` must log or surface to the user.
- Tauri command errors propagate as rejected promises; always handle them.

---

## Logging

Use [`tracing`](https://docs.rs/tracing) throughout the Rust backend — not `println!`
or `eprintln!`. Follow these levels:

| Level | When |
|---|---|
| `ERROR` | unrecoverable failure requiring human attention |
| `WARN` | recoverable anomaly (retry succeeded, degraded mode) |
| `INFO` | significant lifecycle events (session spawned, agent transitioned to AwaitingInput) |
| `DEBUG` | developer-useful state (PTY bytes received, diff pipeline triggered) |
| `TRACE` | high-frequency noise (individual PTY byte sequences) |

Instrument async tasks with `#[tracing::instrument]`. In the UI, use the browser console
with the same severity discipline — no `console.log` left in production paths.

---

## Formatting and linting

### Rust

```sh
# format
cargo fmt --all

# lint (CI enforces --deny warnings)
cargo clippy --workspace --all-targets -- -D warnings
```

`rustfmt.toml` and `clippy.toml` at repo root set project-wide rules. Do not override
them per-crate.

### TypeScript / React

```sh
pnpm lint       # eslint
pnpm format     # prettier
```

ESLint config enforces React 18+ rules, import ordering, and no `any`. Prettier handles
formatting — do not fight it, configure it.

---

## Git commits

Spectty uses **Conventional Commits** (`type(scope): description`).

Common types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`, `build`.

Common scopes: `core`, `adapters`, `tauri`, `ui`, `pty`, `git`, `vibelens`, `session`.

Good examples:

```
feat(core): add AwaitingInput → Idle transition on input given
fix(adapters/pty): flush pending bytes before PTY resize
test(core): add AgentStatus state machine unit tests
docs(engineering): add getting-started guide
```

### Mandatory rules

1. **No `Co-Authored-By` lines.** Not for AI tools, not for pair programming automation.
   If an AI tool auto-adds it, strip it before committing.
2. **No `Generated by` or `AI-assisted` in commit messages.** The commit history
   records decisions, not tools used.
3. Subject line: imperative mood, ≤72 chars, no trailing period.
4. Body (when needed): wrap at 72 chars, explain *why* not *what*.

---

## Branch naming

```
<type>/<short-description>
```

Examples: `feat/vibelens-panel`, `fix/pty-resize-flush`, `refactor/agent-runner-port`.

- Use the same types as Conventional Commits.
- Keep descriptions short and hyphenated.
- Delete branches after merging.

---

## Domain term discipline

Every code identifier, comment, and UI label that refers to a domain concept **must use
the canonical term from the [Glossary](../glossary.md)**. If you are about to write
`AgentState` or `SessionStatus`, stop and check the Glossary. The correct term is
`AgentStatus`. This is not pedantry — divergent naming is how codebases accrue
confusion over time.

When you introduce a new domain concept, add it to the Glossary first, then use it
in code. Never coin terms ad hoc in code comments.
