export type FocusHighlight = { nodeId: string; port: string | null; token: number };

type FocusHighlightDeps = {
  durationMs: number;
  setFocusHighlight: (value: FocusHighlight | null) => void;
  getFocusHighlight: () => FocusHighlight | null;
};

export function createFocusHighlighter(deps: FocusHighlightDeps) {
  let focusHighlightTimer: ReturnType<typeof setTimeout> | null = null;

  const clearFocusHighlightTimer = () => {
    if (focusHighlightTimer) {
      clearTimeout(focusHighlightTimer);
      focusHighlightTimer = null;
    }
  };

  const applyFocusHighlight = (nodeId: string | null, port: string | null) => {
    clearFocusHighlightTimer();
    if (!nodeId) {
      deps.setFocusHighlight(null);
      return;
    }
    const token = Date.now();
    deps.setFocusHighlight({ nodeId, port: port ?? null, token });
    focusHighlightTimer = setTimeout(() => {
      if (deps.getFocusHighlight()?.token === token) {
        deps.setFocusHighlight(null);
      }
      focusHighlightTimer = null;
    }, deps.durationMs);
  };

  return {
    applyFocusHighlight,
    clearFocusHighlightTimer
  };
}
