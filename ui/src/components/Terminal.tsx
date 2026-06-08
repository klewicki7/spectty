import { useRef } from "react";
import "@xterm/xterm/css/xterm.css";

import { useTerminal } from "../hooks/useTerminal";

/**
 * The live terminal pane: a sized container that `useTerminal` mounts an
 * xterm.js instance into and drives against a backend PTY. The component is
 * intentionally thin — all imperative lifecycle lives in the hook (React 19,
 * no manual memoization, named imports).
 */
export function Terminal() {
  const containerRef = useRef<HTMLDivElement>(null);
  useTerminal(containerRef);

  return <div ref={containerRef} className="terminal-pane" />;
}
