import { usePingPong } from "./hooks/usePingPong";

/**
 * M0 shell: a single button that pings the Rust backend and displays the pong
 * payload once the event arrives. This is the visible proof that the Tauri
 * bridge round-trips end to end.
 */
export function App() {
  const { pong, sendPing } = usePingPong();

  return (
    <main>
      <h1>Spectty</h1>
      <button type="button" onClick={() => void sendPing()}>
        Ping backend
      </button>
      <p>{pong ? `Pong: ${pong}` : "No pong yet — click Ping."}</p>
    </main>
  );
}
