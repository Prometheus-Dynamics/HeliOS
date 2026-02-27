import type { PipelineNodeRuntimeMetrics, PipelineStreamNodeMetrics } from '$lib/types/pipeline';
import { cancellableWithTimeout } from '$lib/api/requestUtils';

type RuntimeWithNodeType = PipelineNodeRuntimeMetrics & { nodeType?: string; node_type?: string };

const TUNE_METRICS_REQUEST_TIMEOUT_MS = 2500;

export type TuneMetricsStreamRef = {
  id: string;
  label: string;
};

export type StreamMetricsSummary = {
  streamId: string;
  streamLabel: string;
  nodeCount: number;
  avgFps: number | null;
  avgMs: number | null;
  worstNodeId: string | null;
  worstNodeMs: number | null;
  errorCount: number;
  lastError: string | null;
};

export type TuneMetricsRuntimeDeps = {
  StreamsApi: { getMetrics: (params: { id: string }) => Promise<unknown> };
  buildErrorMessage: (params: { error: unknown; fallback: string }) => string;
  getSelectedPipelineId?: () => string | null;
  getTuneMetricsSnapshots: () => PipelineStreamNodeMetrics[];
  setTuneMetricsSnapshots: (next: PipelineStreamNodeMetrics[]) => void;
  getTuneMetricsStatus: () => 'idle' | 'connecting' | 'connected' | 'error';
  setTuneMetricsStatus: (next: 'idle' | 'connecting' | 'connected' | 'error') => void;
  getTuneMetricsError: () => string | null;
  setTuneMetricsError: (next: string | null) => void;
  getTuneMetricsUpdatedAt: () => number | null;
  setTuneMetricsUpdatedAt: (next: number | null) => void;
  getTuneMetricsRequestId: () => number;
  setTuneMetricsRequestId: (next: number) => void;
  getTuneMetricsInFlight: () => boolean;
  setTuneMetricsInFlight: (next: boolean) => void;
};

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

