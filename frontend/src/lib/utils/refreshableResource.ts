import {
  invalidateSWR,
  readSWR,
  revalidateSWR,
  type SwrReadOptions,
  type SwrSnapshot,
  type SwrWrite
} from '$lib/utils/swrCache';
import {
  isResourceCacheResult,
  type ResourceCacheContext,
  type ResourceCacheResult
} from '$lib/api/resourceCache';

export type RefreshableResource<T> = {
  read: () => ReturnType<typeof readSWR<T>>;
  refresh: (options?: { force?: boolean }) => Promise<T>;
  invalidate: () => void;
};

export type RefreshableLoader<T> = (context: ResourceCacheContext<T>) => Promise<T | ResourceCacheResult<T>>;
export type RefreshableLoaderLike<T> = RefreshableLoader<T> | (() => Promise<T | ResourceCacheResult<T>>);

function resolveLoaderWrite<T>(result: T | ResourceCacheResult<T>, cached: SwrSnapshot<T> | null): SwrWrite<T> {
  if (!isResourceCacheResult<T>(result)) {
    return { __heliosSwrWrite: true, data: result };
  }

  if (result.status === 'not_modified') {
    if (!cached) {
      throw new Error('Conditional resource returned not_modified without cached data');
    }
    return {
      __heliosSwrWrite: true,
      data: cached.data,
      meta: {
        etag: result.etag ?? cached.etag ?? null,
        revision: result.revision ?? cached.revision ?? null
      }
    };
  }

  if (typeof result.data === 'undefined') {
    throw new Error('Conditional resource returned no data');
  }

  return {
    __heliosSwrWrite: true,
    data: result.data,
    meta: {
      etag: result.etag ?? null,
      revision: result.revision ?? null
    }
  };
}

export function createRefreshableResource<T>(options: {
  key: string;
  loader: RefreshableLoaderLike<T>;
  staleMs?: number;
  maxAgeMs?: number;
}): RefreshableResource<T> {
  const readOptions: SwrReadOptions = {
    staleMs: options.staleMs,
    maxAgeMs: options.maxAgeMs
  };

  return {
    read: () => readSWR<T>(options.key, readOptions),
    refresh: (refreshOptions) =>
      revalidateSWR<T>(
        options.key,
        async () => {
          const cached = readSWR<T>(options.key, readOptions);
          const loader = options.loader as RefreshableLoader<T>;
          const result = await loader({
            cached: cached?.data,
            etag: cached?.etag ?? null,
            revision: cached?.revision ?? null
          });
          return resolveLoaderWrite(result, cached);
        },
        {
        staleMs: readOptions.staleMs ?? 0,
        maxAgeMs: readOptions.maxAgeMs ?? 0,
        force: refreshOptions?.force
        }
      ),
    invalidate: () => invalidateSWR(options.key)
  };
}
