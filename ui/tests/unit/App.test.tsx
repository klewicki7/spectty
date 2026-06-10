import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// --- Tauri surface -----------------------------------------------------------
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

// --- xterm surface -----------------------------------------------------------
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    open() {}
    write() {}
    onData() {
      return { dispose: vi.fn() };
    }
    loadAddon() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();
  },
}));
vi.mock("@xterm/addon-clipboard", () => ({
  ClipboardAddon: class {},
}));
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

// ResizeObserver stub
class FakeResizeObserver {
  observe() {}
  disconnect() {}
}
vi.stubGlobal("ResizeObserver", FakeResizeObserver);

// --- Import AFTER mocks ------------------------------------------------------
import { App } from "../../src/App";

function fireBackendEvent(event: string, payload: unknown) {
  act(() => {
    eventHandlers.get(event)?.({ payload });
  });
}

describe("App — session routing", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    eventHandlers.clear();
    listenMock.mockReturnValue(Promise.resolve(() => {}));
    invokeMock.mockResolvedValue("session-1");
  });

  it("shows SpawnDialog and M1 Terminal (one terminal-pane) when no session is active", () => {
    render(<App />);
    // SpawnDialog must be visible
    expect(screen.getByRole("button", { name: /spawn/i })).not.toBeNull();
    // Exactly one terminal-pane (the M1 shell terminal)
    const panes = document.querySelectorAll(".terminal-pane");
    expect(panes.length).toBe(1);
  });

  it("hides SpawnDialog and M1 Terminal and renders SessionTerminal after spawn", async () => {
    render(<App />);

    await act(async () => {
      screen.getByRole("button", { name: /spawn/i }).click();
    });

    // SpawnDialog must be gone
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /spawn/i })).toBeNull();
    });

    // SessionTerminal renders one terminal-pane (M1 Terminal is unmounted)
    await waitFor(() => {
      const panes = document.querySelectorAll(".terminal-pane");
      expect(panes.length).toBe(1);
    });
  });

  it("passes a connected onOutput channel to spawn_session (not undefined)", async () => {
    render(<App />);

    await act(async () => {
      screen.getByRole("button", { name: /spawn/i }).click();
    });

    await waitFor(() => {
      const call = invokeMock.mock.calls.find((c) => c[0] === "spawn_session");
      expect(call).toBeDefined();
      // onOutput must be a Channel instance, not undefined or a fallback
      const onOutput = call?.[1].onOutput;
      expect(onOutput).not.toBeUndefined();
      // The channel must have an onmessage property (it's a real Channel)
      expect(onOutput).toHaveProperty("onmessage");
    });
  });

  it("re-shows SpawnDialog and M1 Terminal after session_closed fires", async () => {
    render(<App />);

    // Spawn
    await act(async () => {
      screen.getByRole("button", { name: /spawn/i }).click();
    });
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /spawn/i })).toBeNull(),
    );

    // Backend closes the session
    fireBackendEvent("session_closed", "session-1");

    // SpawnDialog and M1 terminal reappear
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /spawn/i })).not.toBeNull();
    });
    await waitFor(() => {
      const panes = document.querySelectorAll(".terminal-pane");
      expect(panes.length).toBe(1);
    });
  });

  it("mints a fresh output channel on each spawn (no cross-session bleed)", async () => {
    render(<App />);

    // First spawn
    await act(async () => {
      screen.getByRole("button", { name: /spawn/i }).click();
    });
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /spawn/i })).toBeNull(),
    );

    const firstCall = invokeMock.mock.calls.find((c) => c[0] === "spawn_session");
    const firstChannel = firstCall?.[1].onOutput;

    // Close session to get back to SpawnDialog
    fireBackendEvent("session_closed", "session-1");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /spawn/i })).not.toBeNull(),
    );

    invokeMock.mockResolvedValue("session-2");
    invokeMock.mockClear();

    // Second spawn
    await act(async () => {
      screen.getByRole("button", { name: /spawn/i }).click();
    });
    await waitFor(() => {
      expect(invokeMock.mock.calls.find((c) => c[0] === "spawn_session")).toBeDefined();
    });

    const secondCall = invokeMock.mock.calls.find((c) => c[0] === "spawn_session");
    const secondChannel = secondCall?.[1].onOutput;

    // Each spawn must use a different channel instance
    expect(firstChannel).not.toBe(secondChannel);
  });

});
