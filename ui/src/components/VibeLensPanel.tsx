import { useEffect, useRef, useState } from "react";

import {
  getDiffExplanation,
  listenDiffUpdated,
  type DiffExplanation,
  type SessionId,
} from "../session/ipc";

/** Props for the VibeLens panel: the session whose diff it explains. */
export interface VibeLensPanelProps {
  sessionId: SessionId;
}

/**
 * Runtime guard for a hydrate / refresh payload. `getDiffExplanation` resolves to a
 * `DiffExplanation | null`; a corrupt/unexpected blob degrades to "no explanation"
 * rather than crashing the render (defensive — mirrors `SpecPane.isSpecContract`).
 */
function isDiffExplanation(value: unknown): value is DiffExplanation {
  return (
    typeof value === "object" &&
    value !== null &&
    Array.isArray((value as { files?: unknown }).files)
  );
}

/**
 * Classify an explanation for rendering. A `DiffExplanation` with files renders the
 * per-file rationale. With NO files we distinguish two states by the summary:
 * - empty summary  → "no changes" (the pipeline's `empty()` sentinel),
 * - non-empty summary → "degraded/unavailable" (the pipeline retained a message
 *   when git/VibeLens failed — diff-pipeline `Degraded` outcome).
 */
function classify(
  explanation: DiffExplanation | null,
): "loading" | "empty" | "degraded" | "files" {
  if (explanation === null) {
    return "loading";
  }
  if (explanation.files.length > 0) {
    return "files";
  }
  return explanation.summary.length > 0 ? "degraded" : "empty";
}

/**
 * The VibeLens "Why" panel (D29/D34/D37).
 *
 * Renders the `DiffExplanation` per-file rationale and updates on each `diff_updated`
 * event without a manual refresh (the backend pipeline is authoritative). On mount it
 * reads the current explanation via `getDiffExplanation`; a manual refresh control
 * re-reads it on demand (the cooperative/FileWatch triggers drive the live path, this
 * is the explicit user-forced fallback — vibelens-panel-ui scenario 3). An empty
 * explanation shows a "no changes" state; a degraded one shows "unavailable" — never a
 * blank panel or crash.
 *
 * React 19 / React Compiler — named imports, no manual `useMemo`/`useCallback`. The
 * panel ignores `diff_updated` events for any other session (backend authoritative).
 */
export function VibeLensPanel({ sessionId }: VibeLensPanelProps) {
  const [explanation, setExplanation] = useState<DiffExplanation | null>(null);

  const sessionIdRef = useRef(sessionId);
  sessionIdRef.current = sessionId;

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    void getDiffExplanation(sessionId).then((current) => {
      if (!cancelled && isDiffExplanation(current)) {
        setExplanation((prev) => prev ?? current);
      }
    });

    void listenDiffUpdated((payload) => {
      if (payload.session_id === sessionIdRef.current) {
        setExplanation(payload.explanation);
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
  }, [sessionId]);

  const refresh = () => {
    void getDiffExplanation(sessionIdRef.current).then((current) => {
      if (isDiffExplanation(current)) {
        setExplanation(current);
      }
    });
  };

  const state = classify(explanation);

  return (
    <section className="vibelens-panel" aria-label="VibeLens">
      <div className="vibelens-panel__bar">
        <span className="vibelens-panel__title">Why</span>
        <button
          type="button"
          className="vibelens-panel__refresh"
          onClick={refresh}
        >
          Refresh
        </button>
      </div>
      {state === "loading" ? (
        <p className="vibelens-panel__loading" data-testid="vibelens-loading">
          No explanation yet.
        </p>
      ) : null}
      {state === "empty" ? (
        <p className="vibelens-panel__empty" data-testid="vibelens-empty">
          No changes to explain.
        </p>
      ) : null}
      {state === "degraded" ? (
        <p className="vibelens-panel__degraded" data-testid="vibelens-degraded">
          VibeLens unavailable — {explanation?.summary}
        </p>
      ) : null}
      {state === "files" && explanation !== null ? (
        <>
          {explanation.summary.length > 0 ? (
            <p className="vibelens-panel__summary">{explanation.summary}</p>
          ) : null}
          <ul className="vibelens-panel__files">
            {explanation.files.map((file) => (
              <li
                key={file.path}
                className="vibelens-file"
                data-file-path={file.path}
              >
                <span className="vibelens-file__path">{file.path}</span>
                <span className="vibelens-file__rationale">
                  {file.rationale}
                </span>
              </li>
            ))}
          </ul>
        </>
      ) : null}
    </section>
  );
}
