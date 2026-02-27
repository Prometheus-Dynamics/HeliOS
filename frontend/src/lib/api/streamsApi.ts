import { EngineStreamsService } from '$lib/ts-bindings/http/client';
import { apiUrl } from '$lib/api/httpClient';
import { runApiRequest, type ApiRequestOptions } from '$lib/api/requestManager';
import { DEFAULT_REQUEST_TIMEOUT_MS, fetchWithRetry } from '$lib/api/requestUtils';
import type { StreamManifest } from '$lib/ts-bindings/http/client';

type CacheEntry<T> = {
  fetchedAt: number;
  value: T;
};

const DEFAULT_STREAMS_CACHE_MS = 750;
const RAW_STREAM_PIPELINE_UUID = '00000000-0000-0000-0000-0000000000aa';
type StreamsList = Awaited<ReturnType<typeof EngineStreamsService.listStreams>>;
let streamsCache: CacheEntry<StreamsList> | null = null;
let streamsInflight: Promise<StreamsList> | null = null;

function resolveStreamsCacheMs(options?: ApiRequestOptions): number {
  const raw = options?.cacheMs;
  if (typeof raw !== 'number' || !Number.isFinite(raw)) {
    return DEFAULT_STREAMS_CACHE_MS;
  }
  return Math.max(0, Math.floor(raw));
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

  streamsInflight = runApiRequest(() => EngineStreamsService.listStreams(), { label: 'listStreams', ...options })
    .then((value) => {
      if (cacheMs > 0) {
        streamsCache = { fetchedAt: Date.now(), value };
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

export type RegisterNetcamStreamInput = {
  url: string;
  name?: string | null;
  fps?: number | null;
  width?: number | null;
  height?: number | null;
  startOnBoot?: boolean | null;
};

function makeNetcamManifest(input: RegisterNetcamStreamInput): StreamManifest {
  const url = input.url.trim();
  const fps = typeof input.fps === 'number' && Number.isFinite(input.fps) ? Math.max(1, Math.min(120, Math.trunc(input.fps))) : 30;
  const width = typeof input.width === 'number' && Number.isFinite(input.width) ? Math.max(0, Math.trunc(input.width)) : 0;
  const height = typeof input.height === 'number' && Number.isFinite(input.height) ? Math.max(0, Math.trunc(input.height)) : 0;
  const alias = input.name?.trim().length ? input.name.trim() : `Netcam ${url}`;

  // The backend's `StreamManifest.identity` is `DeviceIdentity { id, alias, hardware_id }`.
  // TS bindings may lag, so cast to `any` here to keep runtime JSON correct.
  return {
    identity: { id: null, alias, hardware_id: null } as any,
    capture: {
      device_keys: [url],
      backend: 'Netcam',
      handle: {
        // styx serializes BackendHandle as an internally-tagged JSON enum (`{ type: "netcam", ... }`).
        // The OpenAPI union can lag behind that serde representation, so keep the runtime shape correct.
        type: 'netcam',
        url,
        width,
        height,
        fps
      },
      mode: {
        format: {
          code: 'MJPG',
          color: 'Srgb',
          resolution: { width: Math.max(1, width || 1), height: Math.max(1, height || 1) },
        },
        interval: { numerator: 1, denominator: fps },
      },
      interval: { numerator: 1, denominator: fps },
      controls: [],
    },
    pipeline_enabled: true,
    active_pipeline_id: RAW_STREAM_PIPELINE_UUID,
    active_pipeline_output: 'raw',
    pipelines: [{ pipeline_id: RAW_STREAM_PIPELINE_UUID, pipeline_graph: null, pipeline_output: 'raw' }],
    pipeline_layout: {
      rows: 1,
      columns: 1,
      slots: [{ row: 0, column: 0, pipeline_id: RAW_STREAM_PIPELINE_UUID, output_key: 'raw' }]
    },
    start_on_boot: Boolean(input.startOnBoot),
  } as any;
}

export const StreamsApi = {
  listStreams: (options?: ApiRequestOptions) => listStreamsSingleflight(options),
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
    runApiRequest(() => {
      const controller = new AbortController();
      const promise = (async () => {
        const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
        const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/graph/patch`);
        const response = await fetchWithRetry(
          url,
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
            body: JSON.stringify(args.requestBody),
            signal: controller.signal
          },
          { timeoutMs, maxAttempts: 1 }
        );
        if (!response.ok) {
          const text = await response.text().catch(() => '');
          const message = text || `Request failed (${response.status})`;
          throw new Error(message);
        }
        return;
      })();
      (promise as any).cancel = () => controller.abort();
      return promise as any;
    }, { label: 'setPipelineGraphPatch', endpoint: `/streams/${args.id}/pipeline/graph/patch`, ...options }),
  setPipelineInputs: (
    args: { id: string; requestBody: { pipeline_id?: string | null; inputs: Record<string, unknown | null> } },
    options: ApiRequestOptions = {}
  ) =>
    runApiRequest(() => {
      const controller = new AbortController();
      const promise = (async () => {
        const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
        const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/inputs`);
        const response = await fetchWithRetry(
          url,
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
            body: JSON.stringify(args.requestBody),
            signal: controller.signal
          },
          { timeoutMs, maxAttempts: 1 }
        );
        if (!response.ok) {
          const text = await response.text().catch(() => '');
          const message = text || `Request failed (${response.status})`;
          throw new Error(message);
        }
        return;
      })();
      (promise as any).cancel = () => controller.abort();
      return promise as any;
    }, { label: 'setPipelineInputs', endpoint: `/streams/${args.id}/pipeline/inputs`, ...options }),
  setPipelineLayout: (args: Parameters<typeof EngineStreamsService.setPipelineLayout>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.setPipelineLayout(args), { label: 'setPipelineLayout', ...options }),
  setPipelineWires: (
    args: { id: string; requestBody: { wires: any[] } },
    options: ApiRequestOptions = {}
  ) =>
    runApiRequest(() => {
      const controller = new AbortController();
      const promise = (async () => {
        const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
        const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/wires`);
        const response = await fetchWithRetry(
          url,
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
            body: JSON.stringify(args.requestBody),
            signal: controller.signal
          },
          { timeoutMs, maxAttempts: 1 }
        );
        if (!response.ok) {
          const text = await response.text().catch(() => '');
          const message = text || `Request failed (${response.status})`;
          throw new Error(message);
        }
        return;
      })();
      (promise as any).cancel = () => controller.abort();
      return promise as any;
    }, { label: 'setPipelineWires', endpoint: `/streams/${args.id}/pipeline/wires`, ...options }),
  setPipelinePerf: (
    args: { id: string; requestBody: { pipeline_id?: string | null; enabled: boolean } },
    options: ApiRequestOptions = {}
  ) =>
    runApiRequest(() => {
      const controller = new AbortController();
      const promise = (async () => {
        const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
        const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/perf`);
        const response = await fetchWithRetry(
          url,
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
            body: JSON.stringify(args.requestBody),
            signal: controller.signal
          },
          { timeoutMs, maxAttempts: 1 }
        );
        if (!response.ok) {
          const text = await response.text().catch(() => '');
          const message = text || `Request failed (${response.status})`;
          throw new Error(message);
        }
        return;
      })();
      (promise as any).cancel = () => controller.abort();
      return promise as any;
    }, { label: 'setPipelinePerf', endpoint: `/streams/${args.id}/pipeline/perf`, ...options }),
  resetPipelineMetrics: (
    args: { id: string; requestBody: { pipeline_id?: string | null } },
    options: ApiRequestOptions = {}
  ) =>
    runApiRequest(() => {
      const controller = new AbortController();
      const promise = (async () => {
        const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
        const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/metrics/reset`);
        const response = await fetchWithRetry(
          url,
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
            body: JSON.stringify(args.requestBody),
            signal: controller.signal
          },
          { timeoutMs, maxAttempts: 1 }
        );
        if (!response.ok) {
          const text = await response.text().catch(() => '');
          const message = text || `Request failed (${response.status})`;
          throw new Error(message);
        }
        return;
      })();
      (promise as any).cancel = () => controller.abort();
      return promise as any;
    }, { label: 'resetPipelineMetrics', endpoint: `/streams/${args.id}/pipeline/metrics/reset`, ...options }),
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
        () => {
          const controller = new AbortController();
          const promise = (async () => {
            const url = apiUrl(`/streams/${encodeURIComponent(args.id)}/pipeline/profile`);
            const response = await fetchWithRetry(
              url,
              {
                method: 'POST',
                headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
                body: JSON.stringify(args.requestBody),
                signal: controller.signal
              },
              { timeoutMs, maxAttempts: 1 }
            );
            if (!response.ok) {
              const text = await response.text().catch(() => '');
              const message = text || `Request failed (${response.status})`;
              throw new Error(message);
            }
            return (await response.json()) as any;
          })();
          (promise as any).cancel = () => controller.abort();
          return promise as any;
        },
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
  streamFormat: (args: Parameters<typeof EngineStreamsService.streamFormat>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.streamFormat(args), { label: 'streamFormat', ...options }),
  getControls: (args: Parameters<typeof EngineStreamsService.getControls>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.getControls(args), { label: 'getControls', ...options }),
  deleteStream: (args: Parameters<typeof EngineStreamsService.deleteStream>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.deleteStream(args), { label: 'deleteStream', ...options }),
  listBackends: (options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.listBackends(), { label: 'listBackends', ...options }),
  listCodecs: (options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.listCodecs(), { label: 'listCodecs', ...options }),
  startStream: (args: Parameters<typeof EngineStreamsService.startStream>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.startStream(args), { label: 'startStream', timeoutMs: 45_000, ...options }),
  registerNetcamStream: (input: RegisterNetcamStreamInput, options?: ApiRequestOptions) =>
    runApiRequest(
      () =>
        EngineStreamsService.startStream({
          requestBody: makeNetcamManifest(input),
        }),
      { label: 'registerNetcamStream', timeoutMs: 45_000, ...options }
    ),
  setControl: (args: Parameters<typeof EngineStreamsService.setControl>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.setControl(args), { label: 'setControl', ...options }),
  updateStreamPose: (args: Parameters<typeof EngineStreamsService.updateStreamPose>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.updateStreamPose(args), { label: 'updateStreamPose', ...options }),
  clearStreamPose: (args: Parameters<typeof EngineStreamsService.clearStreamPose>[0], options?: ApiRequestOptions) =>
    runApiRequest(() => EngineStreamsService.clearStreamPose(args), { label: 'clearStreamPose', ...options }),
};
