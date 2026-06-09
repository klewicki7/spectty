import { useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";

import {
  closeSession,
  listenSessionClosed,
  listenStatusChanged,
  spawnSession,
  type AgentSpec,
  type AgentStatus,
  type SessionSummary,
} from "../session/ipc";

/** What the session hook exposes to the spawn UI + Pane header. */
export interface UseSession {
  session: SessionSummary | null;
  status: AgentStatus | null;
  spawn: (
    agent: AgentSpec,
    workspacePath: string,
    title: string,
    onOutput?: Channel<unknown>,
  ) => Promise<void>;
  close: () => Promise<void>;
}

/** Default PTY geometry used when the caller does not size the session. */
const DEFAULT_COLS = 80;
const DEFAULT_ROWS = 24;

/**
 * Drive a single agent session against the M2 backend.
 *
 * Mirrors `useTerminal`'s shape: one `useEffect` owns the event-listener
 * lifecycle and returns a cleanup. React 19 / React Compiler — no manual
 * `useMemo`/`useCallback`, named imports only. The hook NEVER computes status
 * locally: it adopts whatever the backend `status_changed` event reports for the
 * current session and ignores events for any other session (backend authoritative,
 * data-flow.md principle 1).
 */
export function useSession(): UseSession {
  const [session, setSession] = useState<SessionSummary | null>(null);
  const [status, setStatus] = useState<AgentStatus | null>(null);

  // The live session id is mirrored in a ref so the event listeners (registered
  // once on mount) always filter against the CURRENT session without re-subscribing.
  const sessionIdRef = useRef<string | null>(null);
  sessionIdRef.current = session?.id ?? null;

  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    void listenStatusChanged((payload) => {
      if (payload.session_id === sessionIdRef.current) {
        setStatus(payload.status);
        setSession((prev) =>
          prev && prev.id === payload.session_id
            ? { ...prev, status: payload.status }
            : prev,
        );
      }
    }).then((unlisten) => unlisteners.push(unlisten));

    void listenSessionClosed((id) => {
      if (id === sessionIdRef.current) {
        setSession(null);
        setStatus(null);
      }
    }).then((unlisten) => unlisteners.push(unlisten));

    return () => {
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }, []);

  const spawn = async (
    agent: AgentSpec,
    workspacePath: string,
    title: string,
    onOutput?: Channel<unknown>,
  ): Promise<void> => {
    const channel = onOutput ?? new Channel<unknown>();
    const id = await spawnSession(
      agent,
      workspacePath,
      title,
      DEFAULT_COLS,
      DEFAULT_ROWS,
      channel,
    );
    // Seed the local projection from the returned id; the authoritative status
    // arrives via `status_changed` (Starting → …). `session_created` would also
    // carry the full summary, but seeding here keeps the UI responsive.
    setSession({ id, title, status: "Starting", agent_kind: agent.kind });
    setStatus("Starting");
  };

  const close = async (): Promise<void> => {
    const id = sessionIdRef.current;
    if (id === null) {
      return;
    }
    await closeSession(id);
    setSession(null);
    setStatus(null);
  };

  return { session, status, spawn, close };
}
