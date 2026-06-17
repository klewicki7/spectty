import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

// `listenSpecUpdated` / `approvePrompt` are the ipc surfaces SpecPane drives the gate
// off (plus `getSpec` for the on-mount hydrate, `getApproval` to resolve the REAL
// pending action_id, and `listenStatusChanged` to flip the gate enabled when the
// agent's approval request lands). Mock the module so the component runs against
// controllable fakes and we can assert the invoke args.
const listenSpecUpdatedMock = vi.fn();
const listenStatusChangedMock = vi.fn();
const getSpecMock = vi.fn();
const getApprovalMock = vi.fn();
const approvePromptMock = vi.fn();

vi.mock("../../src/session/ipc", async () => {
  const actual = await vi.importActual<typeof import("../../src/session/ipc")>(
    "../../src/session/ipc",
  );
  return {
    ...actual,
    listenSpecUpdated: (...a: unknown[]) => listenSpecUpdatedMock(...a),
    listenStatusChanged: (...a: unknown[]) => listenStatusChangedMock(...a),
    getSpec: (...a: unknown[]) => getSpecMock(...a),
    getApproval: (...a: unknown[]) => getApprovalMock(...a),
    approvePrompt: (...a: unknown[]) => approvePromptMock(...a),
  };
});

import { SpecPane } from "../../src/components/SpecPane";
import type { ApprovalRequest, SpecContract } from "../../src/session/ipc";

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

// Capture the handler SpecPane registers for `status_changed` (drives the gate's
// enabled state when the agent's pending approval request lands).
function captureStatusHandler(): (payload: {
  session_id: string;
  status: string;
  quick_actions: unknown[];
}) => void {
  const call = listenStatusChangedMock.mock.calls.at(-1);
  return call?.[0] as never;
}

// Reset every mock to a benign default before each scenario.
function resetMocks() {
  listenSpecUpdatedMock.mockReset();
  listenStatusChangedMock.mockReset();
  getSpecMock.mockReset();
  getApprovalMock.mockReset();
  approvePromptMock.mockReset();
  getSpecMock.mockResolvedValue(null);
  getApprovalMock.mockResolvedValue(null);
  listenSpecUpdatedMock.mockResolvedValue(() => {});
  listenStatusChangedMock.mockResolvedValue(() => {});
  approvePromptMock.mockResolvedValue(true);
}

describe("SpecPane live checklist", () => {
  it("renders the live checklist from a spec_updated event without a manual refresh", async () => {
    resetMocks();

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);

    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());
    await act(async () => {
      captureSpecHandler()({
        session_id: "session-1",
        spec: contract({
          tasks: [{ id: "t1", title: "write the test", status: "done" }],
        }),
      });
    });

    // The task shows its TaskState live; no refresh button was clicked.
    const item = await screen.findByText("write the test");
    expect(item).toBeTruthy();
    const row = item.closest("[data-task-id='t1']");
    expect(row?.getAttribute("data-task-status")).toBe("done");
  });

  it("ignores spec_updated events for a different session", async () => {
    resetMocks();

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());

    captureSpecHandler()({
      session_id: "other-session",
      spec: contract(),
    });

    expect(screen.queryByText("write the test")).toBeNull();
  });

  it("shows a coarse scraped badge for a generic-tier session", async () => {
    resetMocks();

    render(<SpecPane sessionId="session-1" tier="Generic" />);

    const badge = await screen.findByTestId("spec-generic-badge");
    expect(badge).toBeTruthy();
    // A generic session has no structured checklist.
    expect(screen.queryByText("write the test")).toBeNull();
  });
});

