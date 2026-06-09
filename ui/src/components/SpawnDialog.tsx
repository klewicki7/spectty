import { useState } from "react";

import type { AgentKind, AgentSpec, AgentTier } from "../session/ipc";

// The two M2 first-class agent choices. Claude Code is Cooperative (speaks the
// protocol); Generic is a plain command driven by output heuristics, so it
// carries a free-text command. A text field for the workspace path avoids a
// dialog plugin/permission in M2 (design 6.5).
interface AgentChoice {
  kind: AgentKind;
  label: string;
  tier: AgentTier;
}

const AGENT_CHOICES: AgentChoice[] = [
  { kind: "claude-code", label: "Claude Code", tier: "Cooperative" },
  { kind: "generic", label: "Generic", tier: "Generic" },
];

/** Props for the spawn dialog: the spawn action plus a disabled flag. */
export interface SpawnDialogProps {
  onSpawn: (agent: AgentSpec, workspacePath: string, title: string) => void;
  disabled?: boolean;
}

/**
 * Minimal spawn form: pick an agent (Claude Code | Generic + free-text command),
 * a workspace path, and a title → invoke `spawnSession` via the parent's
 * `onSpawn`. React 19 named imports; no manual memoization.
 */
export function SpawnDialog({ onSpawn, disabled = false }: SpawnDialogProps) {
  const [kind, setKind] = useState<AgentKind>(AGENT_CHOICES[0].kind);
  const [command, setCommand] = useState("");
  const [workspacePath, setWorkspacePath] = useState("");
  const [title, setTitle] = useState("");

  const choice =
    AGENT_CHOICES.find((c) => c.kind === kind) ?? AGENT_CHOICES[0];
  const isGeneric = choice.tier === "Generic";

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const parsedCommand =
      isGeneric && command.trim().length > 0
        ? command.trim().split(/\s+/)
        : null;
    const agent: AgentSpec = {
      kind,
      command: parsedCommand,
      tier: choice.tier,
    };
    onSpawn(agent, workspacePath, title);
  };

  return (
    <form className="spawn-dialog" onSubmit={submit}>
      <fieldset className="spawn-dialog__agents">
        <legend>Agent</legend>
        {AGENT_CHOICES.map((c) => (
          <label key={c.kind} className="spawn-dialog__agent">
            <input
              type="radio"
              name="agent"
              value={c.kind}
              checked={kind === c.kind}
              onChange={() => setKind(c.kind)}
            />
            {c.label}
          </label>
        ))}
      </fieldset>

      {isGeneric ? (
        <label className="spawn-dialog__field">
          Command
          <input
            type="text"
            value={command}
            placeholder="e.g. bash -l"
            onChange={(e) => setCommand(e.target.value)}
          />
        </label>
      ) : null}

      <label className="spawn-dialog__field">
        Workspace
        <input
          type="text"
          value={workspacePath}
          placeholder="/path/to/repo"
          onChange={(e) => setWorkspacePath(e.target.value)}
        />
      </label>

      <label className="spawn-dialog__field">
        Title
        <input
          type="text"
          value={title}
          placeholder="Session title"
          onChange={(e) => setTitle(e.target.value)}
        />
      </label>

      <button type="submit" disabled={disabled}>
        Spawn
      </button>
    </form>
  );
}
