import { act, renderHook, waitFor } from "@testing-library/react";
import { createRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// --- Tauri surface -----------------------------------------------------------
// `invoke` is a spy; the Channel mock captures the constructed instance so the
// test can fire synthetic output messages exactly like the real backend would.
const invokeMock = vi.fn();
const listenMock = vi.fn();

interface FakeChannel {
  onmessage: ((message: unknown) => void) | null;
}
let lastChannel: FakeChannel | null = null;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
    constructor() {
      lastChannel = this as FakeChannel;
    }
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

// --- xterm surface -----------------------------------------------------------
// A fake Terminal that records the calls the hook makes and lets the test drive
// the `onData` callback. No real canvas/DOM rendering happens.
type DataHandler = (data: string) => void;

interface FakeTerminalState {
  opened: HTMLElement | null;
  writes: Uint8Array[];
  disposed: boolean;
  dataHandler: DataHandler | null;
  cols: number;
  rows: number;
}

let term: FakeTerminalState;
const fitMock = vi.fn();

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    constructor() {
      term = {
        opened: null,
        writes: [],
        disposed: false,
        dataHandler: null,
        cols: 80,
        rows: 24,
      };
    }
    open(parent: HTMLElement) {
      term.opened = parent;
    }
    write(data: Uint8Array) {
      term.writes.push(data as Uint8Array);
    }
    onData(handler: DataHandler) {
      term.dataHandler = handler;
      return { dispose: vi.fn() };
    }
    loadAddon() {}
    dispose() {
      term.disposed = true;
    }
  },
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = fitMock;
  },
}));

vi.mock("@xterm/addon-clipboard", () => ({
  ClipboardAddon: class {},
}));

// ResizeObserver does not exist in jsdom — provide a controllable stub that lets
// the test trigger a resize callback synchronously.
let resizeCallback: (() => void) | null = null;
class FakeResizeObserver {
  constructor(cb: () => void) {
    resizeCallback = cb;
  }
  observe() {}
  disconnect() {}
}
vi.stubGlobal("ResizeObserver", FakeResizeObserver);

import { useTerminal } from "../../src/hooks/useTerminal";

function mountHook() {
  const ref = createRef<HTMLDivElement>();
  // Give the ref a real element so `term.open(ref.current)` has a target.
  (ref as { current: HTMLDivElement | null }).current =
    document.createElement("div");
  return renderHook(() => useTerminal(ref));
}

describe("useTerminal", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    fitMock.mockReset();
    listenMock.mockResolvedValue(() => {});
    invokeMock.mockResolvedValue("pty-1");
    lastChannel = null;
    resizeCallback = null;
  });

  it("invokes pty_spawn on mount with cols/rows and an output channel", async () => {
    mountHook();

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        "pty_spawn",
        expect.objectContaining({ cols: 80, rows: 24 }),
      );
    });
    // The fourth arg shape: onOutput must be the constructed Channel instance.
    const spawnCall = invokeMock.mock.calls.find((c) => c[0] === "pty_spawn");
    expect(spawnCall?.[1]).toHaveProperty("onOutput");
  });

  it("writes decoded channel bytes to term.write", async () => {
    mountHook();

    await waitFor(() => expect(lastChannel).not.toBeNull());

    // Fire the wire shape Tauri actually delivers: a JSON number[].
    act(() => {
      lastChannel?.onmessage?.([104, 105]); // "hi"
    });

    expect(term.writes.length).toBe(1);
    expect(term.writes[0]).toBeInstanceOf(Uint8Array);
    expect(Array.from(term.writes[0])).toEqual([104, 105]);
  });

  it("invokes send_input when term.onData fires", async () => {
    mountHook();

    await waitFor(() => expect(term.dataHandler).not.toBeNull());

    invokeMock.mockClear();
    act(() => {
      term.dataHandler?.("a");
    });

    await waitFor(() => {
      const call = invokeMock.mock.calls.find((c) => c[0] === "send_input");
      expect(call?.[1]).toMatchObject({ id: "pty-1" });
    });
  });

  it("invokes pty_resize after a ResizeObserver fit", async () => {
    mountHook();

    await waitFor(() => expect(resizeCallback).not.toBeNull());
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("pty_spawn", expect.anything()));

    invokeMock.mockClear();
    fitMock.mockClear();
    act(() => {
      resizeCallback?.();
    });

    expect(fitMock).toHaveBeenCalled();
    await waitFor(() => {
      const call = invokeMock.mock.calls.find((c) => c[0] === "pty_resize");
      expect(call?.[1]).toMatchObject({ id: "pty-1", cols: 80, rows: 24 });
    });
  });

  it("disposes the terminal and invokes pty_kill on unmount", async () => {
    const { unmount } = mountHook();

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("pty_spawn", expect.anything()));

    invokeMock.mockClear();
    unmount();

    await waitFor(() => expect(term.disposed).toBe(true));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("pty_kill", { id: "pty-1" });
    });
  });
});
