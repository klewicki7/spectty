# ADR-0005: Build on the gentle-ai/engram Stack

- Status: Accepted
- Date: 2026-06-07
- Deciders: project owner

## Context

The gentle-ai/engram stack already exists, is MIT-licensed, and the owner uses it daily
as the structured memory and injection layer for Claude Code. It provides:

- **engram**: a persistent MCP-based memory store with a stable HTTP/MCP contract
- **Injection patterns**: per-project Provisioner, `additionalContext` hooks, SKILL.md
  injection at global and project scope
- **SDD artifact pipeline**: Spec → Design → Tasks → Apply → Verify → Archive

Spectty needs exactly these capabilities at its runtime core (session memory, agent
provisioning, spec tracking). Two questions arise: should Spectty depend on engram at
all, and if so, how tightly?

The risk of tight coupling is real: gentle-ai is a fast-moving solo-maintainer project.
The risk of ignoring it is also real: reimplementing session memory and injection
infrastructure would cost months and produce an inferior result.

## Decision

Spectty builds **on top of** the gentle-ai/engram stack, kept behind Spectty's own ports:

1. **engram as a runtime dependency behind `PersistencePort`**: The Core never imports
   engram directly. `PersistencePort` (a Rust trait / TypeScript interface at the
   boundary) defines what Spectty needs — save observation, search, get by ID. The
   engram adapter implements that port. The Core is blind to the backend.

2. **Injection patterns copied, not depended on as a binary**: Spectty's own
   `AgentProvisioner` component replicates gentle-ai's provisioning patterns
   (global-scope SKILL injection, `additionalContext` hooks, per-agent tool
   registration) into Spectty's own codebase. It **coexists** with the user's existing
   gentle-ai setup — it does not replace or conflict with it. There is no `import
   gentle_ai` at runtime; only the pattern is adopted.

3. **SDD artifact model generalized for the Spec pane**: The Spec pane's data model
   (Spec → progress → verify) is derived from the SDD pipeline. Spectty does not
   execute SDD skills directly; it generalizes the artifact shape into its own
   `SpecArtifact` domain type.

The coupling point is engram's **stable MCP/HTTP contract** — the same surface any MCP
client depends on. If engram is replaced, only the `PersistencePort` adapter changes.

## Consequences

**Positive**
- Massive leverage: session memory, artifact tracking, and agent injection infrastructure
  are proven and available on day one.
- Swappability is structural: a new backend (SQLite, Postgres, anything) requires only a
  new `PersistencePort` adapter, zero Core changes.
- The Provisioner coexistence model means Spectty improves the user's existing agent
  workflow rather than disrupting it.
- Adopting the SDD artifact shape means SDD-generated artifacts are natively legible in
  the Spec pane without a translation layer.

**Negative**
- engram is a fast-moving solo-maintainer project. Breaking changes to its MCP contract
  would require updating the `PersistencePort` adapter. Mitigated by: (a) depending only
  on the stable MCP contract, not internal APIs; (b) the adapter is a thin translation
  layer, not deep business logic.
- Copying injection patterns (rather than importing) means Spectty must track upstream
  improvements manually. Acceptable: the patterns are stable conventions, not volatile
  algorithms.

**Neutral**
- The engram process runs as a side-car MCP server, the same way it runs today in the
  owner's daily workflow. No new deployment model is introduced.
- This decision is a specific application of [ADR-0003](0003-hexagonal-architecture.md):
  every external service lives behind a port.

## Alternatives considered

### Reimplement everything independently

Build Spectty's own session memory, provisioning, and artifact tracking from scratch
without referencing gentle-ai's work.

**Why not chosen:** This is the "pride over pragmatism" path. It would cost multiple
months to reach feature parity with infrastructure that already exists, is MIT-licensed,
and is already proven in the owner's daily workflow. The result would be inferior and
later. The port-based boundary gives all the independence benefits without the cost.

### Hard-couple to engram without a port

Import engram's internals directly and call them from Core logic.

**Why not chosen:** This would bind the Core to engram's internal API surface —
the most volatile part of a fast-moving solo project. Any engram refactor becomes a
Spectty Core change. The `PersistencePort` boundary costs almost nothing to define and
eliminates that fragility entirely.
