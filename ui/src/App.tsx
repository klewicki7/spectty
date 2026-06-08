import { Terminal } from "./components/Terminal";

/**
 * M1 shell: a single live terminal pane backed by a real PTY on the Rust side.
 * The M0 ping placeholder has been replaced — the terminal IS the app now.
 */
export function App() {
  return (
    <main className="app">
      <Terminal />
    </main>
  );
}
