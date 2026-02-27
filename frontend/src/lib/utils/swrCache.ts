type SwrEntry<T> = {
  data?: T;
  fetchedAt: number;
  promise?: Promise<T>;
  error?: unknown;
};

export type SwrReadOptions = {
  staleMs?: number;
  maxAgeMs?: number;
};

type SwrFetchOptions = {
  staleMs: number;
  maxAgeMs: number;
  force?: boolean;
};

type SwrSnapshot<T> = {
  data: T;
  fetchedAt: number;
  ageMs: number;
  isStale: boolean;
};

const cache = new Map<string, SwrEntry<unknown>>();

export function readSWR<T>(key: string, options: SwrReadOptions = {}): SwrSnapshot<T> | null {
  const entry = cache.get(key) as SwrEntry<T> | undefined;
  if (!entry || entry.data === undefined) return null;
  const ageMs = Date.now() - entry.fetchedAt;
  if (options.maxAgeMs != null && ageMs > options.maxAgeMs) {
    cache.delete(key);
    return null;
  }
  const isStale = options.staleMs != null ? ageMs >= options.staleMs : false;
  return {
    data: entry.data,
    fetchedAt: entry.fetchedAt,
    ageMs,
    isStale
  };
}

export function primeSWR<T>(key: string, data: T): void {
  cache.set(key, { data, fetchedAt: Date.now() });
}

export function invalidateSWR(key: string): void {
  cache.delete(key);
}

export function invalidateSWRPrefix(prefix: string): void {
  if (!prefix) return;
  for (const key of cache.keys()) {
    if (key.startsWith(prefix)) {
      cache.delete(key);
    }
  }
}

export async function revalidateSWR<T>(
  key: string,
  fetcher: () => Promise<T>,
  options: SwrFetchOptions
): Promise<T> {
  const now = Date.now();
  const entry = cache.get(key) as SwrEntry<T> | undefined;
  if (entry?.promise) {
    return entry.promise;
  }
  if (entry && entry.data !== undefined && !options.force) {
    const ageMs = now - entry.fetchedAt;
    if (ageMs <= options.maxAgeMs && ageMs < options.staleMs) {
      return entry.data;
    }
  }

  const promise = fetcher()
    .then((data) => {
      cache.set(key, { data, fetchedAt: Date.now() });
      return data;
    })
    .catch((error) => {
      if (entry) {
        entry.error = error;
        cache.set(key, entry);
      }
      throw error;
    })
    .finally(() => {
      const current = cache.get(key) as SwrEntry<T> | undefined;
      if (current?.promise === promise) {
        delete current.promise;
        cache.set(key, current);
      }
    });

  cache.set(key, { data: entry?.data, fetchedAt: entry?.fetchedAt ?? 0, promise });
  return promise;
}
