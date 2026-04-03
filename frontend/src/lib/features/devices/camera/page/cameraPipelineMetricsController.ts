import type {
  PipelineGraphMetrics,
  PipelineNodeRuntimeMetrics,
  StreamManifest,
  StreamMetrics,
  StreamPipelineBinding
} from '$lib/api/client';
import { RAW_PIPELINE_ID, RAW_PIPELINE_UUID } from './cameraPipelineShared';

type UnknownRecord = Record<string, unknown>;
type LegacyPipelineBinding = {
  pipelineId?: string | null;
  id?: string | null;
  pipeline?: { id?: string | null; graph?: unknown } | null;
};

type PipelineMetricsState = {
  get manifestState(): StreamManifest | null;
  get streamMetrics(): StreamMetrics | null;
};

const asRecord = (value: unknown): UnknownRecord | null =>
  value && typeof value === 'object' ? (value as UnknownRecord) : null;

const readString = (record: UnknownRecord | null, ...keys: string[]): string | null => {
  for (const key of keys) {
    const value = record?.[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return null;
};

const pipelineIdFromBinding = (binding: StreamPipelineBinding | LegacyPipelineBinding): string | null => {
  const bindingRecord = asRecord(binding);
  return (
    readString(bindingRecord, 'pipeline_id', 'pipelineId', 'id') ??
    readString(asRecord(bindingRecord?.pipeline), 'id')
  );
};

const manifestBindings = (manifest: StreamManifest | null): Array<StreamPipelineBinding | LegacyPipelineBinding> => {
  if (Array.isArray(manifest?.pipelines)) {
    return manifest.pipelines;
  }
  const legacyBindings = asRecord(manifest)?.pipelines;
  if (Array.isArray(legacyBindings)) {
    return legacyBindings.filter((entry): entry is LegacyPipelineBinding => asRecord(entry) !== null);
  }
  const bindingsRecord = asRecord(legacyBindings);
  if (!bindingsRecord) {
    return [];
  }
  return Object.values(bindingsRecord).filter((entry): entry is LegacyPipelineBinding => asRecord(entry) !== null);
};

const asPipelineGraphMetrics = (value: unknown): PipelineGraphMetrics | null =>
  asRecord(value) ? (value as PipelineGraphMetrics) : null;

export function createCameraPipelineMetricsController(state: PipelineMetricsState) {
  function normalizePipelineIdForMetrics(pipelineId: string | null): string | null {
    if (!pipelineId) return null;
    const normalized = String(pipelineId).trim();
    if (!normalized.length) return null;
    return normalized === RAW_PIPELINE_ID ? RAW_PIPELINE_UUID : normalized;
  }

  function activePipelineWireId(): string | null {
    const manifestRecord = asRecord(state.manifestState);
    const active =
      state.manifestState?.active_pipeline_id ??
      readString(manifestRecord, 'pipeline_id', 'activePipelineId', 'pipelineId');
    return normalizePipelineIdForMetrics(active ?? null);
  }

  function pipelineMetricsForId(metrics: StreamMetrics | null, pipelineId: string | null): PipelineGraphMetrics | null {
    if (!metrics) return null;
    const wireId = normalizePipelineIdForMetrics(pipelineId);
    if (!wireId) return null;
    const instances = asRecord(metrics.pipeline_instances);
    const instanceMetrics = instances?.[wireId];
    const resolvedInstance = asPipelineGraphMetrics(instanceMetrics);
    if (resolvedInstance) {
      return resolvedInstance;
    }
    const active = activePipelineWireId();
    if (active && active === wireId) {
      return metrics.pipeline ?? null;
    }
    // Fallback: when manifest active pipeline lags briefly, still map single-pipeline metrics.
    const bindingIds = manifestBindings(state.manifestState)
      .map((entry) => normalizePipelineIdForMetrics(pipelineIdFromBinding(entry)))
      .filter((id: string | null): id is string => typeof id === 'string' && id.length > 0);
    if (bindingIds.length === 1 && bindingIds[0] === wireId) {
      return metrics.pipeline ?? null;
    }
    return null;
  }

  function findOutputNodeMetrics(
    graphMetrics: PipelineGraphMetrics | null,
    outputKey: string | null
  ): PipelineNodeRuntimeMetrics | null {
    const nodes = graphMetrics?.nodes ?? null;
    if (!nodes) return null;
    const entries = Object.values(nodes);
    if (!entries.length) return null;
    const normalizedOutput = typeof outputKey === 'string' ? outputKey.trim().toLowerCase() : null;
    const outputNodes = entries.filter((node) => {
      const label = typeof node?.node_label === 'string' ? node.node_label.toLowerCase() : '';
      return label.startsWith('output:');
    });
    if (!outputNodes.length) return null;
    if (!normalizedOutput) {
      return outputNodes[0] ?? null;
    }
    const match = outputNodes.find((node) => {
      const label = typeof node?.node_label === 'string' ? node.node_label.toLowerCase() : '';
      return label === `output:${normalizedOutput}`;
    });
    return match ?? outputNodes[0] ?? null;
  }

  function pipelineDurationMs(graphMetrics: PipelineGraphMetrics | null, outputKey: string | null): number | null {
    if (!graphMetrics) return null;
    const nodes = graphMetrics?.nodes ?? null;
    if (!nodes) return null;
    const graphNode = nodes.graph ?? Object.values(nodes).find((node) => node?.node_type === 'graph');
    const graphMetricsRecord = asRecord(graphNode?.metrics);
    const graphMsRaw =
      graphNode?.metrics?.average_time_ms ??
      (typeof graphMetricsRecord?.averageTimeMs === 'number' ? graphMetricsRecord.averageTimeMs : null);
    if (typeof graphMsRaw === 'number' && Number.isFinite(graphMsRaw) && graphMsRaw > 0) {
      return graphMsRaw;
    }
    const values = Object.values(nodes)
      .filter((node) => node?.node_type !== 'graph')
      .map((node) => node?.metrics?.average_time_ms)
      .filter((value): value is number => typeof value === 'number' && Number.isFinite(value));
    if (!values.length) return null;
    const total = values.reduce((sum, value) => sum + value, 0);
    if (total >= 0.5) {
      return total;
    }
    const outputNode = findOutputNodeMetrics(graphMetrics, outputKey);
    const fps = outputNode?.metrics?.average_fps;
    if (typeof fps === 'number' && fps > 0) {
      return 1000 / fps;
    }
    return total;
  }

  function formatDurationMs(value: number | null): string | null {
    if (typeof value !== 'number' || !Number.isFinite(value)) return null;
    if (value >= 10) return `${Math.round(value)} ms`;
    if (value >= 1) return `${value.toFixed(1)} ms`;
    return `${value.toFixed(2)} ms`;
  }

  function outputDurationForPipeline(pipelineId: string | null, outputKey: string | null): string | null {
    const metrics = pipelineMetricsForId(state.streamMetrics, pipelineId);
    const durationMs = pipelineDurationMs(metrics, outputKey);
    return formatDurationMs(durationMs);
  }

  return {
    normalizePipelineIdForMetrics,
    activePipelineWireId,
    pipelineMetricsForId,
    findOutputNodeMetrics,
    pipelineDurationMs,
    formatDurationMs,
    outputDurationForPipeline
  };
}
