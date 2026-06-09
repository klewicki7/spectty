import { Terminal } from "./components/Terminal";
import { PaneHeader } from "./components/PaneHeader";
import { SpawnDialog } from "./components/SpawnDialog";
import { useSession } from "./hooks/useSession";

/**
 * M2 shell: the M1 live terminal pane gains an agent-session surface — a spawn
 * dialog (pick agent + workspace) and a Pane header showing the live
 * `AgentStatus` badge + session title + a Close button. `useSession` owns the
 * spawn/close orchestration and the backend status subscription; the header
 * NEVER computes status or calls IPC locally (backend authoritative). Closing a
 * session clears it (session → null), which re-shows the spawn dialog.
 */
export function App() {
  const { session, status, spawn, close } = useSession();

  return (
    <main className="app">
      <PaneHeader
        title={session?.title ?? ""}
        status={status}
        onClose={session !== null ? () => void close() : undefined}
      />
      {session === null ? (
        <SpawnDialog
          onSpawn={(agent, workspacePath, title) =>
            void spawn(agent, workspacePath, title)
          }
        />
      ) : null}
      <Terminal />
    </main>
  );
}
