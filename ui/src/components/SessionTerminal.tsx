import { useRef } from "react";
import "@xterm/xterm/css/xterm.css";

import {
  useSessionTerminal,
  type BufferedOutputChannel,
} from "../hooks/useSessionTerminal";
import type { SessionId } from "../session/ipc";

interface SessionTerminalProps {
  sessionId: SessionId;
  outputChannel: BufferedOutputChannel;
}

/**
 * Terminal pane wired to an existing M2 agent session.
 *
 * Thin wrapper — all imperative lifecycle lives in `useSessionTerminal`
 * (React 19, no manual memoization, named imports). Unlike the M1 `Terminal`,
 * this component does NOT spawn a PTY; the session and its PTY already exist.
 * It only mounts xterm into the container, drains buffered output, and wires
 * input + resize through the session's PtyRegistry entry (D13).
 */
export function SessionTerminal({ sessionId, outputChannel }: SessionTerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  useSessionTerminal(containerRef, sessionId, outputChannel);

  return <div ref={containerRef} className="terminal-pane" />;
}
