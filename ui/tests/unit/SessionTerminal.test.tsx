import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

// --- Tauri surface -----------------------------------------------------------
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

// --- xterm surface (minimal — we only check the component renders a container) ---
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
import { SessionTerminal } from "../../src/components/SessionTerminal";
import { createBufferedOutputChannel } from "../../src/hooks/useSessionTerminal";

describe("SessionTerminal component", () => {
  it("renders a terminal-pane container element", () => {
    const buffered = createBufferedOutputChannel();
    const { container } = render(
      <SessionTerminal sessionId="session-1" outputChannel={buffered} />,
    );
    expect(container.querySelector(".terminal-pane")).not.toBeNull();
  });

  it("accepts different session ids without throwing", () => {
    const buffered = createBufferedOutputChannel();
    expect(() =>
      render(
        <SessionTerminal sessionId="session-abc" outputChannel={buffered} />,
      ),
    ).not.toThrow();
  });
});
