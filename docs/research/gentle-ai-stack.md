# Gentle AI / engram Stack — Research Findings

> **Status**: Complete. Source of truth for Spectty's stack-reuse decisions.
> Raw engram observations: `research/gentle-ai-cli` (#757), `research/engram-internals` (#758),
> `research/sdd-agent-teams` (#756), `research/gentle-ai-web` (#759),
> `research/gentle-ai-synthesis` (#760), `research/gentle-ai-licenses` (#761).

---

## 1. Ecosystem Overview

### Author

**Alan Buscaglia** ("Alan-TheGentleman") — Argentine developer based in Barcelona.
App Lead at Prowler Cloud, Google Developer Expert (Angular), Microsoft MVP.
100K+ YouTube subscribers at [@GentlemanProgramming](https://www.youtube.com/@GentlemanProgramming).
Personal GitHub: [Alan-TheGentleman](https://github.com/Alan-TheGentleman).
Org: [Gentleman-Programming](https://github.com/Gentleman-Programming).
Free/open-source book: <https://the-amazing-gentleman-programming-book.vercel.app>.

### Philosophy

- **"We are Tony Stark. AI is Jarvis."** — human retains decision authority, AI executes.
- "NOT: AI programs for me. YES: I program WITH AI."
- Anti-vibe-coding: structured SDD (Spec-Driven Development) over intuition-led sessions.
- Concepts > Code: teaches architecturally correct AI usage, not just code generation.
- Stack is positioned as the antidote to vibe coding, built on: persistent memory + skills + SDD + TDD.

### Components

| Repo | Stars (June 2026) | Language | Role |
|---|---|---|---|
| [engram](https://github.com/Gentleman-Programming/engram) | 4.2k | Go | Persistent memory — SQLite + MCP + HTTP API |
| [gentle-ai](https://github.com/Gentleman-Programming/gentle-ai) | 3.7k | Go + Bun/JS | Ecosystem configurator CLI — provisions 15 agents |
| [agent-teams-lite](https://github.com/Gentleman-Programming/agent-teams-lite) *(archived)* | 1.2k | Markdown | SDD orchestrator + 9 sub-agents — absorbed into gentle-ai (March 2026) |
| [Gentleman-Skills](https://github.com/Gentleman-Programming/Gentleman-Skills) | 543 | Markdown | Community SKILL.md registry — Angular, React 19, Next.js 15, TypeScript, etc. |
| [gentleman-guardian-angel](https://github.com/Gentleman-Programming/gentleman-guardian-angel) | 1k | Bash | Pre-commit AI code review; zero dependencies; supports 6+ providers |
| [Gentleman-MCP](https://github.com/Gentleman-Programming/Gentleman-MCP) | — | Go | Universal MCP gateway (TLS 1.3, JWT, NATS) |
| [gentle-pi](https://github.com/Gentleman-Programming/gentle-pi) | 114 | — | Pi-specific package: gentle-engram + pi-subagents + pi-intercom |
| [from-chat-to-cognitive-system](https://github.com/Gentleman-Programming/from-chat-to-cognitive-system) | 87 | HTML | 40-slide presentation: "De Chat a Sistema Cognitivo" |

### Distribution

```
brew tap Gentleman-Programming/homebrew-tap
brew install gentle-ai
```

Also available via Scoop (Windows) and `go install`. engram is listed in the Claude Code Marketplace as a plugin. Pi packages available via `pi.dev/packages/gentle-engram`.

**15 supported agents**: claude-code, opencode, kilocode, gemini-cli, cursor, vscode-copilot, codex, windsurf, antigravity, kimi, qwen-code, kiro-ide, openclaw, trae, pi.

---

## 2. gentle-ai CLI & Provisioning Patterns

### What gentle-ai Is

gentle-ai v1.30.8+ is a **Go binary with embedded Bun/JS runtime** (~9.4 MB, arm64). The Go layer handles CLI scaffolding, file I/O, and the Bubbletea TUI (setup wizard, Rose Pine theme). The JS/Bun layer handles the plugin/delegation runtime (OpenCode plugin model-variants, background-agents).

Subcommands: `install`, `uninstall`, `sync`, `skill-registry refresh`, `update`, `upgrade`, `restore`, `version`, `doctor`. Plus an interactive TUI setup wizard.

Binary location: `/opt/homebrew/Cellar/gentle-ai/1.30.8/bin/gentle-ai`.

### Per-Agent Native-Format Provisioning

gentle-ai writes each agent's files in that agent's native format:

| Agent | Files written |
|---|---|
| Claude Code | `CLAUDE.md` + `~/.claude/settings.json` hooks + `~/.claude/skills/{phase}/SKILL.md` |
| Cursor | `~/.cursor/agents/` subagent YAML files + `.cursorrules` |
| Gemini CLI | `~/.gemini/GEMINI.md` |
| Kiro IDE | `~/.kiro/skills/` + `~/.kiro/agents/` |
| Codex | `~/.codex/skills/` |
| Windsurf | `~/.codeium/windsurf/skills/` + `.windsurf/workflows/` |
| OpenCode | `opencode.json` agents overlay with per-agent model routing |

For SDD, phase-specific sub-agent definitions (`sdd-apply`, `sdd-archive`, `sdd-design`, `sdd-explore`, `sdd-init`, `sdd-onboard`, `sdd-propose`, `sdd-spec`, `sdd-tasks`, `sdd-verify`) are materialized in each agent's native sub-agent format.

### Patterns Spectty Copies

1. **Managed-section markers** — `<!-- gentle-ai:persona -->`, `<!-- gentle-ai:custom-agent:{name} -->` delimit sections in config files for safe re-sync without destroying user content.

2. **Global vs project scope** — global install writes to `~/.claude/`, `~/.gemini/`, etc. Project-level (`gga init` / `--scope project`) writes to `.claude/`, `.gemini/`, etc. Project overrides user for same-named skills.

3. **Atomic writes** — writes to `.tmp` then POSIX renames to destination; concurrent readers never see a partial file.

4. **Backup before write** — auto-snapshot as `tar.gz`, deduplicated, keeps 5 most recent.

5. **Refresh-on-every-prompt hook** — `UserPromptSubmit` hook in `~/.claude/settings.json` runs:
   ```
   gentle-ai skill-registry refresh --quiet --no-gitignore --cwd "${CLAUDE_PROJECT_DIR:-$PWD}" || true
   ```
   SHA fingerprint cached in `.atl/.skill-registry.cache.json` prevents redundant rescans.

6. **"Pass paths not summaries" contract** — the skill registry (`.atl/skill-registry.md`) stores only `(name, trigger/description, scope, exact SKILL.md path)`. Orchestrators inject matching paths into sub-agent prompts; sub-agents read the full files. Preserves author intent and is compaction-safe.

7. **Three-layer injection** — MCP tools (via `.mcp.json`) + hook `additionalContext` (session start injection) + `SKILL.md` (system prompt prepend).

### Scan Sources

16 user dirs + 14 project dirs are scanned for skills. Convention files scanned: `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `GEMINI.md`, `copilot-instructions.md`.

---

## 3. Engram Internals

### Architecture

- **Language**: Go 1.25. Single binary with subcommands: `serve` (HTTP), `mcp` (stdio), `tui` (BubbleTea), `setup`, `sync`, `export`, `import`.
- **Storage**: SQLite at `~/.engram/engram.db` — WAL mode, NORMAL synchronous, 5 s busy timeout.
- **Dual server**:
  - MCP server (stdio transport, via mark3labs/mcp-go) — used by agent tool calls.
  - HTTP REST API on port `:7437` — used by shell hooks and plugin adapters (hooks cannot use stdio).

### Data Model

```
sessions         id (TEXT PK), project, directory, started_at, ended_at, summary
observations     id (AUTOINCREMENT), session_id FK, type, title, content (max 2000 chars),
                 tool_name, project, scope (project|personal), topic_key,
                 normalized_hash (SHA-256), revision_count, duplicate_count,
                 last_seen_at, created_at, updated_at, deleted_at (soft delete)
user_prompts     separate prompt storage
sync_chunks      tracks git-synced chunk IDs for dedup on import
observations_fts FTS5 virtual table on title+content+tool_name+type+project — kept in sync via SQLite triggers
```

### topic_key Upsert

On `AddObservation` with a `topic_key`: queries for an existing row with same `topic_key + project + scope`; if found, updates in-place (`revision_count++`) and returns the existing ID — no new row. This is the core mechanism Spectty uses behind `PersistencePort`.

Topic key format: `{family}/{segment}` — e.g. `architecture/auth-model`, `sdd/my-change/spec`. `SuggestTopicKey()` infers family from type + content keywords, normalizes to kebab-case, max 120 chars.

Without a topic key: deduplication by `normalized_hash` within a 15-minute window — increments `duplicate_count` instead of inserting a new row.

### Session & Compaction

- Sessions created on-demand (`INSERT OR IGNORE`) via `POST /sessions`.
- `mem_context` returns last 5 sessions + last 20 observations + last 10 prompts as formatted markdown.
- **Compaction two-sided handshake**: post-compaction hook injects `"FIRST ACTION REQUIRED: call mem_session_summary with compacted content"` into `additionalContext`. The new agent context sees this, saves the summary, and session continuity is preserved.

### Search

Pure SQLite FTS5 — **no embeddings, no vectors, no external services**. Terms are quoted before passing to `MATCH` to avoid syntax errors. BM25 ranking is used implicitly by FTS5.

### Injection Pattern (Claude Code)

1. `.mcp.json` declares `{"mcpServers": {"engram": {"command": "engram", "args": ["mcp"]}}}` — makes 19 MCP tools available.
2. `hooks.json` registers: `SessionStart` (startup → `session-start.sh`; compact → `post-compaction.sh`), `SubagentStop` (→ `subagent-stop.sh` async passive capture).
3. `session-start.sh`: starts `engram serve` if not running, creates session via HTTP, runs `engram sync --import` if `.engram/manifest.json` exists, fetches `/context`, injects Memory Protocol + context as `additionalContext` stdout.
4. `SKILL.md`: injected into Claude system prompt — contains the full memory protocol.

### Git Sync

`engram sync` exports memories as gzip-compressed JSONL chunks into `.engram/chunks/`. `manifest.json` is append-only and git-mergeable. Each developer creates independent chunks — no merge conflicts. Chunk IDs in `sync_chunks` prevent double-import.

### Extended Features (installed version vs open-source)

The open-source base has FTS5 search + topic_key upsert + 15-min dedup. The installed version (v1.15.3+) adds `mem_judge`, `judgment_required` conflict resolution, `capture_prompt`, and passive capture scanning for `## Key Learnings:` sections in sub-agent output.

---

## 4. SDD & Agent-Teams

### Artifact DAG

```
explore (opt) → propose → [spec ∥ design] → tasks → apply (batched) → verify → archive
```

`spec` and `design` are parallel. DAG state is persisted separately as `sdd/{change}/state` to survive compaction.

### Artifact Storage

| Backend | Where |
|---|---|
| `engram` (default) | topic keys: `sdd/{change}/{phase}` |
| `openspec` | `openspec/changes/{change-name}/*.md` |
| `hybrid` | both — cross-session recovery + local files |
| `none` | inline only |

### Sub-Agent Context Protocol

- Sub-agents get **fresh context** with no inherited memory.
- Orchestrator passes **artifact references** (topic keys or file paths), not content. Sub-agents do the two-step `mem_search → mem_get_observation` retrieval themselves.
- Anti-recursion: `sdd-phase-common.md` enforces the executor boundary — every SDD phase agent is an EXECUTOR, not an orchestrator. No sub-agents spawned from within a phase.
- Skill registry: orchestrator resolves once per session, passes exact `SKILL.md` paths. Sub-agents never search the registry.

### Model-Per-Phase Routing

| Phase | Model |
|---|---|
| propose, design, verify | Opus (architectural) |
| spec, tasks, apply | Sonnet (structured/mechanical) |
| archive | Haiku (copy and close) |

### Progress Model

`apply-progress` is a **running artifact** updated by each batch (merge, not overwrite). The tasks checklist with `[x]` IS the live progress model — the agent marks items complete as each batch lands.

### Human-in-the-Loop Gates

- **Interactive mode**: orchestrator pauses after each phase, shows summary, asks "Continue?" before the next phase.
- **Review workload guard** at `sdd-tasks`: 400-line budget. Delivery strategies: `ask-on-risk` (default), `auto-chain`, `single-pr`, `exception-ok`.
- **Chain strategies**: `stacked-to-main` (each PR targets main) or `feature-branch-chain` (PRs target the feature/tracker branch).
- **Strict TDD mode**: `sdd-init` detects test runner; orchestrator forwards `strict_tdd: true` to `apply` and `verify` sub-agents. Mandatory RED → GREEN → TRIANGULATE → REFACTOR cycle with evidence table.

### Token Economics

Delegation saves 50–70% for 8+ file tasks. Crossover is ~8–12 files depending on SDD dependency count. System prompt dominates fixed cost (~7,554 tokens). Sub-agent file reads disappear from orchestrator context — this is the primary saving (~60%).

### Relevance to Spectty

The SDD artifact pipeline is the direct blueprint for Spectty's **living Spec pane**:

- The tasks checklist (`[x]`) is the live progress model the Spec pane subscribes to.
- `apply-progress` topic keys are what the Spec pane polls (engram HTTP `:7437`).
- Human-in-the-loop gates (`interactive` mode, review-workload guard) map to the steering UX in the Spec pane.
- The spec delta format (ADDED / MODIFIED / REMOVED + Given/When/Then) renders as a diff in the Spec pane.

---

## 5. Licenses

| Repo | License | Verified from |
|---|---|---|
| engram | **MIT** (Copyright © 2026 Alan Buscaglia) | `~/Desktop/mcps/engram/LICENSE` + README `## License → MIT` |
| gentle-ai | **MIT** (Copyright © 2025 Gentleman Programming) | Homebrew formula `license "MIT"` + bundled `LICENSE` |
| agent-teams-lite *(archived)* | **MIT** (Copyright © 2026 Gentleman Programming) | `LICENSE` file in archived repo |

### Red-Herring Resolved

engram's README contains a comparison table with a column `AGPL-3.0`. That column refers to a **competitor** memory tool (the "web viewer on localhost:37777, auto-capture all tool calls" product). It is NOT engram's license and NOT Engram Cloud's license. engram is MIT, full stop. No AGPL contamination.

### Implications for Spectty

1. Using engram as an **external dependency** (its MCP/HTTP binary) imposes **zero license obligation** on Spectty's own code. Spectty may be proprietary or any license.
2. **Copying source code** only requires preserving the MIT copyright + notice for the copied portions.
3. **Architecture patterns and ideas** (provisioning model, injection contract, topic_key design) are not copyrightable — reusing them is unrestricted.

> ❓ OPEN: Gentleman-Skills repo license not yet verified from primary source (web-only review). Low risk for code reuse; confirm before redistributing their SKILL.md content.

---

## 6. Strategic Finding — What Spectty Reuses vs Builds

### The Core Insight

**gentle-ai is an install-time CONFIGURATOR. It has no runtime.**

The entire ecosystem — gentle-ai CLI, engram, SDD, skills — provides zero of the following: cockpit UI, living spec pane, agent activity monitor, cost/token visibility, real-time event stream, cross-session memory dashboard. Spectty's concept is **unaddressed territory** within this stack AND **complementary** to it. Spectty is the runtime cockpit they lack, not a competitor.

### Capability Map

| Capability | Reuse from stack | Build in Spectty |
|---|---|---|
| Persistent memory store | engram (Go binary, SQLite+FTS5, topic_key upsert) via `PersistencePort` | — |
| Session compaction survival | engram two-sided handshake pattern | — |
| Git-based memory sync | engram `sync` chunks (JSONL, no-conflict) | — |
| Per-agent native-format provisioning | gentle-ai pattern: managed-section markers, atomic writes, backup, SHA cache | `ProvisionerPort` adapters (one per agent) |
| Skill injection ("pass paths not summaries") | gentle-ai skill-registry contract | Spectty skill index |
| Hook-driven refresh loop | UserPromptSubmit hook wiring pattern | Spectty hook manifest |
| SDD artifact pipeline | SDD DAG + engram topic_key scheme | Living Spec pane subscribes/polls `sdd/{change}/*` keys |
| Human-in-the-loop gates | Interactive mode + review-workload guard pattern | Spec pane steering UX |
| Model-per-phase routing | SDD model assignment table | Spectty agent routing config |
| Real-time event stream | **NONE** — engram is a store, not pub/sub | **Spectty builds polling + push/subscribe layer (the #1 technical problem)** |
| Per-file/line live progress | **NONE** — granularity is task-level, after batch returns | Spectty adds finer-grained structured (JSON) progress model |
| Structured progress format | **NONE** — `apply-progress` is narrative LLM-merged text | Spectty defines a machine-readable JSON progress contract |
| Cost / token visibility UI | **NONE** | Spectty cockpit cost pane |
| Cross-session memory dashboard | engram TUI (read-only, terminal only) | Spectty memory panel (integrated, visual) |
| Terminal / PTY hosting | **NONE** | Spectty PTY layer (Tauri + portable-pty) |
| Multi-panel cockpit layout | **NONE** | Spectty layout engine |

### The #1 Technical Problem

engram is a **store**, not a pub/sub system. A live Spec pane that shows real-time agent progress requires Spectty to add:

- A **polling layer** against engram HTTP `:7437` (`GET /observations?topic_key=sdd/{change}/apply-progress`), OR
- A **push/subscribe layer** (SSE or WebSocket) on top of engram's HTTP API.

This is not provided by the stack and must be engineered by Spectty.

### Gaps Spectty Must Fill

1. **Real-time event stream** — engram has no push notifications; Spectty adds polling or SSE.
2. **Progress granularity** — task-level only today; Spectty wants per-file/line events.
3. **Structured progress format** — `apply-progress` is LLM-merged narrative text; Spectty needs machine-readable JSON.
4. **Cost/visibility UI** — zero in the stack; the entire cockpit cost pane is Spectty.
5. **PTY hosting** — no terminal emulator in the stack; Spectty owns the shell/PTY layer.
