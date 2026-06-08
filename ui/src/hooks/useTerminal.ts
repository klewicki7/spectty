import { useEffect, type RefObject } from "react";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { ClipboardAddon } from "@xterm/addon-clipboard";

import {
  createOutputChannel,
  killPty,
  resizePty,
  sendInput,
  spawnPty,
  type PtyId,
} from "../pty/ipc";

/** Configurable scrollback depth retained beyond the viewport (spec: 5000). */
const SCROLLBACK = 5000;

/** The lifecycle event the backend emits when a PTY's child process exits. */
const PTY_EXIT_EVENT = "pty_exit";

/** Payload shape of the `pty_exit` event (mirrors the Rust `PtyExit` struct). */
interface PtyExitPayload {
  id: PtyId;
  code: number | null;
}

/**
 * Mount and drive a live xterm.js terminal bound to a backend PTY.
 *
 * Mirrors `usePingPong`'s shape: a single `useEffect` owns the full imperative
 * lifecycle and returns a cleanup function. React 19 / React Compiler — no
 * manual `useMemo`/`useCallback`, named imports only.
 *
 * On mount it: creates the Terminal, loads the fit + clipboard addons, opens it
 * into `containerRef`, fits to the container, spawns the PTY (streaming output
 * over a `Channel` whose bytes are decoded and written to the terminal), wires
 * keystrokes (`onData` → `send_input`), and re-fits + resizes the PTY whenever
 * the container resizes. On unmount it disconnects the observer, disposes the
 * terminal, and kills the PTY.
 */
export function useTerminal(
  containerRef: RefObject<HTMLDivElement | null>,
): void {
  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }

    // `disposed` guards the async spawn against an unmount that races ahead of
    // it: if the effect is torn down before `pty_spawn` resolves, we kill the
    // PTY as soon as its id arrives so no backend session is leaked.
    let disposed = false;
    let ptyId: PtyId | null = null;

    const term = new Terminal({
      scrollback: SCROLLBACK,
      convertEol: false,
      cursorBlink: true,
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(new ClipboardAddon());

    term.open(container);
    fitAddon.fit();

    // Output: decoded PTY bytes (R1 handled in `createOutputChannel`) → terminal.
    const channel = createOutputChannel((bytes) => {
      term.write(bytes);
    });

    // Input: keystrokes/paste → backend (only once the PTY id is known).
    const dataDisposable = term.onData((data) => {
      if (ptyId === null) {
        return;
      }
      void sendInput(ptyId, new TextEncoder().encode(data));
    });

    // Resize: container change → re-fit → forward new size to the PTY (SIGWINCH).
    const observer = new ResizeObserver(() => {
      fitAddon.fit();
      if (ptyId !== null) {
        void resizePty(ptyId, term.cols, term.rows);
      }
    });
    observer.observe(container);

    // Lifecycle: surface the backend's exit event (low-frequency, via listen).
    const unlistenPromise = listen<PtyExitPayload>(PTY_EXIT_EVENT, () => {
      // M1 keeps the pane mounted; the exit is observed for future status UI.
    });

    void spawnPty(term.cols, term.rows, undefined, channel).then((id) => {
      if (disposed) {
        // Unmounted before spawn resolved — tear down the freshly-born PTY.
        void killPty(id);
        return;
      }
      ptyId = id;
    });

    return () => {
      disposed = true;
      observer.disconnect();
      dataDisposable.dispose();
      unlistenPromise.then((unlisten) => unlisten());
      term.dispose();
      if (ptyId !== null) {
        void killPty(ptyId);
      }
    };
  }, [containerRef]);
}
