import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PaneHeader, statusBadge } from "../../src/components/PaneHeader";
import type { AgentStatus } from "../../src/session/ipc";

describe("statusBadge mapping", () => {
  const cases: Array<[AgentStatus | null, string, string]> = [
    ["Starting", "Starting", "starting"],
    ["Idle", "Idle", "idle"],
    ["Running", "Running", "running"],
    ["AwaitingInput", "Awaiting input", "awaiting-input"],
    ["Completed", "Completed", "completed"],
    ["Error", "Error", "error"],
    [null, "No session", "none"],
  ];

  it.each(cases)(
    "maps status %s to label %s and variant %s",
    (status, label, variant) => {
      const badge = statusBadge(status);
      expect(badge.label).toBe(label);
      expect(badge.variant).toBe(variant);
    },
  );

  it("covers every AgentStatus variant", () => {
    const statuses: AgentStatus[] = [
      "Starting",
      "Idle",
      "Running",
      "AwaitingInput",
      "Completed",
      "Error",
    ];
    for (const s of statuses) {
      expect(statusBadge(s).label.length).toBeGreaterThan(0);
    }
  });
});

describe("PaneHeader component", () => {
  it("renders the session title", () => {
    render(<PaneHeader title="My Agent" status="Running" />);
    expect(screen.getByText("My Agent")).toBeTruthy();
  });

  it("renders the status badge label and a status-keyed data attribute", () => {
    render(<PaneHeader title="My Agent" status="AwaitingInput" />);

    const badge = screen.getByText("Awaiting input");
    expect(badge).toBeTruthy();
    expect(badge.getAttribute("data-status")).toBe("awaiting-input");
  });

  it("falls back to a placeholder title when none is given", () => {
    const { container } = render(<PaneHeader title="" status={null} />);
    const titleEl = container.querySelector(".pane-header__title");
    expect(titleEl?.textContent).toBe("No session");
  });

  it("renders a Close button and calls onClose when clicked", () => {
    const onClose = vi.fn();
    render(<PaneHeader title="My Agent" status="Running" onClose={onClose} />);

    const closeButton = screen.getByRole("button", { name: /close/i });
    fireEvent.click(closeButton);

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not render a Close button when onClose is not provided", () => {
    render(<PaneHeader title="My Agent" status="Running" />);
    expect(screen.queryByRole("button", { name: /close/i })).toBeNull();
  });
});
