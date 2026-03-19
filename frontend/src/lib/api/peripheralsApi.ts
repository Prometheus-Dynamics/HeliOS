import { apiFetchCachedJson, type ApiRequestOptions } from '$lib/api/core/http';
import type { CameraDiscoveryResponse, PeripheralInventory } from '$lib/ts-bindings/http/client';

type CacheEntry<T> = {
  fetchedAt: number;
  value: T;
  etag?: string | null;
  revision?: number | null;
};

const DEFAULT_PERIPHERALS_CACHE_MS = 1_000;
const DEFAULT_CAMERAS_CACHE_MS = 5_000;

let peripheralsCache: CacheEntry<PeripheralInventory> | null = null;
let peripheralsInflight: Promise<PeripheralInventory> | null = null;
let camerasCache: CacheEntry<CameraDiscoveryResponse> | null = null;
let camerasInflight: Promise<CameraDiscoveryResponse> | null = null;

function resolveCacheMs(raw: number | undefined, fallback: number): number {
  if (typeof raw !== 'number' || !Number.isFinite(raw)) {
    return fallback;
  }
  return Math.max(0, Math.floor(raw));
}

async function listPeripheralsSingleflight(options?: ApiRequestOptions): Promise<PeripheralInventory> {
  const cacheMs = resolveCacheMs(options?.cacheMs, DEFAULT_PERIPHERALS_CACHE_MS);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && peripheralsCache && now - peripheralsCache.fetchedAt < cacheMs) {
    return peripheralsCache.value;
  }
  if (peripheralsInflight) {
    return peripheralsInflight;
  }

  peripheralsInflight = apiFetchCachedJson<PeripheralInventory>(
    '/peripherals',
    {
      cached: peripheralsCache?.value,
      etag: peripheralsCache?.etag ?? null,
      revision: peripheralsCache?.revision ?? null
    },
    { headers: { Accept: 'application/json' } },
    { label: 'listPeripherals', ...options }
  )
    .then((result) => {
      const value = result.status === 'not_modified' ? peripheralsCache?.value : result.data;
      if (!value) {
        throw new Error('Peripherals inventory unavailable');
      }
      peripheralsCache = {
        fetchedAt: Date.now(),
        value,
        etag: result.etag ?? peripheralsCache?.etag ?? null,
        revision: result.revision ?? peripheralsCache?.revision ?? null
      };
      return value;
    })
    .finally(() => {
      peripheralsInflight = null;
    });
  return peripheralsInflight;
}

async function listCamerasSingleflight(options?: ApiRequestOptions): Promise<CameraDiscoveryResponse> {
  const cacheMs = resolveCacheMs(options?.cacheMs, DEFAULT_CAMERAS_CACHE_MS);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && camerasCache && now - camerasCache.fetchedAt < cacheMs) {
    return camerasCache.value;
  }
  if (camerasInflight) {
    return camerasInflight;
  }

  camerasInflight = apiFetchCachedJson<CameraDiscoveryResponse>(
    '/peripherals/cameras',
    {
      cached: camerasCache?.value,
      etag: camerasCache?.etag ?? null,
      revision: camerasCache?.revision ?? null
    },
    { headers: { Accept: 'application/json' } },
    { label: 'listCameras', ...options }
  )
    .then((result) => {
      const value = result.status === 'not_modified' ? camerasCache?.value : result.data;
      if (!value) {
        throw new Error('Camera inventory unavailable');
      }
      camerasCache = {
        fetchedAt: Date.now(),
        value,
        etag: result.etag ?? camerasCache?.etag ?? null,
        revision: result.revision ?? camerasCache?.revision ?? null
      };
      return value;
    })
    .finally(() => {
      camerasInflight = null;
    });

  return camerasInflight;
}

export const PeripheralsApi = {
  listPeripherals: (options?: ApiRequestOptions) => listPeripheralsSingleflight(options),
  listCameras: (options?: ApiRequestOptions) => listCamerasSingleflight(options)
};
