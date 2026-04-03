import { PipelinesService } from '$lib/api/client';
import { apiFetchCachedJson, runApiRequest, type ApiRequestOptions } from '$lib/api/core/http';
import { invalidateSWRPrefix } from '$lib/utils/swrCache';

type CacheEntry<T> = {
  fetchedAt: number;
  value: T;
  etag?: string | null;
  revision?: number | null;
};

const DEFAULT_REGISTRY_CACHE_MS = 5_000;
const DEFAULT_GRAPHS_CACHE_MS = 2_000;
const DEFAULT_TEMPLATES_CACHE_MS = 10_000;
type RegistrySnapshot = Awaited<ReturnType<typeof PipelinesService.listRegistry>>;
type GraphList = Awaited<ReturnType<typeof PipelinesService.listGraphs>>;
type TemplateList = Awaited<ReturnType<typeof PipelinesService.listTemplates>>;
let registryCache: CacheEntry<RegistrySnapshot> | null = null;
let registryInflight: Promise<RegistrySnapshot> | null = null;
let graphsCache: CacheEntry<GraphList> | null = null;
let graphsInflight: Promise<GraphList> | null = null;
let templatesCache: CacheEntry<TemplateList> | null = null;
let templatesInflight: Promise<TemplateList> | null = null;

function resolveCacheMs(options: ApiRequestOptions | undefined, defaultMs: number): number {
  const raw = options?.cacheMs;
  if (typeof raw !== 'number' || !Number.isFinite(raw)) {
    return defaultMs;
  }
  return Math.max(0, Math.floor(raw));
}

async function listRegistrySingleflight(options?: ApiRequestOptions) {
  const cacheMs = resolveCacheMs(options, DEFAULT_REGISTRY_CACHE_MS);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && registryCache && now - registryCache.fetchedAt < cacheMs) {
    return registryCache.value;
  }

  if (registryInflight) {
    return registryInflight;
  }

  registryInflight = apiFetchCachedJson<RegistrySnapshot>(
    '/pipelines/registry',
    {
      cached: registryCache?.value,
      etag: registryCache?.etag ?? null,
      revision: registryCache?.revision ?? null
    },
    { headers: { Accept: 'application/json' } },
    { label: 'listRegistry', ...options }
  )
    .then((result) => {
      const value = result.status === 'not_modified' ? registryCache?.value : result.data;
      if (!value) {
        throw new Error('Pipeline registry unavailable');
      }
      if (cacheMs > 0) {
        registryCache = {
          fetchedAt: Date.now(),
          value,
          etag: result.etag ?? registryCache?.etag ?? null,
          revision: result.revision ?? registryCache?.revision ?? null
        };
      } else {
        registryCache = null;
      }
      return value;
    })
    .finally(() => {
      registryInflight = null;
    });

  return registryInflight;
}

async function listGraphsSingleflight(options?: ApiRequestOptions) {
  const cacheMs = resolveCacheMs(options, DEFAULT_GRAPHS_CACHE_MS);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && graphsCache && now - graphsCache.fetchedAt < cacheMs) {
    return graphsCache.value;
  }

  if (graphsInflight) {
    return graphsInflight;
  }

  graphsInflight = apiFetchCachedJson<GraphList>(
    '/pipelines/graphs',
    {
      cached: graphsCache?.value,
      etag: graphsCache?.etag ?? null,
      revision: graphsCache?.revision ?? null
    },
    { headers: { Accept: 'application/json' } },
    { label: 'listGraphs', ...options }
  )
    .then((result) => {
      const value = result.status === 'not_modified' ? graphsCache?.value : result.data;
      if (!value) {
        throw new Error('Pipeline graph list unavailable');
      }
      if (cacheMs > 0) {
        graphsCache = {
          fetchedAt: Date.now(),
          value,
          etag: result.etag ?? graphsCache?.etag ?? null,
          revision: result.revision ?? graphsCache?.revision ?? null
        };
      } else {
        graphsCache = null;
      }
      return value;
    })
    .finally(() => {
      graphsInflight = null;
    });

  return graphsInflight;
}

async function listTemplatesSingleflight(options?: ApiRequestOptions) {
  const cacheMs = resolveCacheMs(options, DEFAULT_TEMPLATES_CACHE_MS);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && templatesCache && now - templatesCache.fetchedAt < cacheMs) {
    return templatesCache.value;
  }

  if (templatesInflight) {
    return templatesInflight;
  }

  templatesInflight = runApiRequest(() => PipelinesService.listTemplates(), { label: 'listTemplates', ...options })
    .then((value) => {
      if (cacheMs > 0) {
        templatesCache = { fetchedAt: Date.now(), value };
      } else {
        templatesCache = null;
      }
      return value;
    })
    .finally(() => {
      templatesInflight = null;
    });

  return templatesInflight;
}

function invalidateGraphsCache(): void {
  graphsCache = null;
}

export const PipelinesApi = {
  listRegistry: (options?: ApiRequestOptions) => listRegistrySingleflight(options),
  fetchGraph: (args: Parameters<typeof PipelinesService.fetchGraph>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => PipelinesService.fetchGraph(args), { label: 'fetchGraph', ...options }),
  listGraphs: (options?: ApiRequestOptions) => listGraphsSingleflight(options),
  listTemplates: (options?: ApiRequestOptions) => listTemplatesSingleflight(options),
  uploadGraph: async (args: Parameters<typeof PipelinesService.uploadGraph>[0], options?: ApiRequestOptions) => {
    const result = await runApiRequest(() => PipelinesService.uploadGraph(args), { label: 'uploadGraph', ...options });
    invalidateGraphsCache();
    invalidateSWRPrefix('pipelines:');
    return result;
  },
  fetchTemplate: (args: Parameters<typeof PipelinesService.fetchTemplate>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => PipelinesService.fetchTemplate(args), { label: 'fetchTemplate', ...options }),
  deleteGraph: async (args: Parameters<typeof PipelinesService.deleteGraph>[0], options?: ApiRequestOptions) => {
    const result = await runApiRequest(() => PipelinesService.deleteGraph(args), { label: 'deleteGraph', ...options });
    invalidateGraphsCache();
    invalidateSWRPrefix('pipelines:');
    return result;
  },
  validateGraph: (args: Parameters<typeof PipelinesService.validateGraph>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => PipelinesService.validateGraph(args), { label: 'validateGraph', ...options }),
  updateGraph: async (
    args: Parameters<typeof PipelinesService.updateGraph>[0],
    options?: ApiRequestOptions
  ) => {
    const result = await runApiRequest(() => PipelinesService.updateGraph(args), { label: 'updateGraph', ...options });
    invalidateGraphsCache();
    invalidateSWRPrefix('pipelines:');
    return result;
  },
};
