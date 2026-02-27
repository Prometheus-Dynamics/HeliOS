export type Poller = ReturnType<typeof createPoller>;

export function createPoller(options: {
  getIntervalMs: () => number;
  shouldPoll?: () => boolean;
  onPoll: (signal: AbortSignal) => Promise<void>;
}) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let controller: AbortController | null = null;
  let inFlight = false;
  let nonce = 0;

  const shouldContinue = options.shouldPoll ?? (() => true);

  function stop(): void {
    nonce += 1;
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    if (controller) {
      controller.abort();
      controller = null;
    }
    inFlight = false;
  }

  function schedule(delayMs: number): void {
    if (timer) {
      clearTimeout(timer);
    }
    timer = setTimeout(() => {
      timer = null;
      void poll();
    }, delayMs);
  }

  async function poll(): Promise<void> {
    const currentNonce = nonce;
    if (inFlight) {
      schedule(options.getIntervalMs());
      return;
    }
    inFlight = true;
    controller?.abort();
    controller = new AbortController();
    const signal = controller.signal;
    try {
      await options.onPoll(signal);
    } finally {
      inFlight = false;
      if (controller?.signal === signal) {
        controller = null;
      }
      if (currentNonce === nonce && shouldContinue()) {
        schedule(options.getIntervalMs());
      }
    }
  }

  function isBusy(): boolean {
    return inFlight || timer !== null;
  }

  return {
    stop,
    schedule,
    poll,
    isBusy
  };
}
