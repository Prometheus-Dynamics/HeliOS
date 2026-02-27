import type { StreamMetrics } from '$lib/api/httpClient';
import { RAW_PIPELINE_ID, RAW_PIPELINE_UUID } from './cameraPipelineTuningController';

type PipelineMetricsState = {
  get manifestState(): any;
  get streamMetrics(): StreamMetrics | null;
};

export function createCameraPipelineMetricsController(state: PipelineMetricsState) {
  function normalizePipelineIdForMetrics(pipelineId: string | null): string | null {
    if (!pipelineId) return null;
    const normalized = String(pipelineId).trim();
    if (!normalized.length) return null;
    return normalized === RAW_PIPELINE_ID ? RAW_PIPELINE_UUID : normalized;
  }

  function activePipelineWireId(): string | null {
    const active = (state.manifestState as any)?.active_pipeline_id ?? (state.manifestState as any)?.pipeline_id ?? null;
    if (typeof active !== 'string') return null;
    return normalizePipelineIdForMetrics(active);
  }

  function pipelineMetricsForId(metrics: StreamMetrics | null, pipelineId: string | null): any | null {
    if (!metrics) return null;
    const wireId = normalizePipelineIdForMetrics(pipelineId);
    if (!wireId) return null;
    const instances = (metrics as any)?.pipeline_instances ?? null;
    if (instances && typeof instances === 'object' && wireId in instances) {
      return instances[wireId];
    }
    const active = activePipelineWireId();
    if (active && active === wireId) {
      return (metrics as any).pipeline ?? null;
    }
    // Fallback: when manifest active pipeline lags briefly, still map single-pipeline metrics.
    const bindingsRaw = (state.manifestState as any)?.pipelines;
    const bindings = Array.isArray(bindingsRaw) ? bindingsRaw : [];
    const bindingIds = bindings
      .map((entry: any) => normalizePipelineIdForMetrics(entry?.pipeline_id ?? entry?.pipelineId ?? entry?.id ?? null))
      .filter((id: string | null): id is string => typeof id === 'string' && id.length > 0);
    if (bindingIds.length === 1 && bindingIds[0] === wireId) {
      return (metrics as any).pipeline ?? null;
    }
    return null;
  }

  function findOutputNodeMetrics(graphMetrics: any, outputKey: string | null): any | null {
    const nodes = graphMetrics?.nodes ?? null;
    if (!nodes || typeof nodes !== 'object') return null;
    const entries = Object.values(nodes);
    if (!entries.length) return null;
    const normalizedOutput = typeof outputKey === 'string' ? outputKey.trim().toLowerCase() : null;
    const outputNodes = entries.filter((node: any) => {
      const label = typeof node?.node_label === 'string' ? node.node_label.toLowerCase() : '';
      return label.startsWith('output:');
    });
    if (!outputNodes.length) return null;
    if (!normalizedOutput) {
      return outputNodes[0] ?? null;
    }
    const match = outputNodes.find((node: any) => {
      const label = typeof node?.node_label === 'string' ? node.node_label.toLowerCase() : '';
      return label === `output:${normalizedOutput}`;
    });
    return match ?? outputNodes[0] ?? null;
  }

  function pipelineDurationMs(graphMetrics: any, outputKey: string | null): number | null {
    if (!graphMetrics) return null;
    const nodes = graphMetrics?.nodes ?? null;
    if (!nodes || typeof nodes !== 'object') return null;
    const graphNode = (nodes as any).graph ?? Object.values(nodes).find((node: any) => node?.node_type === 'graph');
    const graphMsRaw = graphNode?.metrics?.average_time_ms ?? graphNode?.metrics?.averageTimeMs ?? null;
    if (typeof graphMsRaw === 'number' && Number.isFinite(graphMsRaw) && graphMsRaw > 0) {
      return graphMsRaw;
    }
    const values = Object.values(nodes)
      .filter((node: any) => node?.node_type !== 'graph')
      .map((node: any) => node?.metrics?.average_time_ms)
      .filter((value: any) => typeof value === 'number' && Number.isFinite(value)) as number[];
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
