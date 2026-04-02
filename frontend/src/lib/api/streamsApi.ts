import { EngineStreamsService } from '$lib/ts-bindings/http/client';
import { apiUrl } from '$lib/api/httpClient';
import { apiFetch, apiFetchCachedJson, runApiRequest, type ApiRequestOptions } from '$lib/api/core/http';
import { DEFAULT_REQUEST_TIMEOUT_MS, fetchWithRetry } from '$lib/api/requestUtils';
import { makeNetcamManifest } from '$lib/api/streamManifestBuilders';
import type { RegisterNetcamStreamInput } from '$lib/api/streamManifestBuilders';
import type {
  CancelablePromise,
  RootStatusPayload,
  StreamCapabilitiesResponse,
  StreamManifest,
  StreamPipelineWire
} from '$lib/ts-bindings/http/client';

type CacheEntry<T> = {
  fetchedAt: number;
  value: T;
  etag?: string | null;
  revision?: number | null;
};

const DEFAULT_STREAMS_CACHE_MS = 750;
const DEFAULT_STREAM_CAPABILITIES_CACHE_MS = 10_000;
type StreamsList = Awaited<ReturnType<typeof EngineStreamsService.listStreams>>;
type StreamCapabilities = Awaited<ReturnType<typeof EngineStreamsService.streamCapabilitiesHandler>>;
type CodecList = Awaited<ReturnType<typeof EngineStreamsService.listCodecs>>;
let streamsCache: CacheEntry<StreamsList> | null = null;
let streamsInflight: Promise<StreamsList> | null = null;
let streamCapabilitiesCache: CacheEntry<StreamCapabilities> | null = null;
let streamCapabilitiesInflight: Promise<StreamCapabilities> | null = null;
let codecInventoryCache: CacheEntry<CodecList> | null = null;
let codecInventoryInflight: Promise<CodecList> | null = null;
let runtimeStatusCache: CacheEntry<RootStatusPayload> | null = null;
let runtimeStatusInflight: Promise<RootStatusPayload> | null = null;

function withAbort<T>(task: (controller: AbortController) => Promise<T>): CancelablePromise<T> {
  const controller = new AbortController();
  const promise = task(controller) as CancelablePromise<T>;
  promise.cancel = () => controller.abort();
  return promise;
}

async function postJsonRequest(
  url: string,
  body: unknown,
  timeoutMs: number,
  controller: AbortController
): Promise<Response> {
  const response = await fetchWithRetry(
    url,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      body: JSON.stringify(body),
      signal: controller.signal
    },
    { timeoutMs, maxAttempts: 1 }
  );
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    const message = text || `Request failed (${response.status})`;
    throw new Error(message);
  }
  return response;
}

function resolveStreamsCacheMs(options?: ApiRequestOptions): number {
  const raw = options?.cacheMs;
  if (typeof raw !== 'number' || !Number.isFinite(raw)) {
    return DEFAULT_STREAMS_CACHE_MS;
  }
  return Math.max(0, Math.floor(raw));
}

function resolveStreamCapabilitiesCacheMs(options?: ApiRequestOptions): number {
  const raw = options?.cacheMs;
  if (typeof raw !== 'number' || !Number.isFinite(raw)) {
    return DEFAULT_STREAM_CAPABILITIES_CACHE_MS;
  }
  return Math.max(0, Math.floor(raw));
}

function primeRuntimeStatusCaches(payload: RootStatusPayload, fetchedAt: number): void {
  streamCapabilitiesCache = { fetchedAt, value: payload.streams.capabilities };
  codecInventoryCache = { fetchedAt, value: payload.streams.codecs };
  streamsCache = {
    fetchedAt,
    value: payload.streams.resolvedStreams,
    etag: streamsCache?.etag ?? null,
    revision: payload.streams.revision
  };
}

async function runtimeStatusSingleflight(options?: ApiRequestOptions): Promise<RootStatusPayload> {
  const cacheMs = resolveStreamCapabilitiesCacheMs(options);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && runtimeStatusCache && now - runtimeStatusCache.fetchedAt < cacheMs) {
    return runtimeStatusCache.value;
  }

  if (runtimeStatusInflight) {
    return runtimeStatusInflight;
  }

  runtimeStatusInflight = apiFetch<RootStatusPayload>(
    apiUrl('/').replace(/\/+$/, ''),
    { headers: { Accept: 'application/json' } },
    { label: 'runtimeStatus', ...options }
  )
    .then((value) => {
      const fetchedAt = Date.now();
      if (cacheMs > 0) {
        runtimeStatusCache = { fetchedAt, value };
        primeRuntimeStatusCaches(value, fetchedAt);
      } else {
        runtimeStatusCache = null;
      }
      return value;
    })
    .finally(() => {
      runtimeStatusInflight = null;
    });

  return runtimeStatusInflight;
}

