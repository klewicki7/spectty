import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Mock the Tauri v2 API surface the hook depends on. `invoke` becomes a spy we
// can assert against; `listen` captures the registered callback so the test can
// fire a synthetic "pong" event without any running backend.
const invokeMock = vi.fn();
const listenMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

import { usePingPong } from "../../src/hooks/usePingPong";

type PongHandler = (event: { payload: string }) => void;

describe("usePingPong", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    // listen() resolves to an unlisten function in the real API.
    listenMock.mockResolvedValue(() => {});
    invokeMock.mockResolvedValue(undefined);
  });

  it("registers a 'pong' listener on mount", async () => {
    renderHook(() => usePingPong());

    await waitFor(() => {
      expect(listenMock).toHaveBeenCalledWith("pong", expect.any(Function));
    });
  });

  it("invokes the 'ping' command when sendPing is called", async () => {
    const { result } = renderHook(() => usePingPong());

    await act(async () => {
      await result.current.sendPing();
    });

    expect(invokeMock).toHaveBeenCalledWith("ping");
  });

  it("surfaces the payload when a 'pong' event fires", async () => {
    const { result } = renderHook(() => usePingPong());

    await waitFor(() => expect(listenMock).toHaveBeenCalled());

    // Pull the handler the hook registered and fire a synthetic pong.
    const [, handler] = listenMock.mock.calls[0] as [string, PongHandler];
    act(() => {
      handler({ payload: "pong from spectty backend" });
    });

    await waitFor(() => {
      expect(result.current.pong).toBe("pong from spectty backend");
    });
  });
});
