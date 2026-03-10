import { apiFetchCachedJson, type ApiRequestOptions } from '$lib/api/core/http';
import type { DeviceMetrics } from '$lib/ts-bindings/http/client';

type CacheEntry<T> = {
  fetchedAt: number;
  value: T;
  etag?: string | null;
  revision?: number | null;
};

const DEFAULT_METRICS_CACHE_MS = 750;

let metricsCache: CacheEntry<DeviceMetrics> | null = null;
let metricsInflight: Promise<DeviceMetrics> | null = null;

function resolveCacheMs(options?: ApiRequestOptions): number {
  const raw = options?.cacheMs;
  if (typeof raw !== 'number' || !Number.isFinite(raw)) {
    return DEFAULT_METRICS_CACHE_MS;
  }
  return Math.max(0, Math.floor(raw));
}

async function metricsSingleflight(options?: ApiRequestOptions): Promise<DeviceMetrics> {
  const cacheMs = resolveCacheMs(options);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && metricsCache && now - metricsCache.fetchedAt < cacheMs) {
    return metricsCache.value;
  }

  if (metricsInflight) {
    return metricsInflight;
  }

  metricsInflight = apiFetchCachedJson<DeviceMetrics>(
    '/device/metrics',
    {
      cached: metricsCache?.value,
      etag: metricsCache?.etag ?? null,
      revision: metricsCache?.revision ?? null
    },
    { headers: { Accept: 'application/json' } },
    { label: 'deviceMetrics', ...options }
  )
    .then((result) => {
      const nextValue = result.status === 'not_modified' ? metricsCache?.value : result.data;
      if (!nextValue) {
        throw new Error('Device metrics unavailable');
      }

      metricsCache = {
        fetchedAt: Date.now(),
        value: nextValue,
        etag: result.etag ?? metricsCache?.etag ?? null,
        revision: result.revision ?? metricsCache?.revision ?? null
      };
      return nextValue;
    })
    .finally(() => {
      metricsInflight = null;
    });

  return metricsInflight;
}

export const DeviceApi = {
  metrics: (options?: ApiRequestOptions) => metricsSingleflight(options)
};
