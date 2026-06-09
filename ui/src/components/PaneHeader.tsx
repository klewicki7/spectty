import type { AgentStatus } from "../session/ipc";

/** A label + style variant for one `AgentStatus` (or the no-session state). */
export interface StatusBadge {
  label: string;
  variant: string;
}

// Single source of truth for the status → badge mapping. The variant doubles as
// the `data-status` attribute and the CSS modifier key in `styles.css`:
//   Starting=grey, Idle=blue, Running=green(pulse), AwaitingInput=amber(pulse),
//   Completed=grey-check, Error=red (design 6.5 / WU-10.7).
const STATUS_BADGES: Record<AgentStatus, StatusBadge> = {
  Starting: { label: "Starting", variant: "starting" },
  Idle: { label: "Idle", variant: "idle" },
  Running: { label: "Running", variant: "running" },
  AwaitingInput: { label: "Awaiting input", variant: "awaiting-input" },
  Completed: { label: "Completed", variant: "completed" },
  Error: { label: "Error", variant: "error" },
};

const NO_SESSION_BADGE: StatusBadge = { label: "No session", variant: "none" };

/**
 * Map an `AgentStatus` (or `null` for no active session) to its badge. PURE — the
 * load-bearing mapping the Pane header renders; tested exhaustively in vitest.
 */
export function statusBadge(status: AgentStatus | null): StatusBadge {
  if (status === null) {
    return NO_SESSION_BADGE;
  }
  return STATUS_BADGES[status];
}

/** Props for the Pane header: the session title + its current status. */
export interface PaneHeaderProps {
  title: string;
  status: AgentStatus | null;
  /**
   * End the active session. When provided, the header renders a Close button
   * that invokes this callback; the header NEVER calls IPC itself (purely
   * presentational — the parent owns `useSession.close`).
   */
  onClose?: () => void;
}

/**
 * The Pane header: session title + an `AgentStatus` badge + an optional Close
 * button. The status is supplied by the parent (which owns `useSession`); the
 * header NEVER computes it locally (backend authoritative). When there is no
 * session, the title/badge fall back to a neutral "No session" state. The Close
 * button appears only when an `onClose` handler is supplied (i.e. a session is
 * active).
 */
export function PaneHeader({ title, status, onClose }: PaneHeaderProps) {
  const badge = statusBadge(status);
  const displayTitle = title.length > 0 ? title : NO_SESSION_BADGE.label;

  return (
    <header className="pane-header">
      <span className="pane-header__title">{displayTitle}</span>
      <span
        className={`status-badge status-badge--${badge.variant}`}
        data-status={badge.variant}
      >
        {badge.label}
      </span>
      {onClose ? (
        <button
          type="button"
          className="pane-header__close"
          onClick={onClose}
        >
          Close
        </button>
      ) : null}
    </header>
  );
}
