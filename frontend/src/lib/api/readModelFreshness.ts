import type { ReadModelFreshness } from '$lib/ts-bindings/http/client';

export function readModelFreshnessLabel(freshness: ReadModelFreshness | null | undefined): string {
  switch (freshness?.state) {
    case 'live':
      return 'Live';
    case 'stale':
      return 'Stale';
    case 'unavailable':
      return 'Unavailable';
    default:
      return 'Unknown';
  }
}

export function readModelFreshnessDetail(freshness: ReadModelFreshness | null | undefined): string {
  if (!freshness) {
    return 'Snapshot freshness is unavailable.';
  }

  if (freshness.state === 'live') {
    return 'Read model is serving a live snapshot.';
  }

  if (freshness.state === 'stale') {
    return freshness.reason === 'refresh_timeout'
      ? 'Showing cached data because the refresh timed out.'
      : 'Showing cached data because the refresh failed.';
  }

  return freshness.reason === 'refresh_timeout'
    ? 'Snapshot is unavailable because the refresh timed out.'
    : 'Snapshot is unavailable because the refresh failed.';
}

export function isReadModelLive(freshness: ReadModelFreshness | null | undefined): boolean {
  return freshness?.state === 'live';
}