const numberFromRecord = (record: Record<string, unknown> | null, key: string, fallback = 0): number => {
  const value = record?.[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
};

export const tuneRuntimeFromStats = (
  stats: Record<string, unknown> | null | undefined,
  lastError: string | null = null,
  lastErrorAt: number | null = null
): PipelineNodeRuntimeMetrics => {
  const record = stats ?? null;
  const lastSampleAge = record?.['last_sample_age_ms'];
  return {
    metrics: {
      averageTimeMs: numberFromRecord(record, 'average_time_ms', 0),
      averageFps: numberFromRecord(record, 'average_fps', numberFromRecord(record, 'fps', 0)),
      sampleCount: numberFromRecord(record, 'sample_count', 0),
      windowSize: numberFromRecord(record, 'window_size', 60),
      lastSampleAgeMs: typeof lastSampleAge === 'number' && Number.isFinite(lastSampleAge) ? lastSampleAge : null
    },
    outputEdges: [],
    inputQueues: undefined,
    outputSinks: undefined,
    inputSources: undefined,
    inputSync: undefined,
    lastError,
    lastErrorAt
  };
};

export const buildTuneMetricsSnapshot = (
  stream: TuneMetricsStreamRef,
  metrics: Record<string, unknown> | null,
  options: { error?: string | null; errorAt?: number | null; pipelineId?: string | null } = {}
): PipelineStreamNodeMetrics => {
  const nodeMetrics: Record<string, PipelineNodeRuntimeMetrics> = {};
  const metricsRecord = metrics ?? null;
  nodeMetrics['capture'] = tuneRuntimeFromStats(
    asRecord(metricsRecord?.['capture']),
    options.error ?? null,
    typeof options.errorAt === 'number' ? options.errorAt : null
  );
  nodeMetrics['host'] = tuneRuntimeFromStats(asRecord(metricsRecord?.['host']));
  if (metricsRecord?.['encoder']) {
    nodeMetrics['encoder'] = tuneRuntimeFromStats(asRecord(metricsRecord?.['encoder']));
  }
  if (metricsRecord?.['decoder']) {
    nodeMetrics['decoder'] = tuneRuntimeFromStats(asRecord(metricsRecord?.['decoder']));
  }

  // Prefer per-pipeline metrics when present (multiplex). Keys are pipeline UUID strings.
  const pipelineInstances = asRecord(metricsRecord?.['pipeline_instances']);
  const pipelineFromInstances =
    options.pipelineId && pipelineInstances ? asRecord(pipelineInstances?.[options.pipelineId]) : null;
  const pipeline = pipelineFromInstances ?? asRecord(metricsRecord?.['pipeline']);
  const pipelineNodes = asRecord(pipeline?.['nodes']);
  if (pipelineNodes) {
    Object.entries(pipelineNodes).forEach(([runtimeNodeId, runtime]) => {
      const runtimeRecord = asRecord(runtime);
      const stats = asRecord(runtimeRecord?.['metrics']) ?? {};
      const lastError = typeof runtimeRecord?.['last_error'] === 'string' ? runtimeRecord['last_error'] : null;
      const lastErrorAt = typeof runtimeRecord?.['last_error_at'] === 'number' ? runtimeRecord['last_error_at'] : null;
      nodeMetrics[runtimeNodeId] = tuneRuntimeFromStats(stats, lastError, lastErrorAt);
    });
  }
  return {
    streamId: stream.id,
    streamPath: stream.label,
    metrics: nodeMetrics
  };
};

export const buildPipelineMetricsSummary = (snapshots: PipelineStreamNodeMetrics[]): StreamMetricsSummary[] =>
  snapshots.map((snapshot: PipelineStreamNodeMetrics) => {
    const metricsEntries = Object.entries(snapshot.metrics ?? {});
    const nodeCount = metricsEntries.length;
    let fpsTotal = 0;
    let fpsCount = 0;
    let msTotal = 0;
    let graphMs: number | null = null;
    let worstNodeId: string | null = null;
    let worstNodeMs: number | null = null;
    let errorCount = 0;
    let lastError: string | null = null;

    for (const [nodeId, runtime] of metricsEntries) {
      const runtimeWithType = runtime as RuntimeWithNodeType;
      const nodeType = runtimeWithType?.nodeType ?? runtimeWithType?.node_type ?? null;
      const metrics = (runtime as PipelineNodeRuntimeMetrics).metrics;
      if (nodeId === 'graph' || nodeType === 'graph') {
        if (Number.isFinite(metrics?.averageTimeMs)) {
          graphMs = metrics.averageTimeMs;
        }
        if (runtime?.lastError) {
          errorCount += 1;
          if (!lastError) {
            lastError = runtime.lastError;
          }
        }
        continue;
      }
      if (Number.isFinite(metrics?.averageFps)) {
        fpsTotal += metrics.averageFps;
        fpsCount += 1;
      }
      if (Number.isFinite(metrics?.averageTimeMs)) {
        msTotal += metrics.averageTimeMs;
        if (worstNodeMs === null || metrics.averageTimeMs > worstNodeMs) {
          worstNodeMs = metrics.averageTimeMs;
          worstNodeId = nodeId;
        }
      }
      if (runtime?.lastError) {
        errorCount += 1;
        if (!lastError) {
          lastError = runtime.lastError;
        }
      }
    }

    return {
      streamId: snapshot.streamId,
      streamLabel: snapshot.streamPath?.trim() || snapshot.streamId,
      nodeCount,
      avgFps: fpsCount > 0 ? fpsTotal / fpsCount : null,
      avgMs:
        graphMs && graphMs > 0
          ? graphMs
          : msTotal >= 0.5
            ? msTotal
            : fpsCount > 0
              ? 1000 / (fpsTotal / fpsCount)
              : null,
      worstNodeId,
      worstNodeMs,
      errorCount,
      lastError
    };
  });

export const createTuneMetricsRuntime = (deps: TuneMetricsRuntimeDeps) => {
  const fetchTuneMetricsSnapshots = async (streams: TuneMetricsStreamRef[]): Promise<void> => {
    if (!streams.length) {
      deps.setTuneMetricsSnapshots([]);
      deps.setTuneMetricsStatus('idle');
      deps.setTuneMetricsError(null);
      deps.setTuneMetricsUpdatedAt(null);
      return;
    }
    if (deps.getTuneMetricsInFlight()) return;
    deps.setTuneMetricsInFlight(true);
    const requestId = deps.getTuneMetricsRequestId() + 1;
    deps.setTuneMetricsRequestId(requestId);
    deps.setTuneMetricsStatus('connecting');
    deps.setTuneMetricsError(null);
    try {
      const pipelineId = deps.getSelectedPipelineId?.() ?? null;
      const results = await Promise.allSettled(
        streams.map(async (stream) => {
          const metrics = await cancellableWithTimeout(
            () => deps.StreamsApi.getMetrics({ id: stream.id }),
            TUNE_METRICS_REQUEST_TIMEOUT_MS
          );
          return buildTuneMetricsSnapshot(stream, metrics as Record<string, unknown> | null, { pipelineId });
        })
      );
      if (deps.getTuneMetricsRequestId() !== requestId) return;
      const snapshots: PipelineStreamNodeMetrics[] = [];
      let successCount = 0;
      let errorMessage: string | null = null;
      results.forEach((result, index) => {
        if (result.status === 'fulfilled') {
          snapshots.push(result.value);
          successCount += 1;
        } else {
          const stream = streams[index];
          const message = deps.buildErrorMessage({
            error: result.reason,
            fallback: `Unable to load metrics for ${stream?.label || stream?.id || 'stream'}.`
          });
          if (!errorMessage) errorMessage = message;
          if (stream) {
            snapshots.push(buildTuneMetricsSnapshot(stream, null, { error: message, errorAt: Date.now(), pipelineId }));
          }
        }
      });
      deps.setTuneMetricsSnapshots(snapshots);
      // Treat partial success as connected so the UI doesn't get stuck on "Offline".
      deps.setTuneMetricsStatus(successCount > 0 ? 'connected' : snapshots.length > 0 ? 'error' : errorMessage ? 'error' : 'idle');
      deps.setTuneMetricsError(errorMessage);
      deps.setTuneMetricsUpdatedAt(Date.now());
    } finally {
      deps.setTuneMetricsInFlight(false);
    }
  };

  return {
    fetchTuneMetricsSnapshots
  };
};
