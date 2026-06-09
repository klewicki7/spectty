import { Terminal } from "./components/Terminal";
import { PaneHeader } from "./components/PaneHeader";
import { SpawnDialog } from "./components/SpawnDialog";
import { useSession } from "./hooks/useSession";

/**
 * M2 shell: the M1 live terminal pane gains an agent-session surface — a spawn
 * dialog (pick agent + workspace) and a Pane header showing the live
 * `AgentStatus` badge + session title. `useSession` owns the spawn/close
 * orchestration and the backend status subscription; the header NEVER computes
 * status locally (backend authoritative).
 */
export function App() {
  const { session, status, spawn } = useSession();

  return (
    <main className="app">
      <PaneHeader title={session?.title ?? ""} status={status} />
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
