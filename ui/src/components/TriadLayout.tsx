import type { ReactNode } from "react";

import { SpecPane } from "./SpecPane";
import { VibeLensPanel } from "./VibeLensPanel";
import type { AgentTier, SessionId } from "../session/ipc";

/** Props for the triad layout: the session + its tier + the terminal region. */
export interface TriadLayoutProps {
  sessionId: SessionId;
  tier: AgentTier;
  /**
   * The terminal region (the existing `SessionTerminal`). Passed in as a child so
   * the layout stays purely presentational and does not own the PTY lifecycle.
   */
  children: ReactNode;
}

/**
 * The Triad layout (roadmap exit criterion 8): the Spec pane, the Terminal, and the
 * VibeLens panel rendered SIMULTANEOUSLY for one session. All three regions are always
 * present — the user never navigates away to see one. CSS (`styles.css`) arranges them
 * side-by-side; this component only composes the three regions.
 *
 * Presentational only — `SpecPane` and `VibeLensPanel` own their own backend
 * subscriptions; the terminal is injected as `children`.
 */
export function TriadLayout({ sessionId, tier, children }: TriadLayoutProps) {
  return (
    <div className="triad">
      <div className="triad__spec">
        <SpecPane sessionId={sessionId} tier={tier} />
      </div>
      <div className="triad__terminal">{children}</div>
      <div className="triad__vibelens">
        <VibeLensPanel sessionId={sessionId} />
      </div>
    </div>
  );
}