async function listStreamsSingleflight(options?: ApiRequestOptions) {
  const cacheMs = resolveStreamsCacheMs(options);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && streamsCache && now - streamsCache.fetchedAt < cacheMs) {
    return streamsCache.value;
  }

  if (streamsInflight) {
    return streamsInflight;
  }

  streamsInflight = apiFetchCachedJson<StreamsList>(
    '/streams',
    {
      cached: streamsCache?.value,
      etag: streamsCache?.etag ?? null,
      revision: streamsCache?.revision ?? null
    },
    { headers: { Accept: 'application/json' } },
    { label: 'listStreams', ...options }
  )
    .then((result) => {
      const value = result.status === 'not_modified' ? streamsCache?.value : result.data;
      if (!value) {
        throw new Error('Stream inventory unavailable');
      }
      if (cacheMs > 0) {
        streamsCache = {
          fetchedAt: Date.now(),
          value,
          etag: result.etag ?? streamsCache?.etag ?? null,
          revision: result.revision ?? streamsCache?.revision ?? null
        };
      } else {
        streamsCache = null;
      }
      return value;
    })
    .finally(() => {
      streamsInflight = null;
    });

  return streamsInflight;
}

async function streamCapabilitiesSingleflight(options?: ApiRequestOptions): Promise<StreamCapabilities> {
  const cacheMs = resolveStreamCapabilitiesCacheMs(options);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && streamCapabilitiesCache && now - streamCapabilitiesCache.fetchedAt < cacheMs) {
    return streamCapabilitiesCache.value;
  }

  if (streamCapabilitiesInflight) {
    return streamCapabilitiesInflight;
  }

  streamCapabilitiesInflight = runtimeStatusSingleflight(options)
    .then((payload) => {
      const value = payload.streams.capabilities;
      if (cacheMs <= 0) {
        streamCapabilitiesCache = null;
      }
      return value;
    })
    .finally(() => {
      streamCapabilitiesInflight = null;
    });

  return streamCapabilitiesInflight;
}

async function codecInventorySingleflight(options?: ApiRequestOptions): Promise<CodecList> {
  const cacheMs = resolveStreamCapabilitiesCacheMs(options);
  const now = Date.now();
  if (!options?.forceRefresh && cacheMs > 0 && codecInventoryCache && now - codecInventoryCache.fetchedAt < cacheMs) {
    return codecInventoryCache.value;
  }

  if (codecInventoryInflight) {
    return codecInventoryInflight;
  }

  codecInventoryInflight = runtimeStatusSingleflight(options)
    .then((payload) => {
      const value = payload.streams.codecs;
      if (cacheMs <= 0) {
        codecInventoryCache = null;
      }
      return value;
    })
    .finally(() => {
      codecInventoryInflight = null;
    });

  return codecInventoryInflight;
}

export type { RegisterNetcamStreamInput };

