import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// The Tauri command names registered in `src-tauri/src/lib.rs` (M2). Kept as
// constants so the camelCase JS args ↔ snake_case Rust args mapping lives in one
// place (mirrors `pty/ipc.ts`).
const SPAWN_SESSION = "spawn_session";
const CLOSE_SESSION = "close_session";
const LIST_SESSIONS = "list_sessions";
const GET_SESSION = "get_session";

// M4 triad commands (registered in `src-tauri/src/commands/spec.rs`). `get_spec`
// / `get_diff_explanation` read the current contract / explanation on demand
// (mount + manual refresh); `approve_prompt` resolves the plan-approval gate.
const GET_SPEC = "get_spec";
const GET_DIFF_EXPLANATION = "get_diff_explanation";
const APPROVE_PROMPT = "approve_prompt";

// The Tauri events the session runtime emits (snake_case payloads on the wire —
// `StatusChanged`/`SessionSummary` carry NO serde rename, so field names stay
// snake_case across the IPC boundary).
const STATUS_CHANGED_EVENT = "status_changed";
const SESSION_CREATED_EVENT = "session_created";
const SESSION_CLOSED_EVENT = "session_closed";

// M4 triad events (emitted from the SpecBus / diff pipeline poll loops, D29).
// `spec_updated` carries the full `SpecContract`; `diff_updated` the per-file
// `DiffExplanation`. Both payloads are snake_case on the wire (no serde rename).
const SPEC_UPDATED_EVENT = "spec_updated";
const DIFF_UPDATED_EVENT = "diff_updated";

/**
 * Session identity. `SessionId` is a single-field tuple struct on the Rust side
 * (`crates/core/src/entities/session.rs`), so it crosses the IPC boundary as a
 * bare string (== PtyId, D13).
 */
export type SessionId = string;

/**
 * Which agent runs. `AgentKind` is a serde STRING newtype (D12) so it arrives as
 * a bare string; the two M2 first-class kinds are listed for autocomplete but the
 * Core never branches on the value.
 */
export type AgentKind = "claude-code" | "generic" | (string & {});

/**
 * How cooperatively the agent participates in the protocol. `AgentTier` is an
 * externally-tagged unit enum on the Rust side → bare string on the wire.
 */
export type AgentTier = "Cooperative" | "Generic";

/**
 * The six-variant lifecycle status. `AgentStatus` is an externally-tagged unit
 * enum on the Rust side → each variant is a bare string on the wire. The UI NEVER
 * computes this locally; the backend `transition` is authoritative.
 */
export type AgentStatus =
  | "Starting"
  | "Idle"
  | "Running"
  | "AwaitingInput"
  | "Completed"
  | "Error";

/** What agent a session runs (`AgentSpec` on the Rust side). */
export interface AgentSpec {
  kind: AgentKind;
  /** Generic agent: the user-supplied program + args; `null` for first-class. */
  command: string[] | null;
  tier: AgentTier;
}

/** UI-facing projection of a session (`SessionSummary`; snake_case `agent_kind`). */
export interface SessionSummary {
  id: SessionId;
  title: string;
  status: AgentStatus;
  agent_kind: AgentKind;
}

/** Payload of the `status_changed` event (`StatusChanged`; snake_case fields). */
export interface StatusChangedPayload {
  session_id: SessionId;
  status: AgentStatus;
  quick_actions: unknown[];
}

// ── M4 triad: SpecContract / DiffExplanation wire types ────────────────────────
//
// Mirror the Rust serde shapes (`crates/core/src/entities/{spec,diff}.rs`). The
// UI NEVER computes spec/diff state locally — it renders whatever the backend
// `spec_updated` / `diff_updated` events (or the `get_*` reads) deliver.

/**
 * A task's lifecycle state. Crosses the wire under the field name `status` (the
 * Rust `SpecTask::state` is `#[serde(rename = "status")]`, snake_case variants).
 */
const TASK_STATE = {
  PENDING: "pending",
  IN_PROGRESS: "in_progress",
  DONE: "done",
  SKIPPED: "skipped",
} as const;

export type TaskState = (typeof TASK_STATE)[keyof typeof TASK_STATE];

/**
 * The plan-approval lifecycle (`ApprovalState`). PascalCase on the wire — the Rust
 * enum has no `rename_all`, so each variant serializes as its identifier.
 */
const APPROVAL_STATE = {
  PENDING: "Pending",
  APPROVED: "Approved",
  REJECTED: "Rejected",
  ADJUSTED: "Adjusted",
} as const;

export type ApprovalState = (typeof APPROVAL_STATE)[keyof typeof APPROVAL_STATE];

/**
 * The user's decision on the plan-approval gate. snake_case on the wire — the Rust
 * `ApprovalDecision` enum is `#[serde(rename_all = "snake_case")]`.
 */
const APPROVAL_DECISION = {
  APPROVE: "approve",
  REJECT: "reject",
  ADJUST: "adjust",
} as const;

export type ApprovalDecision =
  (typeof APPROVAL_DECISION)[keyof typeof APPROVAL_DECISION];

/** One planned task in a `SpecContract`. The lifecycle field is `status` (D32). */
export interface SpecTask {
  id: string;
  title: string;
  status: TaskState;
  notes?: string | null;
}

/** One incremental progress entry. */
export interface TaskProgress {
  task_id: string;
  status: TaskState;
}

/** The living plan-and-progress aggregate (`SpecContract`, D32). */
export interface SpecContract {
  intent: string;
  proposal?: string | null;
  tasks: SpecTask[];
  progress: TaskProgress[];
  approval: ApprovalState;
  steering_notes: string[];
  dev_override: boolean;
}

