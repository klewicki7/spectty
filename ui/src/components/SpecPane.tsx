import { useEffect, useRef, useState } from "react";

import {
  approvePrompt,
  getApproval,
  getSpec,
  listenSpecUpdated,
  listenStatusChanged,
  type AgentTier,
  type ApprovalDecision,
  type ApprovalRequest,
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
 * The gate resolves through the REAL pending `(session_id, action_id)` fetched via
 * `getApproval` — there is NO hardcoded `action_id` contract with the agent (the agent
 * supplies a free-form id, D31). While the contract says `approval === "Pending"` but no
 * pending `ApprovalRequest` is stored yet, the gate renders its buttons DISABLED with a
 * "waiting for the agent's approval request" hint (an honest distinct state, never a
 * click that silently no-ops). The pending request is re-fetched on mount, whenever the
 * spec transitions into `Pending`, and on `status_changed(AwaitingInput)` (the same event
 * the backend approval poll loop already drives — the cheapest correct trigger to flip the
 * gate enabled, no new event needed).
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
  // The CURRENT pending approval request, or null when none is stored. Its `action_id`
  // is what the gate resolves through — never a hardcoded constant.
  const [approval, setApproval] = useState<ApprovalRequest | null>(null);
  const isGeneric = tier === "Generic";

  // Mirror the live session id in a ref so the listener (registered once) always
  // filters against the current session without re-subscribing.
  const sessionIdRef = useRef(sessionId);
  sessionIdRef.current = sessionId;

  useEffect(() => {
    if (isGeneric) {
      return;
    }
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    // Fetch the current pending request for this session; ignore a stale resolution.
    const refetchApproval = () => {
      void getApproval(sessionIdRef.current).then((req) => {
        if (!cancelled) {
          setApproval(req);
        }
      });
    };

    // On-mount hydrate: restore the prior plan + any pending approval immediately.
    void getSpec(sessionId).then((stored) => {
      if (!cancelled && isSpecContract(stored)) {
        setSpec((prev) => prev ?? stored);
      }
    });
    refetchApproval();

    void listenSpecUpdated((payload) => {
      if (payload.session_id === sessionIdRef.current) {
        setSpec(payload.spec);
        // A spec that just entered the approval gate is the cheapest signal to look
        // for the agent's pending request without adding a new event.
        if (payload.spec.approval === "Pending") {
          refetchApproval();
        }
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisteners.push(fn);
      }
    });

    // The backend approval poll loop surfaces a pending request as
    // status_changed(AwaitingInput). Re-fetching on that event is the cheapest correct
    // way to flip the gate from disabled (waiting) to enabled once the agent's request
    // actually lands — no new event needed.
    void listenStatusChanged((payload) => {
      if (
        payload.session_id === sessionIdRef.current &&
        payload.status === "AwaitingInput"
      ) {
        refetchApproval();
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisteners.push(fn);
      }
    });

    return () => {
      cancelled = true;
      for (const fn of unlisteners) {
        fn();
      }
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

  // The pending request whose action_id the gate resolves through. Null (or already
  // resolved) means the gate has nothing live to resolve → disabled.
  const pendingRequest =
    approval !== null && (approval.resolution ?? null) === null ? approval : null;

  const decide = (decision: ApprovalDecision) => {
    // NEVER resolve without a real pending request: no action_id, nothing to resolve.
    if (pendingRequest === null) {
      return;
    }
    void approvePrompt(
      sessionIdRef.current,
      pendingRequest.action_id,
      decision,
    );
  };

  const gateVisible = spec !== null && spec.approval === "Pending";
  const gateReady = pendingRequest !== null;

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
          {gateVisible ? (
            <div className="spec-pane__gate" role="group" aria-label="Plan approval">
              <p className="spec-pane__gate-prompt">
                {gateReady
                  ? "Approve the plan to begin edits."
                  : "Waiting for the agent's approval request…"}
              </p>
              <button
                type="button"
                disabled={!gateReady}
                onClick={() => decide("approve")}
              >
                Approve
              </button>
              <button
                type="button"
                disabled={!gateReady}
                onClick={() => decide("adjust")}
              >
                Adjust
              </button>
              <button
                type="button"
                disabled={!gateReady}
                onClick={() => decide("reject")}
              >
                Reject
              </button>
            </div>
          ) : null}
        </>
      )}
    </section>
  );
}
