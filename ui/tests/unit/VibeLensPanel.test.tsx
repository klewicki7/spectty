import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const listenDiffUpdatedMock = vi.fn();
const getDiffExplanationMock = vi.fn();

vi.mock("../../src/session/ipc", async () => {
  const actual = await vi.importActual<typeof import("../../src/session/ipc")>(
    "../../src/session/ipc",
  );
  return {
    ...actual,
    listenDiffUpdated: (...a: unknown[]) => listenDiffUpdatedMock(...a),
    getDiffExplanation: (...a: unknown[]) => getDiffExplanationMock(...a),
  };
});

import { VibeLensPanel } from "../../src/components/VibeLensPanel";
import type { DiffExplanation } from "../../src/session/ipc";

function captureDiffHandler(): (payload: {
  session_id: string;
  explanation: DiffExplanation;
}) => void {
  return listenDiffUpdatedMock.mock.calls.at(-1)?.[0] as never;
}

const multiFile: DiffExplanation = {
  files: [
    { path: "src/a.rs", rationale: "added a guard clause" },
    { path: "src/b.ts", rationale: "renamed the handler" },
  ],
  summary: "2 files changed",
};

describe("VibeLensPanel rationale rendering", () => {
  it("renders per-file rationale from a diff_updated event", async () => {
    listenDiffUpdatedMock.mockReset();
    getDiffExplanationMock.mockReset();
    getDiffExplanationMock.mockResolvedValue(null);
    listenDiffUpdatedMock.mockResolvedValue(() => {});

    render(<VibeLensPanel sessionId="session-1" />);
    await waitFor(() => expect(listenDiffUpdatedMock).toHaveBeenCalled());

    captureDiffHandler()({ session_id: "session-1", explanation: multiFile });

    expect(await screen.findByText("src/a.rs")).toBeTruthy();
    expect(screen.getByText("added a guard clause")).toBeTruthy();
    expect(screen.getByText("src/b.ts")).toBeTruthy();
    expect(screen.getByText("renamed the handler")).toBeTruthy();
  });

  it("ignores diff_updated events for a different session", async () => {
    listenDiffUpdatedMock.mockReset();
    getDiffExplanationMock.mockReset();
    getDiffExplanationMock.mockResolvedValue(null);
    listenDiffUpdatedMock.mockResolvedValue(() => {});

    render(<VibeLensPanel sessionId="session-1" />);
    await waitFor(() => expect(listenDiffUpdatedMock).toHaveBeenCalled());

    captureDiffHandler()({ session_id: "other", explanation: multiFile });
    expect(screen.queryByText("src/a.rs")).toBeNull();
  });

  it("renders a no-changes state for an empty explanation", async () => {
    listenDiffUpdatedMock.mockReset();
    getDiffExplanationMock.mockReset();
    getDiffExplanationMock.mockResolvedValue(null);
    listenDiffUpdatedMock.mockResolvedValue(() => {});

    render(<VibeLensPanel sessionId="session-1" />);
    await waitFor(() => expect(listenDiffUpdatedMock).toHaveBeenCalled());

    captureDiffHandler()({
      session_id: "session-1",
      explanation: { files: [], summary: "" },
    });

    expect(await screen.findByTestId("vibelens-empty")).toBeTruthy();
  });
});

describe("VibeLensPanel degraded state", () => {
  it("shows an unavailable indicator for a degraded explanation and does not crash", async () => {
    listenDiffUpdatedMock.mockReset();
    getDiffExplanationMock.mockReset();
    getDiffExplanationMock.mockResolvedValue(null);
    listenDiffUpdatedMock.mockResolvedValue(() => {});

    render(<VibeLensPanel sessionId="session-1" />);
    await waitFor(() => expect(listenDiffUpdatedMock).toHaveBeenCalled());

    // A degraded explanation: no per-file rationale but a non-empty summary that
    // signals the explainer was unavailable.
    captureDiffHandler()({
      session_id: "session-1",
      explanation: {
        files: [],
        summary: "VibeLens unavailable — explanation could not be generated",
      },
    });

    const degraded = await screen.findByTestId("vibelens-degraded");
    expect(degraded).toBeTruthy();
    expect(degraded.textContent).toContain("unavailable");
  });
});

describe("VibeLensPanel manual refresh", () => {
  it("manual refresh re-reads the explanation for the session", async () => {
    listenDiffUpdatedMock.mockReset();
    getDiffExplanationMock.mockReset();
    listenDiffUpdatedMock.mockResolvedValue(() => {});
    getDiffExplanationMock.mockResolvedValue(multiFile);

    render(<VibeLensPanel sessionId="session-1" />);
    await waitFor(() => expect(getDiffExplanationMock).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: /refresh/i }));

    await waitFor(() =>
      expect(getDiffExplanationMock).toHaveBeenCalledTimes(2),
    );
    expect(getDiffExplanationMock).toHaveBeenLastCalledWith("session-1");
    expect(await screen.findByText("src/a.rs")).toBeTruthy();
  });
});
