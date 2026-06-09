import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// The Tauri command names registered in `src-tauri/src/lib.rs` (M2). Kept as
// constants so the camelCase JS args ↔ snake_case Rust args mapping lives in one
// place (mirrors `pty/ipc.ts`).
const SPAWN_SESSION = "spawn_session";
const CLOSE_SESSION = "close_session";
const LIST_SESSIONS = "list_sessions";
const GET_SESSION = "get_session";

// The Tauri events the session runtime emits (snake_case payloads on the wire —
// `StatusChanged`/`SessionSummary` carry NO serde rename, so field names stay
// snake_case across the IPC boundary).
const STATUS_CHANGED_EVENT = "status_changed";
const SESSION_CREATED_EVENT = "session_created";
const SESSION_CLOSED_EVENT = "session_closed";

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
