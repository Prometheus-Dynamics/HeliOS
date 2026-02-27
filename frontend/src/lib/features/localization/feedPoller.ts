import { createPoller } from '$lib/utils/poller';

export type FeedPoller = ReturnType<typeof createFeedPoller>;

export function pollIntervalMs(pollHz: number): number {
  const clamped = Math.max(1, Math.min(240, pollHz));
  return Math.max(1, Math.floor(1000 / clamped));
}

export function createFeedPoller(options: {
  getIntervalMs: () => number;
  hasSources: () => boolean;
  onPoll: (signal: AbortSignal) => Promise<void>;
}) {
  return createPoller({
    getIntervalMs: options.getIntervalMs,
    shouldPoll: options.hasSources,
    onPoll: options.onPoll
  });
}
