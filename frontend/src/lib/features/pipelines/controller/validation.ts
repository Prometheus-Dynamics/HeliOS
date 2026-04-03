import { createPipelineValidationWorker } from '$lib/workers/factories';
import type {
  PipelineDiagnosticWarning,
  PipelineGraphPlan,
  PipelineOverviewPipeline
} from '$lib/types/pipeline';
import { resolveNodeOrder } from '../daedalusGraph';
import { serializeGraphPlan } from '../graph';
import { validatePipelineGraph } from './io';
import { reportError } from '$lib/ui/errorPolicy';
import type { PipelineMetricsState } from './pipelineMetrics';
import { get, type Readable, type Writable } from 'svelte/store';

type ValidationMessageMap = Record<string, { ok: boolean; warnings: string[] }>;
type PipelineStoreLike = {
  update: (
    id: string,
    updater: (pipeline: PipelineOverviewPipeline) => PipelineOverviewPipeline
  ) => void;
};
type ToasterLike = {
  success: (payload: { title: string; description: string }) => void;
};

const RUNTIME_WARNING_PREFIX = 'RuntimeError:';

function resolveDiagnosticNodeId(
  plan: PipelineGraphPlan,
  spanNode: string | null,
  spanPort: string | null
): string | null {
  if (!spanNode) return null;
  const trimmed = spanNode.trim();
  if (!trimmed) return null;
  if (plan.format === 'daedalus' && /^\d+$/u.test(trimmed)) {
    const index = Number.parseInt(trimmed, 10);
    if (Number.isFinite(index)) {
      const order = resolveNodeOrder(plan);
      const candidate = order[index];
      if (candidate) return candidate;
    }
  }
  if (plan.nodes?.[trimmed]) return trimmed;
  const normalized = trimmed.toLowerCase();
  const candidates = Object.entries(plan.nodes ?? {}).filter(([, node]) => {
    const backendId = node.backendId?.trim().toLowerCase();
    const nodeId = node.id?.trim().toLowerCase();
    return normalized === backendId || normalized === nodeId;
  });
  if (candidates.length === 0) {
    if (spanPort) {
      const normalizedPort = spanPort.trim().toLowerCase();
      const byPort = Object.entries(plan.nodes ?? {}).filter(([, node]) => {
        const inputs = Object.keys(node.inputs ?? {}).some((key) => key.trim().toLowerCase() === normalizedPort);
        const outputs = Object.keys(node.outputs ?? {}).some((key) => key.trim().toLowerCase() === normalizedPort);
        return inputs || outputs;
      });
      if (byPort.length === 1) return byPort[0]?.[0] ?? null;
    }
    return null;
  }
  if (candidates.length === 1) return candidates[0]?.[0] ?? null;
  if (spanPort) {
    const normalizedPort = spanPort.trim().toLowerCase();
    const byPort = candidates.filter(([, node]) => {
      const inputs = Object.keys(node.inputs ?? {}).some((key) => key.trim().toLowerCase() === normalizedPort);
      const outputs = Object.keys(node.outputs ?? {}).some((key) => key.trim().toLowerCase() === normalizedPort);
      return inputs || outputs;
    });
    if (byPort.length === 1) return byPort[0]?.[0] ?? null;
    if (byPort.length > 0) return byPort[0]?.[0] ?? null;
  }
  return candidates[0]?.[0] ?? null;
}

function buildRuntimeDiagnosticWarnings(state: PipelineMetricsState | null | undefined): PipelineDiagnosticWarning[] {
  const snapshots = state?.metrics ?? null;
  if (!snapshots || snapshots.length === 0) return [];
  const out: PipelineDiagnosticWarning[] = [];
  const seen = new Set<string>();

  for (const snapshot of snapshots) {
    const streamLabelRaw = String(snapshot?.streamPath ?? snapshot?.streamId ?? '').trim();
    const streamLabel = streamLabelRaw || 'unknown_stream';

    const pushWarning = (nodeIdRaw: string, errorRaw: string) => {
      const nodeId = String(nodeIdRaw ?? '').trim();
      const error = String(errorRaw ?? '').trim();
      if (!nodeId || !error) return;
      const message = `${RUNTIME_WARNING_PREFIX} ${streamLabel}: ${error}`;
      const key = `${streamLabel}::${nodeId}::${error}`;
      if (seen.has(key)) return;
      seen.add(key);
      out.push({ message, nodeId });
    };

    for (const [nodeId, metrics] of Object.entries(snapshot?.metrics ?? {})) {
      const err = metrics?.lastError ?? null;
      if (typeof err === 'string' && err.trim()) pushWarning(nodeId, err);
    }
    for (const [nodeId, metrics] of Object.entries(snapshot?.groups ?? {})) {
      const err = metrics?.lastError ?? null;
      if (typeof err === 'string' && err.trim()) pushWarning(nodeId, err);
    }
  }

  return out;
}

