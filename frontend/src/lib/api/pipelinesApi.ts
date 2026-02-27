import { PipelinesService } from '$lib/ts-bindings/http/client';
import { runApiRequest, type ApiRequestOptions } from '$lib/api/requestManager';
import { invalidateSWRPrefix } from '$lib/utils/swrCache';

type CacheEntry<T> = {
  fetchedAt: number;
  value: T;
};

const DEFAULT_REGISTRY_CACHE_MS = 5_000;
type RegistrySnapshot = Awaited<ReturnType<typeof PipelinesService.listRegistry>>;
let registryCache: CacheEntry<RegistrySnapshot> | null = null;
let registryInflight: Promise<RegistrySnapshot> | null = null;

function resolveRegistryCacheMs(options?: ApiRequestOptions): number {
  const raw = options?.cacheMs;
  if (typeof raw !== 'number' || !Number.isFinite(raw)) {
    return DEFAULT_REGISTRY_CACHE_MS;
  }
  return Math.max(0, Math.floor(raw));
}

async function listRegistrySingleflight(options?: ApiRequestOptions) {
  const cacheMs = resolveRegistryCacheMs(options);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && registryCache && now - registryCache.fetchedAt < cacheMs) {
    return registryCache.value;
  }

  if (registryInflight) {
    return registryInflight;
  }

  registryInflight = runApiRequest(() => PipelinesService.listRegistry(), { label: 'listRegistry', ...options })
    .then((value) => {
      if (cacheMs > 0) {
        registryCache = { fetchedAt: Date.now(), value };
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

export const PipelinesApi = {
  listRegistry: (options?: ApiRequestOptions) => listRegistrySingleflight(options),
  fetchGraph: (args: Parameters<typeof PipelinesService.fetchGraph>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => PipelinesService.fetchGraph(args), { label: 'fetchGraph', ...options }),
  listGraphs: (options?: ApiRequestOptions) =>
    runApiRequest(() => PipelinesService.listGraphs(), { label: 'listGraphs', ...options }),
  listTemplates: (options?: ApiRequestOptions) =>
    runApiRequest(() => PipelinesService.listTemplates(), { label: 'listTemplates', ...options }),
  uploadGraph: async (args: Parameters<typeof PipelinesService.uploadGraph>[0], options?: ApiRequestOptions) => {
    const result = await runApiRequest(() => PipelinesService.uploadGraph(args), { label: 'uploadGraph', ...options });
    invalidateSWRPrefix('pipelines:');
    return result;
  },
  fetchTemplate: (args: Parameters<typeof PipelinesService.fetchTemplate>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => PipelinesService.fetchTemplate(args), { label: 'fetchTemplate', ...options }),
  deleteGraph: async (args: Parameters<typeof PipelinesService.deleteGraph>[0], options?: ApiRequestOptions) => {
    const result = await runApiRequest(() => PipelinesService.deleteGraph(args), { label: 'deleteGraph', ...options });
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
    invalidateSWRPrefix('pipelines:');
    return result;
  },
};
