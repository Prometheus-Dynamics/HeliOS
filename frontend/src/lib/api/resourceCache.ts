export type ResourceCacheMetadata = {
  etag?: string | null;
  revision?: number | null;
};

export type ResourceCacheContext<T> = {
  cached?: T;
  etag?: string | null;
  revision?: number | null;
};

const RESOURCE_CACHE_RESULT = '__helios_resource_cache__';

export type ResourceCacheResult<T> = {
  [RESOURCE_CACHE_RESULT]: true;
  status: 'data' | 'not_modified';
  data?: T;
  etag?: string | null;
  revision?: number | null;
};

export function cacheResourceData<T>(data: T, metadata: ResourceCacheMetadata = {}): ResourceCacheResult<T> {
  return {
    [RESOURCE_CACHE_RESULT]: true,
    status: 'data',
    data,
    etag: metadata.etag ?? null,
    revision: metadata.revision ?? null
  };
}

export function cacheResourceNotModified<T>(metadata: ResourceCacheMetadata = {}): ResourceCacheResult<T> {
  return {
    [RESOURCE_CACHE_RESULT]: true,
    status: 'not_modified',
    etag: metadata.etag ?? null,
    revision: metadata.revision ?? null
  };
}

export function isResourceCacheResult<T>(value: unknown): value is ResourceCacheResult<T> {
  if (!value || typeof value !== 'object') return false;
  return (value as Record<string, unknown>)[RESOURCE_CACHE_RESULT] === true;
}