function isRuntimeDiagnosticWarning(warning: PipelineDiagnosticWarning | null | undefined): boolean {
  const message = typeof warning?.message === 'string' ? warning.message : '';
  return message.startsWith(RUNTIME_WARNING_PREFIX);
}

function mergeRuntimeDiagnostics(
  existing: PipelineOverviewPipeline['diagnostics'] | null | undefined,
  runtimeWarnings: PipelineDiagnosticWarning[],
): PipelineOverviewPipeline['diagnostics'] | null {
  const baseWarnings = (existing?.warnings ?? []).filter((warning) => !isRuntimeDiagnosticWarning(warning));
  const mergedWarnings = [...baseWarnings, ...runtimeWarnings];
  const error = existing?.error ?? null;
  if (mergedWarnings.length === 0 && !error) return null;
  return {
    warnings: mergedWarnings,
    ...(error ? { error } : {})
  };
}

export function createRuntimeDiagnosticsSubscription(args: {
  pipelineMetricsState: Readable<Record<string, PipelineMetricsState>>;
  pipelineStore: PipelineStoreLike;
  clonePipeline: (pipeline: PipelineOverviewPipeline) => PipelineOverviewPipeline;
}) {
  const { pipelineMetricsState, pipelineStore, clonePipeline } = args;
  const runtimeDiagnosticsSignature = new Map<string, string>();

  return pipelineMetricsState.subscribe(($metrics) => {
    for (const [pipelineId, state] of Object.entries($metrics ?? {})) {
      const isTeardownReset =
        state?.status === 'idle' &&
        !state?.metrics &&
        !state?.error &&
        state?.updatedAt == null;
      if (isTeardownReset) continue;
      const runtimeWarnings = buildRuntimeDiagnosticWarnings(state);
      const signature = runtimeWarnings
        .map((warning) => `${warning.nodeId ?? ''}|${warning.port ?? ''}|${warning.message}`)
        .sort()
        .join('\n');
      if (runtimeDiagnosticsSignature.get(pipelineId) === signature) continue;
      runtimeDiagnosticsSignature.set(pipelineId, signature);
      pipelineStore.update(pipelineId, (current) => {
        const clone = clonePipeline(current);
        clone.diagnostics = mergeRuntimeDiagnostics(clone.diagnostics ?? null, runtimeWarnings);
        return clone;
      });
    }
  });
}

