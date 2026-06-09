import { useRef } from "react";

import { Terminal } from "./components/Terminal";
import { SessionTerminal } from "./components/SessionTerminal";
import { PaneHeader } from "./components/PaneHeader";
import { SpawnDialog } from "./components/SpawnDialog";
import { useSession } from "./hooks/useSession";
import { createBufferedOutputChannel } from "./hooks/useSessionTerminal";

/**
 * M2 shell: the M1 live terminal pane gains an agent-session surface — a spawn
 * dialog (pick agent + workspace) and a Pane header showing the live
 * `AgentStatus` badge + session title + a Close button. `useSession` owns the
 * spawn/close orchestration and the backend status subscription; the header
 * NEVER computes status or calls IPC locally (backend authoritative). Closing a
 * session clears it (session → null), which re-shows the spawn dialog and
 * the M1 shell terminal.
 *
 * Output channel lifecycle: `outputChannelRef` holds a single buffered channel
 * created once. `createBufferedOutputChannel` builds a `Channel<unknown>` whose
 * `onmessage` queues decoded bytes until `drainTo` is called; `SessionTerminal`
 * calls `drainTo` synchronously inside its `useEffect` after xterm opens. This
 * ensures zero bytes are lost between `spawnSession` returning and xterm mounting:
 * any bytes the backend streams before the effect runs are queued and flushed
 * during mount. A new channel is minted on every spawn so there is no cross-
 * session bleed.
 */
export function App() {
  const { session, status, spawn, close } = useSession();

  // A stable ref holds the current session's buffered output channel. A new
  // channel is created each time the user spawns so old messages from a
  // previous session never bleed into the next one.
  const outputChannelRef = useRef(createBufferedOutputChannel());

  return (
    <main className="app">
      <PaneHeader
        title={session?.title ?? ""}
        status={status}
        onClose={session !== null ? () => void close() : undefined}
      />
      {session === null ? (
        <>
          <SpawnDialog
            onSpawn={(agent, workspacePath, title) => {
              // Mint a fresh channel for this spawn so old session output
              // cannot bleed into the new session.
              outputChannelRef.current = createBufferedOutputChannel();
              void spawn(
                agent,
                workspacePath,
                title,
                outputChannelRef.current.channel,
              );
            }}
          />
          <Terminal />
        </>
      ) : (
        <SessionTerminal
          sessionId={session.id}
          outputChannel={outputChannelRef.current}
        />
      )}
    </main>
  );
}
