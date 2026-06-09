import { act, renderHook, waitFor } from "@testing-library/react";
import { createRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// --- Tauri surface -----------------------------------------------------------
const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

// --- xterm surface -----------------------------------------------------------
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

// ResizeObserver stub
let resizeCallback: (() => void) | null = null;
class FakeResizeObserver {
  constructor(cb: () => void) {
    resizeCallback = cb;
  }
  observe() {}
  disconnect() {}
}
vi.stubGlobal("ResizeObserver", FakeResizeObserver);

// --- Import hook AFTER mocks -------------------------------------------------
import {
  useSessionTerminal,
  createBufferedOutputChannel,
} from "../../src/hooks/useSessionTerminal";

function mountHook(sessionId = "session-1") {
  const ref = createRef<HTMLDivElement>();
  (ref as { current: HTMLDivElement | null }).current =
    document.createElement("div");
  const buffered = createBufferedOutputChannel();
  return { hook: renderHook(() => useSessionTerminal(ref, sessionId, buffered)), buffered };
}

describe("useSessionTerminal", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    fitMock.mockReset();
    resizeCallback = null;
    // send_input and pty_resize return nothing meaningful
    invokeMock.mockResolvedValue(undefined);
  });

  it("opens the terminal into the container on mount", async () => {
    mountHook();

    await waitFor(() => expect(term.opened).not.toBeNull());
  });

  it("does NOT invoke pty_spawn (session already exists)", async () => {
    mountHook();

    // Give it a tick to settle
    await waitFor(() => expect(term.opened).not.toBeNull());

    const spawnCall = invokeMock.mock.calls.find((c) => c[0] === "pty_spawn");
    expect(spawnCall).toBeUndefined();
  });

  it("writes decoded bytes from the provided output channel to the terminal", async () => {
    const ref = createRef<HTMLDivElement>();
    (ref as { current: HTMLDivElement | null }).current =
      document.createElement("div");

    // Build a buffered channel whose raw channel.onmessage we control.
    const buffered = createBufferedOutputChannel();
    renderHook(() => useSessionTerminal(ref, "session-1", buffered));

    await waitFor(() => expect(term.opened).not.toBeNull());

    // Simulate backend bytes arriving through the channel after mount
    act(() => {
      buffered.channel.onmessage?.([72, 101, 108, 108, 111]); // "Hello"
    });

    expect(term.writes.length).toBe(1);
    expect(Array.from(term.writes[0])).toEqual([72, 101, 108, 108, 111]);
  });

  it("buffers bytes that arrive BEFORE drainTo is called, then flushes them", () => {
    // Directly test the createBufferedOutputChannel contract: bytes pushed
    // before drainTo are queued and flushed in order when drainTo fires.
    const { channel, drainTo } = createBufferedOutputChannel();

    // Push bytes before drainTo is called (simulates early backend output)
    channel.onmessage?.([1, 2, 3]);
    channel.onmessage?.([4, 5, 6]);

    const writes: Uint8Array[] = [];
    drainTo((bytes) => writes.push(bytes));

    expect(writes.length).toBe(2);
    expect(Array.from(writes[0])).toEqual([1, 2, 3]);
    expect(Array.from(writes[1])).toEqual([4, 5, 6]);
  });

  it("routes bytes arriving after drainTo directly to the sink without buffering", () => {
    const { channel, drainTo } = createBufferedOutputChannel();

    const writes: Uint8Array[] = [];
    drainTo((bytes) => writes.push(bytes));

    // After drainTo, messages go directly to sink
    channel.onmessage?.([7, 8, 9]);

    expect(writes.length).toBe(1);
    expect(Array.from(writes[0])).toEqual([7, 8, 9]);
  });

  it("invokes send_input when term.onData fires", async () => {
    mountHook("session-42");

    await waitFor(() => expect(term.dataHandler).not.toBeNull());

    invokeMock.mockClear();
    act(() => {
      term.dataHandler?.("z");
    });

    await waitFor(() => {
      const call = invokeMock.mock.calls.find((c) => c[0] === "send_input");
      expect(call?.[1]).toMatchObject({ id: "session-42" });
    });
  });

  it("invokes pty_resize after a ResizeObserver fit", async () => {
    mountHook("session-42");

    await waitFor(() => expect(resizeCallback).not.toBeNull());
    await waitFor(() => expect(term.opened).not.toBeNull());

    invokeMock.mockClear();
    fitMock.mockClear();
    act(() => {
      resizeCallback?.();
    });

    expect(fitMock).toHaveBeenCalled();
    await waitFor(() => {
      const call = invokeMock.mock.calls.find((c) => c[0] === "pty_resize");
      expect(call?.[1]).toMatchObject({ id: "session-42", cols: 80, rows: 24 });
    });
  });

  it("disposes the terminal and disconnects observer on unmount — does NOT call pty_kill", async () => {
    const { hook } = mountHook();

    await waitFor(() => expect(term.opened).not.toBeNull());

    invokeMock.mockClear();
    hook.unmount();

    await waitFor(() => expect(term.disposed).toBe(true));

    // MUST NOT kill the PTY — session teardown is owned by close_session
    const killCall = invokeMock.mock.calls.find((c) => c[0] === "pty_kill");
    expect(killCall).toBeUndefined();
  });
});
