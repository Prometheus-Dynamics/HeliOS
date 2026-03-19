import { createPoller } from '$lib/utils/poller';

export type FeedPoller = ReturnType<typeof createFeedPoller>;

export type PollRateLimits = {
  minHz: number;
  maxHz: number;
  defaultHz: number;
};

export function normalizePollHz(pollHz: number, limits?: PollRateLimits | null): number {
  const hasLimits =
    Number.isFinite(limits?.minHz) &&
    Number.isFinite(limits?.maxHz) &&
    Number.isFinite(limits?.defaultHz) &&
    (limits?.minHz ?? 0) > 0 &&
    (limits?.maxHz ?? 0) > 0;
  if (!hasLimits) {
    return Number.isFinite(pollHz) && pollHz > 0 ? pollHz : 1;
  }
  const minHz = Math.max(1, Math.floor(limits!.minHz));
  const maxHz = Math.max(minHz, Math.floor(limits!.maxHz));
  const fallbackHz = Math.min(maxHz, Math.max(minHz, Math.floor(limits!.defaultHz)));
  const requestedHz = Number.isFinite(pollHz) && pollHz > 0 ? pollHz : fallbackHz;
  return Math.min(maxHz, Math.max(minHz, requestedHz));
}

export function pollIntervalMs(pollHz: number, limits?: PollRateLimits | null): number {
  const clamped = normalizePollHz(pollHz, limits);
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
