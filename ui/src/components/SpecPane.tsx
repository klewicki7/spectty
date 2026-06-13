import { useEffect, useRef, useState } from "react";

import {
  approvePrompt,
  getSpec,
  listenSpecUpdated,
  type AgentTier,
  type ApprovalDecision,
  type SessionId,
  type SpecContract,
  type TaskState,
} from "../session/ipc";

/** Props for the Spec pane: the session it tracks + how cooperative its agent is. */
export interface SpecPaneProps {
  sessionId: SessionId;
  /**
   * `Cooperative` agents push a structured `SpecContract` (live checklist + plan
   * approval); `Generic` agents have no structured spec, so the pane degrades to a
   * coarse scraped badge (spec-pane-ui scenario 2).
   */
  tier: AgentTier;
}

/**
 * The action id the plan-approval gate addresses. The cooperative agent's
 * `spectty_approval` for the PLAN uses this stable id (the per-edit prompts use
 * their own ids); the SpecPane gate resolves exactly the plan approval.
 */
const PLAN_ACTION_ID = "plan";

/**
 * Runtime guard for an on-mount hydrate payload. `getSpec` resolves to a
 * `SpecContract | null`, but a corrupt/unexpected blob degrades to "no plan"
 * rather than crashing the render (defensive — the backend already drops corrupt
 * blobs, this is belt-and-suspenders for the UI).
 */
function isSpecContract(value: unknown): value is SpecContract {
  return (
    typeof value === "object" &&
    value !== null &&
    Array.isArray((value as { tasks?: unknown }).tasks)
  );
}

// Human-readable label for each `TaskState`. The variant string doubles as the
// `data-task-status` attribute + CSS modifier key (mirrors PaneHeader.statusBadge).
const TASK_STATE_LABELS: Record<TaskState, string> = {
  pending: "Pending",
  in_progress: "In progress",
  done: "Done",
  skipped: "Skipped",
};

/**
 * The Living Spec pane (D29/D32/D33).
 *
 * For a cooperative session it renders the `SpecContract` as a live checklist that
 * updates on each `spec_updated` event WITHOUT a manual refresh (the backend SpecBus
 * poll loop is authoritative), shows each task's `TaskState`, and presents the
 * plan-approval gate (Approve / Reject / Adjust → `approve_prompt`) while approval is
 * `Pending`. The gate disappears once approval resolves. On mount it hydrates once via
 * `getSpec` so a restored session shows its prior plan immediately (exit criterion 6).
 *
 * For a generic session there is no structured spec, so the pane shows a coarse
 * "PTY-scraped progress" badge instead of a precise checklist (graceful degradation).
 *
 * React 19 / React Compiler — named imports, no manual `useMemo`/`useCallback`. The
 * pane NEVER computes spec state locally: it adopts whatever the backend delivers and
 * ignores events for any other session (backend authoritative).
 */
export function SpecPane({ sessionId, tier }: SpecPaneProps) {
  const [spec, setSpec] = useState<SpecContract | null>(null);
  const isGeneric = tier === "Generic";

  // Mirror the live session id in a ref so the listener (registered once) always
  // filters against the current session without re-subscribing.
  const sessionIdRef = useRef(sessionId);
  sessionIdRef.current = sessionId;

  useEffect(() => {
    if (isGeneric) {
      return;
    }
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    // On-mount hydrate: restore the prior plan immediately (no 2s blank window).
    void getSpec(sessionId).then((stored) => {
      if (!cancelled && isSpecContract(stored)) {
        setSpec((prev) => prev ?? stored);
      }
    });

    void listenSpecUpdated((payload) => {
      if (payload.session_id === sessionIdRef.current) {
        setSpec(payload.spec);
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [sessionId, isGeneric]);

  if (isGeneric) {
    return (
      <section className="spec-pane spec-pane--generic" aria-label="Spec">
        <p className="spec-pane__generic-badge" data-testid="spec-generic-badge">
          Generic agent — progress is scraped from the terminal (no structured plan).
        </p>
      </section>
    );
  }

  const decide = (decision: ApprovalDecision) => {
    void approvePrompt(sessionIdRef.current, PLAN_ACTION_ID, decision);
  };

  const gatePending = spec !== null && spec.approval === "Pending";

  return (
    <section className="spec-pane" aria-label="Spec">
      {spec === null ? (
        <p className="spec-pane__empty">No plan yet.</p>
      ) : (
        <>
          {spec.intent.length > 0 ? (
            <p className="spec-pane__intent">{spec.intent}</p>
          ) : null}
          <ul className="spec-pane__tasks">
            {spec.tasks.map((task) => (
              <li
                key={task.id}
                className={`spec-task spec-task--${task.status}`}
                data-task-id={task.id}
                data-task-status={task.status}
              >
                <span className="spec-task__title">{task.title}</span>
                <span className="spec-task__state">
                  {TASK_STATE_LABELS[task.status]}
                </span>
              </li>
            ))}
          </ul>
          {gatePending ? (
            <div className="spec-pane__gate" role="group" aria-label="Plan approval">
              <p className="spec-pane__gate-prompt">Approve the plan to begin edits.</p>
              <button type="button" onClick={() => decide("approve")}>
                Approve
              </button>
              <button type="button" onClick={() => decide("adjust")}>
                Adjust
              </button>
              <button type="button" onClick={() => decide("reject")}>
                Reject
              </button>
            </div>
          ) : null}
        </>
      )}
    </section>
  );
}
