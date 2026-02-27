import { invalidateSWR, readSWR, revalidateSWR, type SwrReadOptions } from '$lib/utils/swrCache';

export type RefreshableResource<T> = {
  read: () => ReturnType<typeof readSWR<T>>;
  refresh: (options?: { force?: boolean }) => Promise<T>;
  invalidate: () => void;
};

export function createRefreshableResource<T>(options: {
  key: string;
  loader: () => Promise<T>;
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
      revalidateSWR<T>(options.key, options.loader, {
        staleMs: readOptions.staleMs ?? 0,
        maxAgeMs: readOptions.maxAgeMs ?? 0,
        force: refreshOptions?.force
      }),
    invalidate: () => invalidateSWR(options.key)
  };
}
