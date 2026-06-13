import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

// SpecPane / VibeLensPanel each subscribe to their listeners on mount; mock the ipc
// module so TriadLayout renders without touching the real Tauri surface. The terminal
// region is passed in as a child so the layout itself stays presentation-only.
vi.mock("../../src/session/ipc", async () => {
  const actual = await vi.importActual<typeof import("../../src/session/ipc")>(
    "../../src/session/ipc",
  );
  return {
    ...actual,
    listenSpecUpdated: vi.fn().mockResolvedValue(() => {}),
    listenDiffUpdated: vi.fn().mockResolvedValue(() => {}),
    getSpec: vi.fn().mockResolvedValue(null),
    getDiffExplanation: vi.fn().mockResolvedValue(null),
  };
});

import { TriadLayout } from "../../src/components/TriadLayout";

describe("TriadLayout", () => {
  it("renders the spec pane, the terminal region, and the vibelens panel for one session", () => {
    render(
      <TriadLayout sessionId="session-1" tier="Cooperative">
        <div data-testid="terminal-region">terminal</div>
      </TriadLayout>,
    );

    // All three triad regions present simultaneously (roadmap exit criterion 8).
    expect(screen.getByLabelText("Spec")).toBeTruthy();
    expect(screen.getByTestId("terminal-region")).toBeTruthy();
    expect(screen.getByLabelText("VibeLens")).toBeTruthy();
  });

  it("renders the coarse generic badge in the spec region for a generic session", () => {
    render(
      <TriadLayout sessionId="session-1" tier="Generic">
        <div data-testid="terminal-region">terminal</div>
      </TriadLayout>,
    );

    expect(screen.getByTestId("spec-generic-badge")).toBeTruthy();
    expect(screen.getByTestId("terminal-region")).toBeTruthy();
    expect(screen.getByLabelText("VibeLens")).toBeTruthy();
  });
});
