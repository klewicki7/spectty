# Capability: session-registry

> M2 delta spec. New capability established by change `M2-spawn-agent-provisioner`.
> RFC 2119 keywords are normative. Full prose + verification-class tags live in the
> change-level `spec.md`.

The `Session` aggregate root and `SessionRegistry` live in Core, using the M0/M1
`PersistencePort`-style `&self` interior-mutability convention and shared as `tauri::State`.
The Core registry owns ONLY domain state; OS handles stay in the `src-tauri` `PtyRegistry`.

## ADDED Requirements

### Requirement: Session aggregate carries the M2 fields
`spectty-core` MUST grow `Session` into an aggregate root carrying at minimum `id:
SessionId`, `workspace: WorkspaceId`, `agent: AgentSpec`, `status: AgentStatus`, `title`,
`created_at`. `Spec`, `CostMetrics`, `Worktree`, `last_diff` MAY be skeleton fields or
deferred; their behavior MUST NOT be implemented in M2.

#### Scenario: Session exposes id, workspace, agent, status, title, created_at
- **Given** the `spectty-core` `Session` aggregate after M2
- **When** its fields are inspected
- **Then** `id`, `workspace`, `agent`, `status`, `title`, and `created_at` MUST each be
  present (with `CostMetrics` permitted as a skeleton field)

### Requirement: SessionRegistry creates, looks up, and closes sessions
`spectty-core` MUST define a `SessionRegistry` owning `Session` aggregates with `create`,
look-up by `SessionId`, and `close`, using the `&self` interior-mutability convention. It
MUST mint `SessionId`s by migrating the M1 `next_pty_id` counter so `SessionId == PtyId`.

#### Scenario: create then look up returns the same session
- **Given** an empty `SessionRegistry`
- **When** `create` is called with a workspace + `AgentSpec`, then the returned id is looked up
- **Then** the looked-up `Session` MUST be the one just created, with matching workspace and
  agent, asserted as a pure unit

#### Scenario: close removes the session from lookup
- **Given** a `SessionRegistry` holding one created session
- **When** `close` is called with that session's id
- **Then** a subsequent look-up MUST report the session as closed / absent

#### Scenario: SessionRegistry mints ids via &self interior mutability
- **Given** a `SessionRegistry` shared behind a shared reference
- **When** `create` is invoked twice through `&self`
- **Then** two DISTINCT `SessionId`s MUST be minted (monotonic), asserted without a mutable
  borrow of the registry

### Requirement: SessionRegistry stays distinct from the src-tauri PtyRegistry
The Core `SessionRegistry` MUST own only `Session` domain state and MUST NOT import `tauri`,
`portable-pty`, or hold OS handles (writer / child / read-thread stop). Those remain in the
M1 `PtyRegistry`; `SessionId == PtyId` keys both in lockstep.

#### Scenario: The Core registry holds no OS handle
- **Given** the `spectty-core` `SessionRegistry`
- **When** its stored entry shape is inspected
- **Then** it MUST hold only `Session` domain state AND MUST NOT hold a PTY writer, a child
  handle, or any `portable-pty`/`tauri` type
