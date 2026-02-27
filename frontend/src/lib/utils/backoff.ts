export type BackoffOptions = {
  baseMs: number;
  maxMs?: number;
  factor?: number;
};

export type BackoffTimer = {
  reset: () => void;
  bump: () => number;
  schedule: (fn: () => void, delayMs?: number) => void;
  cancel: () => void;
  getDelay: () => number;
};

export function createBackoffTimer(options: BackoffOptions): BackoffTimer {
  const baseMs = Math.max(0, options.baseMs);
  const maxMs = Math.max(baseMs, options.maxMs ?? baseMs);
  const factor = Math.max(1, options.factor ?? 2);
  let delayMs = baseMs;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const cancel = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  };

  const reset = () => {
    delayMs = baseMs;
  };

  const bump = () => {
    delayMs = Math.min(maxMs, Math.max(baseMs, delayMs * factor));
    return delayMs;
  };

  const schedule = (fn: () => void, nextDelay?: number) => {
    cancel();
    const wait = typeof nextDelay === 'number' ? Math.max(0, nextDelay) : delayMs;
    timer = setTimeout(() => {
      timer = null;
      fn();
    }, wait);
  };

  const getDelay = () => delayMs;

  return { reset, bump, schedule, cancel, getDelay };
}
