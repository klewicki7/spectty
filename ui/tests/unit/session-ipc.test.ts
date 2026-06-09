import { describe, expect, it, vi } from "vitest";

// Mirrors `ipc.test.ts`: `invoke` and `listen` are the only Tauri surfaces the
// session ipc wrappers touch. The Channel mock keeps us off the real
// `window.__TAURI_INTERNALS__`.
const invokeMock = vi.fn();
const listenMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

import {
  closeSession,
  getSession,
  listSessions,
  listenSessionClosed,
  listenSessionCreated,
  listenStatusChanged,
  spawnSession,
  type AgentSpec,
  type SessionSummary,
} from "../../src/session/ipc";

const claudeSpec: AgentSpec = {
  kind: "claude-code",
  command: null,
  tier: "Cooperative",
};

describe("session ipc command wrappers", () => {
  it("spawnSession invokes spawn_session with the agent/workspace/title/size and the output channel", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue("session-1");
    const channel = { onmessage: null };

    const id = await spawnSession(
      claudeSpec,
      "/repo",
      "My Agent",
      80,
      24,
      channel as never,
    );

    expect(id).toBe("session-1");
    // Tauri v2 converts these camelCase keys to the Rust snake_case args
    // (`workspace_path`, `on_output`) — mirror the M1 `pty_spawn` convention.
    expect(invokeMock).toHaveBeenCalledWith("spawn_session", {
      agent: claudeSpec,
      workspacePath: "/repo",
      title: "My Agent",
      cols: 80,
      rows: 24,
      onOutput: channel,
    });
  });

  it("closeSession invokes close_session with the bare session id", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);

    await closeSession("session-1");

    // `SessionId` is a one-field tuple struct → serializes as a bare string, so
    // the `id` arg is the plain string (no wrapper object).
    expect(invokeMock).toHaveBeenCalledWith("close_session", {
      id: "session-1",
    });
  });

  it("listSessions invokes list_sessions and returns the summaries", async () => {
    invokeMock.mockReset();
    const summaries: SessionSummary[] = [
      {
        id: "session-1",
        title: "My Agent",
        status: "Idle",
        agent_kind: "claude-code",
      },
    ];
    invokeMock.mockResolvedValue(summaries);

    const result = await listSessions();

    expect(result).toEqual(summaries);
    expect(invokeMock).toHaveBeenCalledWith("list_sessions");
  });

  it("getSession invokes get_session with the id and returns the summary or null", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(null);

    const result = await getSession("missing");

    expect(result).toBeNull();
    expect(invokeMock).toHaveBeenCalledWith("get_session", { id: "missing" });
  });
});

describe("session ipc event listeners", () => {
  it("listenStatusChanged subscribes to the status_changed event", async () => {
    listenMock.mockReset();
    const unlisten = () => {};
    listenMock.mockResolvedValue(unlisten);
    const handler = vi.fn();

    const result = await listenStatusChanged(handler);

    expect(result).toBe(unlisten);
    expect(listenMock).toHaveBeenCalledWith("status_changed", expect.any(Function));
  });

  it("listenStatusChanged forwards the snake_case payload to the handler", async () => {
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => {});
    const handler = vi.fn();

    await listenStatusChanged(handler);
    const tauriHandler = listenMock.mock.calls[0][1] as (event: {
      payload: unknown;
    }) => void;

    tauriHandler({
      payload: { session_id: "session-1", status: "Running", quick_actions: [] },
    });

    expect(handler).toHaveBeenCalledWith({
      session_id: "session-1",
      status: "Running",
      quick_actions: [],
    });
  });

  it("listenSessionCreated subscribes to the session_created event", async () => {
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => {});

    await listenSessionCreated(vi.fn());

    expect(listenMock).toHaveBeenCalledWith(
      "session_created",
      expect.any(Function),
    );
  });

  it("listenSessionClosed subscribes to the session_closed event and forwards the bare id", async () => {
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => {});
    const handler = vi.fn();

    await listenSessionClosed(handler);
    const tauriHandler = listenMock.mock.calls[0][1] as (event: {
      payload: unknown;
    }) => void;
    tauriHandler({ payload: "session-1" });

    expect(listenMock).toHaveBeenCalledWith(
      "session_closed",
      expect.any(Function),
    );
    expect(handler).toHaveBeenCalledWith("session-1");
  });
});
