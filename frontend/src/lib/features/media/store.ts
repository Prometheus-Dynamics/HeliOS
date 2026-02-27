import { get, writable, type Readable } from 'svelte/store';
import { invalidateSWRPrefix, revalidateSWR } from '$lib/utils/swrCache';
import { cancelDebounce, scheduleDebounce, type DebounceHandle } from '$lib/utils/debounce';
import {
  listMediaAssets,
  type MediaAsset,
  type MediaAssetListResult,
  type MediaAssetType,
  type MediaListSort
} from './api';

export type MediaListFilters = {
  kind: 'all' | MediaAssetType;
  sort: MediaListSort;
  query: string;
  cameraSource: string | null;
};

export type MediaListState = {
  assets: MediaAsset[];
  total: number;
  page: number;
  pageSize: number;
  counts: MediaAssetListResult['counts'] | null;
  loading: boolean;
  loadingMore: boolean;
  error: string | null;
};

export type MediaListStore = {
  state: Readable<MediaListState>;
  filters: Readable<MediaListFilters>;
  setFilters: (next: Partial<MediaListFilters>, options?: { refresh?: boolean; resetPage?: boolean }) => void;
  setQuery: (value: string, options?: { debounceMs?: number; refresh?: boolean }) => void;
  refresh: (options?: { resetPage?: boolean; includeCounts?: boolean; page?: number }) => Promise<void>;
  loadMore: () => Promise<void>;
  replaceAsset: (asset: MediaAsset) => void;
  removeAsset: (id: string) => void;
  clear: () => void;
  destroy: () => void;
};

const DEFAULT_PAGE_SIZE = 32;