/** The rationale for one changed file (`FileExplanation`, D34). */
export interface FileExplanation {
  path: string;
  rationale: string;
}

/** The explanation of a whole working-tree diff (`DiffExplanation`, D34). */
export interface DiffExplanation {
  files: FileExplanation[];
  summary: string;
}

/** Payload of the `spec_updated` event (`SpecUpdated`; snake_case fields, D29). */
export interface SpecUpdatedPayload {
  session_id: SessionId;
  spec: SpecContract;
}

/** Payload of the `diff_updated` event (`DiffUpdated`; snake_case fields, D29). */
export interface DiffUpdatedPayload {
  session_id: SessionId;
  explanation: DiffExplanation;
}

/**
 * Spawn an agent session: resolve the runner, inject provisioning, open the PTY,
 * and stream its output over `onOutput`. Returns the minted session id.
 *
 * The camelCase keys map to the Rust snake_case command args (`workspace_path`,
 * `on_output`) via Tauri v2's default conversion — same convention as M1
 * `pty_spawn` in `pty/ipc.ts`.
 */
export async function spawnSession(
  agent: AgentSpec,
  workspacePath: string,
  title: string,
  cols: number,
  rows: number,
  onOutput: Channel<unknown>,
): Promise<SessionId> {
  return invoke<SessionId>(SPAWN_SESSION, {
    agent,
    workspacePath,
    title,
    cols,
    rows,
    onOutput,
  });
}

/** Tear down a session: kill the PTY and retract its injected provisioning. */
export async function closeSession(id: SessionId): Promise<void> {
  await invoke(CLOSE_SESSION, { id });
}

/** Project every live session into a `SessionSummary`. */
export async function listSessions(): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>(LIST_SESSIONS);
}

/** Look up one session's summary by id (`null` when absent). */
export async function getSession(
  id: SessionId,
): Promise<SessionSummary | null> {
  return invoke<SessionSummary | null>(GET_SESSION, { id });
}

/**
 * Subscribe to `status_changed`. The backend fires it only on a REAL transition
 * (data-flow.md principle 1 — backend authoritative); the handler receives the
 * raw snake_case payload.
 */
export async function listenStatusChanged(
  onStatus: (payload: StatusChangedPayload) => void,
): Promise<UnlistenFn> {
  return listen<StatusChangedPayload>(STATUS_CHANGED_EVENT, (event) => {
    onStatus(event.payload);
  });
}

/** Subscribe to `session_created` (payload = the new `SessionSummary`). */
export async function listenSessionCreated(
  onCreated: (summary: SessionSummary) => void,
): Promise<UnlistenFn> {
  return listen<SessionSummary>(SESSION_CREATED_EVENT, (event) => {
    onCreated(event.payload);
  });
}

/** Subscribe to `session_closed` (payload = the bare `SessionId` string). */
export async function listenSessionClosed(
  onClosed: (id: SessionId) => void,
): Promise<UnlistenFn> {
  return listen<SessionId>(SESSION_CLOSED_EVENT, (event) => {
    onClosed(event.payload);
  });
}

// ── M4 triad: spec / diff commands + listeners (D29) ───────────────────────────

/**
 * Read the current `SpecContract` for a session (`null` when none is stored or the
 * stored blob is corrupt). Used on (re-)attach to hydrate immediately; live updates
 * arrive via `spec_updated`.
 */
export async function getSpec(id: SessionId): Promise<SpecContract | null> {
  return invoke<SpecContract | null>(GET_SPEC, { sessionId: id });
}

/**
 * Read the current `DiffExplanation` for a session (`null` when the pipeline has not
 * produced one yet). Used on mount and as the manual-refresh fallback; live updates
 * arrive via `diff_updated`.
 */
export async function getDiffExplanation(
  id: SessionId,
): Promise<DiffExplanation | null> {
  return invoke<DiffExplanation | null>(GET_DIFF_EXPLANATION, { sessionId: id });
}

/**
 * Resolve the plan-approval gate for `(session_id, action_id)` with the user's
 * `decision`, unblocking the agent's `spectty_approval` long-poll. Resolves to
 * `true` when a pending request was resolved, `false` for an unknown key (no-op).
 */
export async function approvePrompt(
  id: SessionId,
  actionId: string,
  decision: ApprovalDecision,
): Promise<boolean> {
  return invoke<boolean>(APPROVE_PROMPT, {
    sessionId: id,
    actionId,
    decision,
  });
}

/**
 * Subscribe to `spec_updated`. The backend emits it only on an ACTUAL spec change
 * (D29 — change-detected poll loop); the handler receives the raw snake_case payload.
 */
export async function listenSpecUpdated(
  onSpec: (payload: SpecUpdatedPayload) => void,
): Promise<UnlistenFn> {
  return listen<SpecUpdatedPayload>(SPEC_UPDATED_EVENT, (event) => {
    onSpec(event.payload);
  });
}

/**
 * Subscribe to `diff_updated`. The backend emits it only when the working-tree diff
 * actually changed (D37 — hash-deduped pipeline); the handler receives the raw
 * snake_case payload.
 */
export async function listenDiffUpdated(
  onDiff: (payload: DiffUpdatedPayload) => void,
): Promise<UnlistenFn> {
  return listen<DiffUpdatedPayload>(DIFF_UPDATED_EVENT, (event) => {
    onDiff(event.payload);
  });
}
