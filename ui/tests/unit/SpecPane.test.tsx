import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

// `listenSpecUpdated` / `approvePrompt` are the only ipc surfaces SpecPane uses
// (plus `getSpec` for the on-mount hydrate). Mock the module so the component
// drives off a controllable fake listener and we can assert the invoke args.
const listenSpecUpdatedMock = vi.fn();
const getSpecMock = vi.fn();
const approvePromptMock = vi.fn();

vi.mock("../../src/session/ipc", async () => {
  const actual = await vi.importActual<typeof import("../../src/session/ipc")>(
    "../../src/session/ipc",
  );
  return {
    ...actual,
    listenSpecUpdated: (...a: unknown[]) => listenSpecUpdatedMock(...a),
    getSpec: (...a: unknown[]) => getSpecMock(...a),
    approvePrompt: (...a: unknown[]) => approvePromptMock(...a),
  };
});

import { SpecPane } from "../../src/components/SpecPane";
import type { SpecContract } from "../../src/session/ipc";

function contract(overrides: Partial<SpecContract> = {}): SpecContract {
  return {
    intent: "fix the auth bug",
    proposal: "a detailed plan",
    tasks: [
      { id: "t1", title: "write the test", status: "in_progress" },
      { id: "t2", title: "make it pass", status: "pending" },
    ],
    progress: [],
    approval: "Pending",
    steering_notes: [],
    dev_override: false,
    ...overrides,
  };
}

// Capture the handler SpecPane registers so the test can push a `spec_updated`.
function captureSpecHandler(): (payload: {
  session_id: string;
  spec: SpecContract;
}) => void {
  const call = listenSpecUpdatedMock.mock.calls.at(-1);
  return call?.[0] as never;
}

describe("SpecPane live checklist", () => {
  it("renders the live checklist from a spec_updated event without a manual refresh", async () => {
    listenSpecUpdatedMock.mockReset();
    getSpecMock.mockReset();
    getSpecMock.mockResolvedValue(null);
    listenSpecUpdatedMock.mockResolvedValue(() => {});

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);

    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());
    captureSpecHandler()({
      session_id: "session-1",
      spec: contract({
        tasks: [{ id: "t1", title: "write the test", status: "done" }],
      }),
    });

    // The task shows its TaskState live; no refresh button was clicked.
    const item = await screen.findByText("write the test");
    expect(item).toBeTruthy();
    const row = item.closest("[data-task-id='t1']");
    expect(row?.getAttribute("data-task-status")).toBe("done");
  });

  it("ignores spec_updated events for a different session", async () => {
    listenSpecUpdatedMock.mockReset();
    getSpecMock.mockReset();
    getSpecMock.mockResolvedValue(null);
    listenSpecUpdatedMock.mockResolvedValue(() => {});

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());

    captureSpecHandler()({
      session_id: "other-session",
      spec: contract(),
    });

    expect(screen.queryByText("write the test")).toBeNull();
  });

  it("shows a coarse scraped badge for a generic-tier session", async () => {
    listenSpecUpdatedMock.mockReset();
    getSpecMock.mockReset();
    getSpecMock.mockResolvedValue(null);
    listenSpecUpdatedMock.mockResolvedValue(() => {});

    render(<SpecPane sessionId="session-1" tier="Generic" />);

    const badge = await screen.findByTestId("spec-generic-badge");
    expect(badge).toBeTruthy();
    // A generic session has no structured checklist.
    expect(screen.queryByText("write the test")).toBeNull();
  });
});

describe("SpecPane plan-approval gate", () => {
  it("approving the plan invokes approve_prompt with an approve decision", async () => {
    listenSpecUpdatedMock.mockReset();
    getSpecMock.mockReset();
    approvePromptMock.mockReset();
    getSpecMock.mockResolvedValue(null);
    listenSpecUpdatedMock.mockResolvedValue(() => {});
    approvePromptMock.mockResolvedValue(true);

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());

    captureSpecHandler()({
      session_id: "session-1",
      spec: contract({ approval: "Pending" }),
    });

    const approve = await screen.findByRole("button", { name: /approve/i });
    fireEvent.click(approve);

    await waitFor(() =>
      expect(approvePromptMock).toHaveBeenCalledWith(
        "session-1",
        "plan",
        "approve",
      ),
    );
  });

  it("rejecting and adjusting invoke approve_prompt with the matching decision", async () => {
    listenSpecUpdatedMock.mockReset();
    getSpecMock.mockReset();
    approvePromptMock.mockReset();
    getSpecMock.mockResolvedValue(null);
    listenSpecUpdatedMock.mockResolvedValue(() => {});
    approvePromptMock.mockResolvedValue(true);

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());
    captureSpecHandler()({
      session_id: "session-1",
      spec: contract({ approval: "Pending" }),
    });

    fireEvent.click(await screen.findByRole("button", { name: /reject/i }));
    await waitFor(() =>
      expect(approvePromptMock).toHaveBeenCalledWith(
        "session-1",
        "plan",
        "reject",
      ),
    );

    fireEvent.click(await screen.findByRole("button", { name: /adjust/i }));
    await waitFor(() =>
      expect(approvePromptMock).toHaveBeenCalledWith(
        "session-1",
        "plan",
        "adjust",
      ),
    );
  });

  it("hides the gate once approval is resolved", async () => {
    listenSpecUpdatedMock.mockReset();
    getSpecMock.mockReset();
    getSpecMock.mockResolvedValue(null);
    listenSpecUpdatedMock.mockResolvedValue(() => {});

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());

    // Pending → gate visible.
    captureSpecHandler()({
      session_id: "session-1",
      spec: contract({ approval: "Pending" }),
    });
    expect(
      await screen.findByRole("button", { name: /approve/i }),
    ).toBeTruthy();

    // Approved → gate gone.
    captureSpecHandler()({
      session_id: "session-1",
      spec: contract({ approval: "Approved" }),
    });
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /approve/i })).toBeNull(),
    );
  });
});

describe("SpecPane mount hydrate", () => {
  it("hydrates from getSpec on mount when a spec is already stored", async () => {
    listenSpecUpdatedMock.mockReset();
    getSpecMock.mockReset();
    listenSpecUpdatedMock.mockResolvedValue(() => {});
    getSpecMock.mockResolvedValue(contract());

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);

    expect(await screen.findByText("write the test")).toBeTruthy();
    expect(getSpecMock).toHaveBeenCalledWith("session-1");
  });
});