export function createMediaListStore(options: {
  pageSize?: number;
  includeCounts?: boolean;
  useCache?: boolean;
  cacheKeyPrefix?: string;
  cacheStaleMs?: number;
  cacheMaxAgeMs?: number;
  debounceMs?: number;
} = {}): MediaListStore {
  const {
    pageSize,
    includeCounts = false,
    useCache = false,
    cacheKeyPrefix = 'media:list',
    cacheStaleMs = 10_000,
    cacheMaxAgeMs = 120_000,
    debounceMs = 250
  } = options;

  const initialPageSize = pageSize ?? DEFAULT_PAGE_SIZE;

  const state = writable<MediaListState>({
    assets: [],
    total: 0,
    page: 1,
    pageSize: initialPageSize,
    counts: null,
    loading: false,
    loadingMore: false,
    error: null
  });

  const filters = writable<MediaListFilters>({
    kind: 'all',
    sort: 'recent',
    query: '',
    cameraSource: null
  });

  let requestId = 0;
  let searchTimer: DebounceHandle = null;

  function buildCacheKey(nextFilters: MediaListFilters, pageTarget: number, includeCountsFlag: boolean): string {
    const pageSizeKey = pageSize ?? get(state).pageSize;
    return [
      cacheKeyPrefix,
      `kind=${encodeURIComponent(nextFilters.kind)}`,
      `source=${encodeURIComponent(nextFilters.cameraSource ?? 'all')}`,
      `search=${encodeURIComponent(nextFilters.query)}`,
      `sort=${nextFilters.sort}`,
      `page=${pageTarget}`,
      `pageSize=${pageSizeKey}`,
      `counts=${includeCountsFlag ? '1' : '0'}`
    ].join('|');
  }

  function applyResult(result: MediaAssetListResult, options: { resetPage: boolean }) {
    state.update((current) => {
      const nextAssets = options.resetPage
        ? result.assets
        : mergeUniqueAssets(current.assets, result.assets);
      return {
        ...current,
        assets: nextAssets,
        total: result.total,
        page: result.page,
        pageSize: result.pageSize,
        counts: result.counts ?? (options.resetPage ? current.counts : current.counts)
      };
    });
  }

  async function refresh(options: { resetPage?: boolean; includeCounts?: boolean; page?: number } = {}): Promise<void> {
    const snapshot = get(filters);
    if (snapshot.cameraSource !== null && !String(snapshot.cameraSource).trim()) {
      clear();
      return;
    }

    const resetPage = options.resetPage ?? true;
    const includeCountsFlag = options.includeCounts ?? includeCounts;
    const nextPage = options.page ?? (resetPage ? 1 : get(state).page + 1);
    const requestPageSize = pageSize ?? undefined;
    const nextRequestId = ++requestId;

    state.update((current) => ({
      ...current,
      loading: resetPage,
      loadingMore: !resetPage,
      error: null
    }));

    try {
      const loader = () =>
        listMediaAssets({
          kind: snapshot.kind === 'all' ? undefined : snapshot.kind,
          cameraSource: snapshot.cameraSource ? snapshot.cameraSource : undefined,
          page: nextPage,
          pageSize: requestPageSize,
          search: snapshot.query.trim() || undefined,
          includeCounts: includeCountsFlag,
          sort: snapshot.sort
        });

      const result = useCache
        ? await revalidateSWR(buildCacheKey(snapshot, nextPage, includeCountsFlag), loader, {
            staleMs: cacheStaleMs,
            maxAgeMs: cacheMaxAgeMs
          })
        : await loader();

      if (nextRequestId !== requestId) return;
      applyResult(result, { resetPage });
    } catch (error) {
      if (nextRequestId !== requestId) return;
      if (useCache) invalidateSWRPrefix(cacheKeyPrefix);
      const message = error instanceof Error ? error.message : 'Unable to load media.';
      state.update((current) => ({
        ...current,
        assets: resetPage ? [] : current.assets,
        total: resetPage ? 0 : current.total,
        error: message
      }));
    } finally {
      if (nextRequestId === requestId) {
        state.update((current) => ({
          ...current,
          loading: false,
          loadingMore: false
        }));
      }
    }
  }

  async function loadMore(): Promise<void> {
    const snapshot = get(state);
    if (snapshot.loading || snapshot.loadingMore) return;
    if (snapshot.assets.length >= snapshot.total) return;
    await refresh({ resetPage: false, includeCounts: false, page: snapshot.page + 1 });
  }

  function setFilters(next: Partial<MediaListFilters>, options: { refresh?: boolean; resetPage?: boolean } = {}): void {
    const shouldRefresh = options.refresh ?? true;
    const shouldReset = options.resetPage ?? true;
    filters.update((current) => ({ ...current, ...next }));
    if (shouldRefresh) {
      void refresh({ resetPage: shouldReset, includeCounts });
    }
  }

  function setQuery(value: string, options: { debounceMs?: number; refresh?: boolean } = {}): void {
    const delay = options.debounceMs ?? debounceMs;
    const shouldRefresh = options.refresh ?? true;

    searchTimer = cancelDebounce(searchTimer);

    if (delay <= 0) {
      filters.update((current) => ({ ...current, query: value }));
      if (shouldRefresh) void refresh({ resetPage: true, includeCounts });
      return;
    }

    searchTimer = scheduleDebounce(searchTimer, () => {
      filters.update((current) => ({ ...current, query: value }));
      if (shouldRefresh) void refresh({ resetPage: true, includeCounts });
    }, delay);
  }

  function replaceAsset(asset: MediaAsset): void {
    state.update((current) => ({
      ...current,
      assets: current.assets.map((entry) => (entry.id === asset.id ? asset : entry))
    }));
  }

  function removeAsset(id: string): void {
    state.update((current) => ({
      ...current,
      assets: current.assets.filter((entry) => entry.id !== id),
      total: Math.max(0, current.total - 1)
    }));
  }

  function clear(): void {
    state.set({
      assets: [],
      total: 0,
      page: 1,
      pageSize: initialPageSize,
      counts: null,
      loading: false,
      loadingMore: false,
      error: null
    });
  }

  function destroy(): void {
    searchTimer = cancelDebounce(searchTimer);
  }

  return {
    state,
    filters,
    setFilters,
    setQuery,
    refresh,
    loadMore,
    replaceAsset,
    removeAsset,
    clear,
    destroy
  };
}

function mergeUniqueAssets(existing: MediaAsset[], incoming: MediaAsset[]): MediaAsset[] {
  const known = new Set(existing.map((entry) => entry.id));
  const merged = [...existing];
  for (const asset of incoming) {
    if (known.has(asset.id)) continue;
    merged.push(asset);
    known.add(asset.id);
  }
  return merged;
}
