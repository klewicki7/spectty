/**
 * Spawn error banner rendering — isolated from the live Tauri IPC mocks in
 * App.test.tsx by mocking `useSession` directly.  The hook's error-capture
 * logic is covered in useSession.test.ts.  Here we only verify that App
 * renders / hides the banner based on the `error` field exposed by useSession.
 */
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// --- Tauri and xterm stubs (required by imports in App's tree) ---------------
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
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
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit = vi.fn(); } }));
vi.mock("@xterm/addon-clipboard", () => ({ ClipboardAddon: class {} }));
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

class FakeResizeObserver {
  observe() {}
  disconnect() {}
}
vi.stubGlobal("ResizeObserver", FakeResizeObserver);

// --- Mock useSession so we can inject error state without live IPC ----------
vi.mock("../../src/hooks/useSession", () => ({
  useSession: vi.fn(),
}));

import { useSession } from "../../src/hooks/useSession";
import { App } from "../../src/App";

const useSessionMock = useSession as ReturnType<typeof vi.fn>;

const baseUseSession = {
  session: null,
  status: null,
  error: null,
  spawn: vi.fn().mockResolvedValue(undefined),
  close: vi.fn().mockResolvedValue(undefined),
};

describe("App — spawn error banner", () => {
  beforeEach(() => {
    useSessionMock.mockReturnValue({ ...baseUseSession });
  });

  it("renders a spawn error banner when useSession.error is set", () => {
    useSessionMock.mockReturnValue({
      ...baseUseSession,
      error: "hooks provisioning inject failed: provisioning io error: No such file or directory (os error 2)",
    });

    render(<App />);

    const alert = screen.getByRole("alert");
    expect(alert).not.toBeNull();
    expect(alert.textContent).toMatch(/Spawn failed/);
    expect(alert.textContent).toMatch(/provisioning io error/);

    // SpawnDialog stays visible so the user can retry
    expect(screen.getByRole("button", { name: /spawn/i })).not.toBeNull();
  });

  it("does not render an error banner when error is null", () => {
    render(<App />);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