export const StreamsApi = {
  runtimeStatus: (options?: ApiRequestOptions) => runtimeStatusSingleflight(options),
  resolvedStreams: (options?: ApiRequestOptions) =>
    runtimeStatusSingleflight(options).then((payload) => payload.streams.resolvedStreams),
  listStreams: (options?: ApiRequestOptions) => listStreamsSingleflight(options),
  streamCapabilities: (options?: ApiRequestOptions) => streamCapabilitiesSingleflight(options),
  streamFormat: (args: Parameters<typeof EngineStreamsService.streamFormat>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.streamFormat(args), { label: 'streamFormat', ...options }),
  getStream: (args: Parameters<typeof EngineStreamsService.getStream>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.getStream(args), { label: 'getStream', ...options }),
  getMetrics: (args: Parameters<typeof EngineStreamsService.getMetrics>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.getMetrics(args), { label: 'getMetrics', ...options }),
  setPipelineOutput: (args: Parameters<typeof EngineStreamsService.setPipelineOutput>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.setPipelineOutput(args), { label: 'setPipelineOutput', ...options }),
  setPipelineGraph: (args: Parameters<typeof EngineStreamsService.setPipelineGraph>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.setPipelineGraph(args), { label: 'setPipelineGraph', ...options }),
  setPipelineGraphPatch: (
    args: { id: string; requestBody: { patch: unknown; pipeline_id?: string | null } },
    options: ApiRequestOptions = {}
  ) =>
    runApiRequest(
      () =>
        withAbort(async (controller) => {
          const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
          const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/graph/patch`);
          await postJsonRequest(url, args.requestBody, timeoutMs, controller);
        }),
      { label: 'setPipelineGraphPatch', endpoint: `/streams/${args.id}/pipeline/graph/patch`, ...options }
    ),
  setPipelineInputs: (
    args: { id: string; requestBody: { pipeline_id?: string | null; inputs: Record<string, unknown | null> } },
    options: ApiRequestOptions = {}
  ) =>
    runApiRequest(
      () =>
        withAbort(async (controller) => {
          const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
          const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/inputs`);
          await postJsonRequest(url, args.requestBody, timeoutMs, controller);
        }),
      { label: 'setPipelineInputs', endpoint: `/streams/${args.id}/pipeline/inputs`, ...options }
    ),
  setPipelineLayout: (args: Parameters<typeof EngineStreamsService.setPipelineLayout>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.setPipelineLayout(args), { label: 'setPipelineLayout', ...options }),
  setPipelineWires: (
    args: { id: string; requestBody: { wires: StreamPipelineWire[] } },
    options: ApiRequestOptions = {}
  ) =>
    runApiRequest(
      () =>
        withAbort(async (controller) => {
          const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
          const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/wires`);
          await postJsonRequest(url, args.requestBody, timeoutMs, controller);
        }),
      { label: 'setPipelineWires', endpoint: `/streams/${args.id}/pipeline/wires`, ...options }
    ),
  setPipelinePerf: (
    args: { id: string; requestBody: { pipeline_id?: string | null; enabled: boolean } },
    options: ApiRequestOptions = {}
  ) =>
    runApiRequest(
      () =>
        withAbort(async (controller) => {
          const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
          const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/perf`);
          await postJsonRequest(url, args.requestBody, timeoutMs, controller);
        }),
      { label: 'setPipelinePerf', endpoint: `/streams/${args.id}/pipeline/perf`, ...options }
    ),
  resetPipelineMetrics: (
    args: { id: string; requestBody: { pipeline_id?: string | null } },
    options: ApiRequestOptions = {}
  ) =>
    runApiRequest(
      () =>
        withAbort(async (controller) => {
          const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
          const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/metrics/reset`);
          await postJsonRequest(url, args.requestBody, timeoutMs, controller);
        }),
      { label: 'resetPipelineMetrics', endpoint: `/streams/${args.id}/pipeline/metrics/reset`, ...options }
    ),
  profilePipeline: (
    args: {
      id: string;
      requestBody: {
        warmup_ms?: number | null;
        duration_ms?: number | null;
        pipeline_id?: string | null;
        enable_perf_counters?: boolean | null;
        capture_flamegraph?: boolean | null;
        reset_metrics?: boolean | null;
      };
    },
    options: ApiRequestOptions = {}
  ) =>
    (() => {
      // NOTE: `runApiRequest` enforces its own timeout (default 5s). Profiling requests are
      // intentionally long-running, so we must pass the same timeout through to avoid aborting
      // at the request-manager layer.
      const timeoutMs = options.timeoutMs ?? 45_000;
      return runApiRequest(
        () =>
          withAbort(async (controller) => {
            const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/profile`);
            const response = await postJsonRequest(url, args.requestBody, timeoutMs, controller);
            return response.json() as Promise<unknown>;
          }),
        { label: 'profilePipeline', endpoint: `/streams/${args.id}/pipeline/profile`, timeoutMs, ...options }
      );
    })(),
  smokePipelineGraph: (args: Parameters<typeof EngineStreamsService.smokePipelineGraph>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.smokePipelineGraph(args), { label: 'smokePipelineGraph', ...options }),
  listPipelineOutputs: (args: Parameters<typeof EngineStreamsService.listPipelineOutputs>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.listPipelineOutputs(args), { label: 'listPipelineOutputs', ...options }),
  getPipelineOutputSample: (
    args: Parameters<typeof EngineStreamsService.getPipelineOutputSample>[0],
    options?: ApiRequestOptions
  ) => runApiRequest(() => EngineStreamsService.getPipelineOutputSample(args), { label: 'getPipelineOutputSample', ...options }),
  getControls: (args: Parameters<typeof EngineStreamsService.getControls>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.getControls(args), { label: 'getControls', ...options }),
  deleteStream: (args: Parameters<typeof EngineStreamsService.deleteStream>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.deleteStream(args), { label: 'deleteStream', ...options }),
  listBackends: (options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.listBackends(), { label: 'listBackends', ...options }),
  listCodecs: (options?: ApiRequestOptions) => codecInventorySingleflight(options),
  startStream: (args: Parameters<typeof EngineStreamsService.startStream>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.startStream(args), { label: 'startStream', timeoutMs: 45_000, ...options }),
  updateStream: (args: Parameters<typeof EngineStreamsService.updateStream>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.updateStream(args), { label: 'updateStream', timeoutMs: 45_000, ...options }),
  registerNetcamStream: async (input: RegisterNetcamStreamInput, options?: ApiRequestOptions) => {
    const capabilities = await streamCapabilitiesSingleflight(options);
    return runApiRequest(
      () =>
        EngineStreamsService.startStream({
          requestBody: makeNetcamManifest(input, capabilities),
        }),
      { label: 'registerNetcamStream', timeoutMs: 45_000, ...options }
    );
  },
  setControl: (args: Parameters<typeof EngineStreamsService.setControl>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.setControl(args), { label: 'setControl', ...options }),
  updateStreamPose: (args: Parameters<typeof EngineStreamsService.updateStreamPose>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.updateStreamPose(args), { label: 'updateStreamPose', ...options }),
  clearStreamPose: (args: Parameters<typeof EngineStreamsService.clearStreamPose>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.clearStreamPose(args), { label: 'clearStreamPose', ...options }),
};