export function createPipelineValidationRuntime(args: {
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  pipelineStore: PipelineStoreLike;
  validationMessages: Writable<ValidationMessageMap>;
  clonePipeline: (pipeline: PipelineOverviewPipeline) => PipelineOverviewPipeline;
  toaster: ToasterLike;
}) {
  const { selectedPipeline, pipelineStore, validationMessages, clonePipeline, toaster } = args;
  let validationWorker: Worker | null = null;
  let validationRequestId = 0;
  const validationResolvers = new Map<number, (warnings: string[]) => void>();

  async function formatValidationWarnings(
    diagnostics: Array<{ code: string; message: string; span?: { node?: string | null; port?: string | null } | null }>
  ): Promise<string[]> {
    const formatFallback = () =>
      diagnostics.map((diag) => {
        const nodeId = diag?.span?.node ?? null;
        const port = diag?.span?.port ?? null;
        const at = nodeId ? ` (node ${nodeId}${port ? `:${port}` : ''})` : '';
        return `${diag.code}: ${diag.message}${at}`;
      });

    if (typeof Worker === 'undefined') return formatFallback();
    if (!validationWorker) {
      validationWorker = createPipelineValidationWorker();
      validationWorker.onmessage = (event) => {
        const payload = event.data as { requestId: number; warnings?: string[] };
        const resolver = validationResolvers.get(payload.requestId);
        if (!resolver) return;
        validationResolvers.delete(payload.requestId);
        resolver(Array.isArray(payload.warnings) ? payload.warnings : []);
      };
    }
    const requestId = ++validationRequestId;
    const result = new Promise<string[]>((resolve) => {
      validationResolvers.set(requestId, resolve);
    });
    validationWorker.postMessage({ requestId, diagnostics });
    return result;
  }

  function clearValidation(pipelineId: string) {
    validationMessages.update((map) => {
      if (!map[pipelineId]) return map;
      const next = { ...map };
      delete next[pipelineId];
      return next;
    });
    pipelineStore.update(pipelineId, (pipeline) => {
      const clone = clonePipeline(pipeline);
      clone.diagnostics = null;
      return clone;
    });
  }

  async function validateCurrentPipeline(
    planOverride?: PipelineGraphPlan | null,
    options?: { quiet?: boolean }
  ) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    const plan = planOverride ?? pipeline.graph;
    try {
      const response = await validatePipelineGraph({
        graph: serializeGraphPlan(plan),
        enable_lints: true,
        active_features: [],
        graph_id: pipeline.id
      });
      const diagnostics = Array.isArray(response?.diagnostics) ? response.diagnostics : [];
      const warnings = await formatValidationWarnings(diagnostics);
      const nodeIds = Array.isArray(response?.node_ids)
        ? response.node_ids.filter((id): id is string => typeof id === 'string' && id.trim().length > 0)
        : [];
      const gpuSegments = Array.isArray(response?.gpu_segments)
        ? response.gpu_segments
            .map((segment) => {
              const bufferId = typeof segment?.buffer_id === 'number' && Number.isFinite(segment.buffer_id) ? segment.buffer_id : null;
              const nodes = Array.isArray(segment?.nodes)
                ? segment.nodes
                    .map((idx) => (typeof idx === 'number' && Number.isInteger(idx) && idx >= 0 ? nodeIds[idx] ?? null : null))
                    .filter((id): id is string => typeof id === 'string' && id.trim().length > 0)
                : [];
              if (bufferId == null || nodes.length === 0) return null;
              return { bufferId, nodes };
            })
            .filter((segment): segment is { bufferId: number; nodes: string[] } => Boolean(segment))
        : [];
      const gpuEdges = Array.isArray(response?.gpu_edges)
        ? response.gpu_edges
            .map((edge) => {
              const edgeIndex = typeof edge?.edge_index === 'number' && Number.isFinite(edge.edge_index) ? edge.edge_index : null;
              if (edgeIndex == null) return null;
              return {
                edgeIndex,
                gpuFastPath: Boolean(edge?.gpu_fast_path),
                bufferId: typeof edge?.buffer_id === 'number' && Number.isFinite(edge.buffer_id) ? edge.buffer_id : null
              };
            })
            .filter((edge): edge is { edgeIndex: number; gpuFastPath: boolean; bufferId: number | null } => Boolean(edge))
        : [];
      const diagnosticWarnings = diagnostics
        .map((diag) => {
          const code = typeof diag?.code === 'string' ? diag.code.trim() : '';
          const message = typeof diag?.message === 'string' ? diag.message.trim() : '';
          const nodeIdRaw = typeof diag?.span?.node === 'string' ? diag.span.node.trim() : '';
          const port = typeof diag?.span?.port === 'string' ? diag.span.port.trim() : '';
          const nodeId = resolveDiagnosticNodeId(plan, nodeIdRaw, port);
          const summary = code && message ? `${code}: ${message}` : message || code;
          if (!summary) return null;
          return {
            message: summary,
            ...(nodeId ? { nodeId } : {}),
            ...(port ? { port } : {})
          };
        })
        .filter(Boolean);

      pipelineStore.update(pipeline.id, (current) => {
        const clone = clonePipeline(current);
        clone.diagnostics = {
          warnings: diagnosticWarnings as Array<{ message: string; nodeId?: string; port?: string }>,
          gpu: {
            segments: gpuSegments,
            edges: gpuEdges
          },
          ...(!response.ok ? { error: 'Graph failed validation' } : {})
        };
        return clone;
      });
      validationMessages.update((map) => ({
        ...map,
        [pipeline.id]: { ok: Boolean(response.ok), warnings }
      }));
      if (!options?.quiet) {
        toaster.success({
          title: 'Validation completed',
          description: response.ok ? 'No issues detected' : `${warnings.length} warnings`
        });
      }
    } catch (error) {
      const message = (error as Error)?.message ?? 'Unknown error';
      if (!options?.quiet) {
        reportError({
          title: 'Validation failed',
          error,
          fallback: message
        });
      }
    }
  }

  function disposeValidation() {
    if (validationWorker) {
      validationWorker.terminate();
      validationWorker = null;
    }
    validationResolvers.clear();
  }

  return {
    clearValidation,
    validateCurrentPipeline,
    disposeValidation
  };
}
