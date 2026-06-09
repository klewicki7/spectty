import { useEffect, type RefObject } from "react";
import { Channel } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { ClipboardAddon } from "@xterm/addon-clipboard";

import { decodeChannelBytes, resizePty, sendInput } from "../pty/ipc";
import type { SessionId } from "../session/ipc";

/** Configurable scrollback depth (mirrors useTerminal). */
const SCROLLBACK = 5000;

/**
 * A buffered output channel factory.
 *
 * The channel created here is handed to `spawnSession` (before the xterm
 * Terminal exists). Any bytes the backend streams before the terminal mounts
 * are decoded and queued in `buffer`. Once `drainTo(sink)` is called, the
 * queue is flushed into `sink` in arrival order, then future messages bypass
 * the buffer and go directly to `sink`. No bytes are ever dropped.
 *
 * This is exported so tests can exercise the buffering contract in isolation.
 */
export interface BufferedOutputChannel {
  channel: Channel<unknown>;
  /** Flush buffered bytes into `sink` and wire future bytes directly to it. */
  drainTo: (sink: (bytes: Uint8Array) => void) => void;
}

export function createBufferedOutputChannel(): BufferedOutputChannel {
  const buffer: Uint8Array[] = [];
  let sink: ((bytes: Uint8Array) => void) | null = null;

  const channel = new Channel<unknown>();
  channel.onmessage = (message: unknown) => {
    const bytes = decodeChannelBytes(message);
    if (sink !== null) {
      sink(bytes);
    } else {
      buffer.push(bytes);
    }
  };

  const drainTo = (newSink: (bytes: Uint8Array) => void): void => {
    sink = newSink;
    for (const chunk of buffer) {
      newSink(chunk);
    }
    buffer.length = 0;
  };

  return { channel, drainTo };
}

/**
 * Mount and drive an xterm.js terminal wired to an EXISTING backend session.
 *
 * Unlike `useTerminal`, this hook does NOT spawn a PTY — the session (and its
 * PTY) already exist. It only:
 *   - Creates the Terminal + FitAddon + ClipboardAddon and opens into the container.
 *   - Calls `drainTo(term.write)` to flush buffered bytes and wire live output.
 *   - Routes keystrokes via `sendInput(sessionId, …)`.
 *   - Observes container size changes and calls `resizePty(sessionId, …)`.
 *   - On unmount: disposes observer/term/onData — does NOT kill the PTY (session
 *     teardown is owned by `close_session` via `useSession.close`).
 *
 * React 19 / React Compiler — no manual `useMemo`/`useCallback`, named imports.
 *
 * @param containerRef  A ref to the DOM node xterm should render into.
 * @param sessionId     The M2 session id (== PtyId in PtyRegistry, design D13).
 * @param outputChannel The buffered channel created in App and handed to
 *                      `spawnSession`. Must be the same object whose `drainTo`
 *                      will flush early bytes.
 */
export function useSessionTerminal(
  containerRef: RefObject<HTMLDivElement | null>,
  sessionId: SessionId,
  outputChannel: BufferedOutputChannel,
): void {
  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }

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

    // Flush any bytes buffered before this mount and wire live output directly
    // to term.write. From this point forward, channel messages arrive here
    // synchronously — nothing is lost.
    outputChannel.drainTo((bytes) => {
      term.write(bytes);
    });

    // Input: keystrokes/paste → backend PTY (same PTY id as session id, D13).
    const dataDisposable = term.onData((data) => {
      void sendInput(sessionId, new TextEncoder().encode(data));
    });

    // Resize: container change → re-fit → forward new size to the PTY.
    const observer = new ResizeObserver(() => {
      fitAddon.fit();
      void resizePty(sessionId, term.cols, term.rows);
    });
    observer.observe(container);

    return () => {
      observer.disconnect();
      dataDisposable.dispose();
      term.dispose();
      // Intentionally NOT calling killPty/closeSession here.
      // Session teardown is owned by the Close button → useSession.close().
    };
  }, [containerRef, sessionId, outputChannel]);
}
