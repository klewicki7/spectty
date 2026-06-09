import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// --- Tauri surface -----------------------------------------------------------
// Mirror `useTerminal.test.ts`: `invoke` is a spy and `listen` captures the
// handler registered per event so the test can fire synthetic backend events.
const invokeMock = vi.fn();
const listenMock = vi.fn();

const eventHandlers = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: (e: { payload: unknown }) => void) => {
    eventHandlers.set(event, handler);
    return listenMock(event, handler);
  },
}));

import { useSession } from "../../src/hooks/useSession";
import type { AgentSpec } from "../../src/session/ipc";

const claudeSpec: AgentSpec = {
  kind: "claude-code",
  command: null,
  tier: "Cooperative",
};

function fireEvent(event: string, payload: unknown) {
  act(() => {
    eventHandlers.get(event)?.({ payload });
  });
}

describe("useSession", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    eventHandlers.clear();
    listenMock.mockReturnValue(Promise.resolve(() => {}));
    invokeMock.mockResolvedValue("session-1");
  });

  it("starts with no session and a null status", () => {
    const { result } = renderHook(() => useSession());

    expect(result.current.session).toBeNull();
    expect(result.current.status).toBeNull();
  });

  it("spawn() invokes spawn_session with the chosen agent and workspace", async () => {
    const { result } = renderHook(() => useSession());

    await act(async () => {
      await result.current.spawn(claudeSpec, "/repo", "My Agent");
    });

    const call = invokeMock.mock.calls.find((c) => c[0] === "spawn_session");
    expect(call?.[1]).toMatchObject({
      agent: claudeSpec,
      workspacePath: "/repo",
      title: "My Agent",
    });
  });

  it("tracks the session after spawn resolves", async () => {
    const { result } = renderHook(() => useSession());

    await act(async () => {
      await result.current.spawn(claudeSpec, "/repo", "My Agent");
    });

    await waitFor(() => {
      expect(result.current.session?.id).toBe("session-1");
      expect(result.current.session?.title).toBe("My Agent");
    });
  });

  it("updates status when a status_changed event arrives for the current session", async () => {
    const { result } = renderHook(() => useSession());

    await act(async () => {
      await result.current.spawn(claudeSpec, "/repo", "My Agent");
    });
    await waitFor(() => expect(result.current.session?.id).toBe("session-1"));

    fireEvent("status_changed", {
      session_id: "session-1",
      status: "Running",
      quick_actions: [],
    });

    await waitFor(() => expect(result.current.status).toBe("Running"));
  });

  it("ignores a status_changed event for a different session", async () => {
    const { result } = renderHook(() => useSession());

    await act(async () => {
      await result.current.spawn(claudeSpec, "/repo", "My Agent");
    });
    await waitFor(() => expect(result.current.session?.id).toBe("session-1"));

    fireEvent("status_changed", {
      session_id: "other-session",
      status: "Error",
      quick_actions: [],
    });

    // Status stays whatever it was (never adopts another session's status).
    expect(result.current.status).not.toBe("Error");
  });

  it("clears the session when a session_closed event arrives for it", async () => {
    const { result } = renderHook(() => useSession());

    await act(async () => {
      await result.current.spawn(claudeSpec, "/repo", "My Agent");
    });
    await waitFor(() => expect(result.current.session?.id).toBe("session-1"));

    fireEvent("session_closed", "session-1");

    await waitFor(() => {
      expect(result.current.session).toBeNull();
      expect(result.current.status).toBeNull();
    });
  });

  it("close() invokes close_session with the current session id", async () => {
    const { result } = renderHook(() => useSession());

    await act(async () => {
      await result.current.spawn(claudeSpec, "/repo", "My Agent");
    });
    await waitFor(() => expect(result.current.session?.id).toBe("session-1"));

    invokeMock.mockClear();
    await act(async () => {
      await result.current.close();
    });

    expect(invokeMock).toHaveBeenCalledWith("close_session", {
      id: "session-1",
    });
  });
});
