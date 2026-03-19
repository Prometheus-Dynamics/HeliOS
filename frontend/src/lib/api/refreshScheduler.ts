import { browser } from '$app/environment';

type RefreshSchedulerOptions = {
  intervalMs: number;
  immediate?: boolean;
  refreshOnFocus?: boolean;
  refreshOnOnline?: boolean;
  allowHidden?: boolean;
  enabled?: () => boolean;
  onError?: (error: unknown) => void;
};

export function startRefreshScheduler(task: () => void | Promise<void>, options: RefreshSchedulerOptions): () => void {
  if (!browser) {
    return () => {};
  }

  const intervalMs = Math.max(50, Math.floor(options.intervalMs));
  const allowHidden = options.allowHidden ?? false;
  const refreshOnFocus = options.refreshOnFocus ?? true;
  const refreshOnOnline = options.refreshOnOnline ?? true;
  const enabled = options.enabled ?? (() => true);

  let stopped = false;
  let inFlight = false;
  let pending = false;

  const run = async (): Promise<void> => {
    if (stopped) return;
    if (!enabled()) return;
    if (!allowHidden && document.hidden) return;
    if (inFlight) {
      pending = true;
      return;
    }
    inFlight = true;
    try {
      await task();
    } catch (error) {
      options.onError?.(error);
    } finally {
      inFlight = false;
      if (pending) {
        pending = false;
        queueMicrotask(() => {
          void run();
        });
      }
    }
  };

  const intervalId = window.setInterval(() => {
    void run();
  }, intervalMs);

  const handleVisibilityChange = (): void => {
    if (!document.hidden) {
      void run();
    }
  };
  const handleFocus = (): void => {
    void run();
  };
  const handleOnline = (): void => {
    void run();
  };

  document.addEventListener('visibilitychange', handleVisibilityChange);
  if (refreshOnFocus) {
    window.addEventListener('focus', handleFocus);
  }
  if (refreshOnOnline) {
    window.addEventListener('online', handleOnline);
  }

  if (options.immediate ?? true) {
    void run();
  }

  return () => {
    stopped = true;
    window.clearInterval(intervalId);
    document.removeEventListener('visibilitychange', handleVisibilityChange);
    if (refreshOnFocus) {
      window.removeEventListener('focus', handleFocus);
    }
    if (refreshOnOnline) {
      window.removeEventListener('online', handleOnline);
    }
  };
}
