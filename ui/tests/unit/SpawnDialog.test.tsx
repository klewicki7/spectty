import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { SpawnDialog } from "../../src/components/SpawnDialog";
import type { AgentSpec } from "../../src/session/ipc";

describe("SpawnDialog Generic command parsing", () => {
  it("parses the Generic command into a clean string[] with no empty-string elements", () => {
    const onSpawn =
      vi.fn<(agent: AgentSpec, workspacePath: string, title: string) => void>();
    render(<SpawnDialog onSpawn={onSpawn} />);

    // Pick the Generic agent so the free-text Command field appears.
    fireEvent.click(screen.getByLabelText("Generic"));

    // Extra/leading/trailing whitespace must collapse — `split(/\s+/)` on a
    // trimmed string yields no empty elements.
    fireEvent.change(screen.getByLabelText("Command"), {
      target: { value: "  bash   -l   --noprofile  " },
    });

    fireEvent.submit(screen.getByRole("button", { name: /spawn/i }));

    expect(onSpawn).toHaveBeenCalledTimes(1);
    const agent = onSpawn.mock.calls[0][0];
    expect(agent.kind).toBe("generic");
    expect(agent.tier).toBe("Generic");
    expect(agent.command).toEqual(["bash", "-l", "--noprofile"]);
    expect(agent.command).not.toContain("");
  });

  it("sends a null command for the Claude Code (first-class) path", () => {
    const onSpawn =
      vi.fn<(agent: AgentSpec, workspacePath: string, title: string) => void>();
    render(<SpawnDialog onSpawn={onSpawn} />);

    fireEvent.submit(screen.getByRole("button", { name: /spawn/i }));

    expect(onSpawn).toHaveBeenCalledTimes(1);
    const agent = onSpawn.mock.calls[0][0];
    expect(agent.kind).toBe("claude-code");
    expect(agent.tier).toBe("Cooperative");
    expect(agent.command).toBeNull();
  });
});
