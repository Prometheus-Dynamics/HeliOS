type IdleWindow = Window & {
  requestIdleCallback?: (
    callback: IdleRequestCallback,
    options?: IdleRequestOptions
  ) => number;
  cancelIdleCallback?: (handle: number) => void;
};

export function scheduleAfterPaint(task: () => void, frames = 1): () => void {
  if (typeof window === 'undefined' || typeof window.requestAnimationFrame !== 'function') {
    const timeout = setTimeout(task, 0);
    return () => clearTimeout(timeout);
  }

  let cancelled = false;
  let handle: number | null = null;

  const queueFrame = (remaining: number) => {
    handle = window.requestAnimationFrame(() => {
      if (cancelled) return;
      if (remaining > 1) {
        queueFrame(remaining - 1);
        return;
      }
      task();
    });
  };

  queueFrame(Math.max(1, Math.floor(frames)));

  return () => {
    cancelled = true;
    if (handle != null && typeof window.cancelAnimationFrame === 'function') {
      window.cancelAnimationFrame(handle);
    }
  };
}

export function scheduleWhenIdle(
  task: () => void,
  options: { timeoutMs?: number; fallbackMs?: number } = {}
): () => void {
  const { timeoutMs = 1500, fallbackMs = 400 } = options;

  if (typeof window === 'undefined') {
    const timeout = setTimeout(task, fallbackMs);
    return () => clearTimeout(timeout);
  }

  const idleWindow = window as IdleWindow;
  let cancelled = false;

  if (typeof idleWindow.requestIdleCallback === 'function') {
    const handle = idleWindow.requestIdleCallback(
      () => {
        if (!cancelled) {
          task();
        }
      },
      { timeout: timeoutMs }
    );
    return () => {
      cancelled = true;
      idleWindow.cancelIdleCallback?.(handle);
    };
  }

  const timeout = setTimeout(() => {
    if (!cancelled) {
      task();
    }
  }, fallbackMs);

  return () => {
    cancelled = true;
    clearTimeout(timeout);
  };
}