describe("SpecPane plan-approval gate", () => {
  // Finding 1 (MAJOR) cross-seam pin: the gate MUST resolve with whatever pending
  // `action_id` the agent supplied — NEVER a hardcoded constant. We feed the EXACT
  // Rust-serialized pending `ApprovalRequest` JSON literal through getApproval's mock
  // (verbatim shape of `src-tauri/src/commands/spec.rs` → `ApprovalRequest`, the same
  // document the MCP effect writes), and assert approve_prompt resolves with THAT id
  // ("plan-1"), proving no hardcoded "plan" survives.
  it("resolves with the agent-supplied action_id from the pending request, not a constant", async () => {
    resetMocks();
    // The literal an agent's spectty_approval produces and the Rust side serializes.
    const pending: ApprovalRequest = JSON.parse(
      JSON.stringify({
        action_id: "plan-1",
        description: "approve the migration plan",
        risk_level: "high",
        options: ["approve", "reject", "adjust"],
        resolution: null,
      }),
    );
    getApprovalMock.mockResolvedValue(pending);

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());
    await act(async () => {
      captureSpecHandler()({
        session_id: "session-1",
        spec: contract({ approval: "Pending" }),
      });
    });

    // The buttons are enabled once the pending request is present.
    const approve = await screen.findByRole("button", { name: /approve/i });
    await waitFor(() =>
      expect((approve as HTMLButtonElement).disabled).toBe(false),
    );
    fireEvent.click(approve);

    await waitFor(() =>
      expect(approvePromptMock).toHaveBeenCalledWith(
        "session-1",
        "plan-1",
        "approve",
      ),
    );
    // The hardcoded literal must never be used.
    expect(approvePromptMock).not.toHaveBeenCalledWith(
      "session-1",
      "plan",
      expect.anything(),
    );
  });

  it("rejecting and adjusting resolve with the real pending action_id", async () => {
    resetMocks();
    getApprovalMock.mockResolvedValue({
      action_id: "plan-7",
      description: "x",
      options: ["approve", "reject", "adjust"],
      resolution: null,
    } satisfies ApprovalRequest);

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());
    await act(async () => {
      captureSpecHandler()({
        session_id: "session-1",
        spec: contract({ approval: "Pending" }),
      });
    });

    const reject = await screen.findByRole("button", { name: /reject/i });
    await waitFor(() =>
      expect((reject as HTMLButtonElement).disabled).toBe(false),
    );
    fireEvent.click(reject);
    await waitFor(() =>
      expect(approvePromptMock).toHaveBeenCalledWith(
        "session-1",
        "plan-7",
        "reject",
      ),
    );

    fireEvent.click(await screen.findByRole("button", { name: /adjust/i }));
    await waitFor(() =>
      expect(approvePromptMock).toHaveBeenCalledWith(
        "session-1",
        "plan-7",
        "adjust",
      ),
    );
  });

  it("disables the gate buttons while approval is Pending but no request exists yet", async () => {
    resetMocks();
    // Spec says Pending, but the agent has NOT written its approval request yet.
    getApprovalMock.mockResolvedValue(null);

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());
    await act(async () => {
      captureSpecHandler()({
        session_id: "session-1",
        spec: contract({ approval: "Pending" }),
      });
    });

    // The gate renders honestly: buttons disabled + a waiting hint. Clicking can NOT
    // silently no-op because there is nothing to click.
    const approve = await screen.findByRole("button", { name: /approve/i });
    await waitFor(() =>
      expect((approve as HTMLButtonElement).disabled).toBe(true),
    );
    expect(screen.getByText(/waiting for the agent/i)).toBeTruthy();
    // No resolution attempt while disabled.
    expect(approvePromptMock).not.toHaveBeenCalled();
  });

  it("flips the gate enabled when the agent's approval request arrives via status_changed", async () => {
    resetMocks();
    // Initially no pending request → disabled.
    getApprovalMock.mockResolvedValue(null);

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());
    await act(async () => {
      captureSpecHandler()({
        session_id: "session-1",
        spec: contract({ approval: "Pending" }),
      });
    });
    const firstApprove = await screen.findByRole("button", { name: /approve/i });
    await waitFor(() =>
      expect((firstApprove as HTMLButtonElement).disabled).toBe(true),
    );

    // The agent now writes its request; the approval poll loop drives AwaitingInput.
    getApprovalMock.mockResolvedValue({
      action_id: "plan-9",
      description: "x",
      options: ["approve"],
      resolution: null,
    } satisfies ApprovalRequest);
    await act(async () => {
      captureStatusHandler()({
        session_id: "session-1",
        status: "AwaitingInput",
        quick_actions: [],
      });
    });

    const approve = await screen.findByRole("button", { name: /approve/i });
    await waitFor(() =>
      expect((approve as HTMLButtonElement).disabled).toBe(false),
    );
    fireEvent.click(approve);
    await waitFor(() =>
      expect(approvePromptMock).toHaveBeenCalledWith(
        "session-1",
        "plan-9",
        "approve",
      ),
    );
  });

  it("hides the gate once approval is resolved", async () => {
    resetMocks();
    getApprovalMock.mockResolvedValue({
      action_id: "plan-1",
      description: "x",
      options: ["approve"],
      resolution: null,
    } satisfies ApprovalRequest);

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);
    await waitFor(() => expect(listenSpecUpdatedMock).toHaveBeenCalled());

    // Pending → gate visible. Wrapped in act() so the dispatch + the async getApproval
    // refetch it triggers both flush before we assert (no act() warning).
    await act(async () => {
      captureSpecHandler()({
        session_id: "session-1",
        spec: contract({ approval: "Pending" }),
      });
    });
    expect(
      await screen.findByRole("button", { name: /approve/i }),
    ).toBeTruthy();

    // Approved → gate gone. Wrapped in act() so the state flush is captured cleanly.
    await act(async () => {
      captureSpecHandler()({
        session_id: "session-1",
        spec: contract({ approval: "Approved" }),
      });
    });
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /approve/i })).toBeNull(),
    );
  });
});

describe("SpecPane mount hydrate", () => {
  it("hydrates from getSpec on mount when a spec is already stored", async () => {
    resetMocks();
    getSpecMock.mockResolvedValue(contract());

    render(<SpecPane sessionId="session-1" tier="Cooperative" />);

    expect(await screen.findByText("write the test")).toBeTruthy();
    expect(getSpecMock).toHaveBeenCalledWith("session-1");
  });
});
