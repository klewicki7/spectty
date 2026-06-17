import { describe, expect, it, vi } from "vitest";

// Mirrors `session-ipc.test.ts`: only `invoke` and `listen` are touched by the
// triad ipc wrappers. The mocks keep us off the real `window.__TAURI_INTERNALS__`.
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
  approvePrompt,
  getDiffExplanation,
  getSpec,
  listenDiffUpdated,
  listenSpecUpdated,
  type DiffExplanation,
  type SpecContract,
} from "../../src/session/ipc";

const sampleSpec: SpecContract = {
  intent: "fix the auth bug",
  proposal: "a detailed plan",
  tasks: [{ id: "t1", title: "write test", status: "in_progress" }],
  progress: [],
  approval: "Pending",
  steering_notes: [],
  dev_override: false,
};

const sampleDiff: DiffExplanation = {
  files: [{ path: "src/a.rs", rationale: "added a guard" }],
  summary: "1 file changed",
};

describe("triad ipc command wrappers", () => {
  it("getSpec invokes get_spec with the session id and returns the contract or null", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(sampleSpec);

    const result = await getSpec("session-1");

    expect(result).toEqual(sampleSpec);
    // Tauri v2 converts the camelCase `sessionId` key to the Rust snake_case
    // `session_id` arg (mirrors the M2 wrappers).
    expect(invokeMock).toHaveBeenCalledWith("get_spec", {
      sessionId: "session-1",
    });
  });

  it("getSpec returns null when no spec is stored", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(null);

    const result = await getSpec("missing");

    expect(result).toBeNull();
  });

  it("getDiffExplanation invokes get_diff_explanation with the session id", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(sampleDiff);

    const result = await getDiffExplanation("session-1");

    expect(result).toEqual(sampleDiff);
    expect(invokeMock).toHaveBeenCalledWith("get_diff_explanation", {
      sessionId: "session-1",
    });
  });

  it("approvePrompt invokes approve_prompt with the session id, action id, and decision", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(true);

    const ok = await approvePrompt("session-1", "edit-1", "approve");

    expect(ok).toBe(true);
    // The `decision` arrives as the snake_case Core string ("approve"/"reject"/
    // "adjust") that `ApprovalDecision` deserializes (commands/spec.rs).
    expect(invokeMock).toHaveBeenCalledWith("approve_prompt", {
      sessionId: "session-1",
      actionId: "edit-1",
      decision: "approve",
    });
  });
});

describe("triad ipc event listeners", () => {
  it("listenSpecUpdated subscribes to the spec_updated event and forwards the payload", async () => {
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => {});
    const handler = vi.fn();

    await listenSpecUpdated(handler);
    const tauriHandler = listenMock.mock.calls[0][1] as (event: {
      payload: unknown;
    }) => void;
    tauriHandler({ payload: { session_id: "session-1", spec: sampleSpec } });

    expect(listenMock).toHaveBeenCalledWith(
      "spec_updated",
      expect.any(Function),
    );
    expect(handler).toHaveBeenCalledWith({
      session_id: "session-1",
      spec: sampleSpec,
    });
  });

  it("listenDiffUpdated subscribes to the diff_updated event and forwards the payload", async () => {
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => {});
    const handler = vi.fn();

    await listenDiffUpdated(handler);
    const tauriHandler = listenMock.mock.calls[0][1] as (event: {
      payload: unknown;
    }) => void;
    tauriHandler({
      payload: { session_id: "session-1", explanation: sampleDiff },
    });

    expect(listenMock).toHaveBeenCalledWith(
      "diff_updated",
      expect.any(Function),
    );
    expect(handler).toHaveBeenCalledWith({
      session_id: "session-1",
      explanation: sampleDiff,
    });
  });
});
