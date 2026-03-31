import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
import type { StreamInfo } from '$lib/ts-bindings/http/client';
import type { PipelineGraphPlan } from '$lib/types/pipeline';
import { normalizeMultiplexSlots, multiplexKey } from './pipelineMultiplexUtils';

type TuneMultiplexLayout = {
  rows: number;
  columns: number;
  slots: Array<{ row: number; column: number; pipeline_id: string | null; output_key: string | null }>;
};

export type TuneMultiplexApplyDeps = {
  browser: boolean;
  RAW_STREAM_PIPELINE_ID: string;
  RAW_STREAM_PIPELINE_UUID: string;
  getTunePreviewStream: () => StreamInfo | null;
  getTuneMultiplexRows: () => number;
  getTuneMultiplexColumns: () => number;
  getTuneMultiplexSlots: () => Record<string, string | null>;
  setTuneMultiplexSlots: (next: Record<string, string | null>) => void;
  getTuneMultiplexSlotOutputs: () => Record<string, string | null>;
  setTuneMultiplexSlotOutputs: (next: Record<string, string | null>) => void;
  getTuneMultiplexGridIsSingle: () => boolean;
  getTuneMultiplexAutoApplyTimer: () => number | null;
  setTuneMultiplexAutoApplyTimer: (timer: number | null) => void;
  getTuneMultiplexBusy: () => boolean;
  setTuneMultiplexBusy: (busy: boolean) => void;
  setTuneMultiplexError: (message: string | null) => void;
  setTuneMultiplexDirty: (dirty: boolean) => void;
  setTuneMultiplexLastAppliedSignature: (signature: string | null) => void;
  setTuneMultiplexHydratedSignature: (signature: string | null) => void;
  setTuneMultiplexHydratedStreamId: (streamId: string | null) => void;
  outputOptionsForPipeline: (pipelineId: string | null) => string[];
  resolvePipelineLabel: (pipelineId: string | null) => string;
  pipelines: { get: () => Array<{ id: string; graph?: PipelineGraphPlan | null }> };
  serializeGraphPlan: (plan: PipelineGraphPlan) => unknown;
  StreamsApi: Pick<typeof SharedStreamsApi, 'setPipelineLayout' | 'smokePipelineGraph' | 'setPipelineGraph' | 'setPipelineOutput'>;
  toaster: { success: (payload: { title: string }) => void; error: (payload: { title: string; description?: string }) => void };
  reportError: (params: { title: string; error: unknown; fallback: string }) => void;
  buildErrorMessage: (params: { error: unknown; fallback: string }) => string;
  fetchTuneStreams: () => Promise<StreamInfo[]>;
  buildTuneMultiplexStateSignature: () => string;
};

export const createTuneMultiplexApply = (deps: TuneMultiplexApplyDeps) => {
  const buildTuneMultiplexLayout = (): TuneMultiplexLayout | null => {
    const rows = Math.min(6, Math.max(1, Math.trunc(deps.getTuneMultiplexRows())));
    const columns = Math.min(6, Math.max(1, Math.trunc(deps.getTuneMultiplexColumns())));
    const normalizedSlots = normalizeMultiplexSlots(rows, columns, deps.getTuneMultiplexSlots());
    deps.setTuneMultiplexSlots(normalizedSlots);
    const normalizedOutputs = normalizeMultiplexSlots(rows, columns, deps.getTuneMultiplexSlotOutputs());
    deps.setTuneMultiplexSlotOutputs(normalizedOutputs);
    const slots = Object.entries(normalizedSlots)
      .map(([key, pipelineId]) => {
        if (!pipelineId) return null;
        const [rowRaw, columnRaw] = key.split(':');
        const row = Math.trunc(Number(rowRaw));
        const column = Math.trunc(Number(columnRaw));
        if (!Number.isInteger(row) || !Number.isInteger(column)) return null;
        const outputOptions = deps.outputOptionsForPipeline(pipelineId);
        const fallbackOutput = outputOptions[0] ?? null;
        const output_key = ((normalizedOutputs[key] ?? fallbackOutput) || null) as string | null;
        if (pipelineId === deps.RAW_STREAM_PIPELINE_ID) {
          return { row, column, pipeline_id: deps.RAW_STREAM_PIPELINE_UUID, output_key };
        }
        return { row, column, pipeline_id: pipelineId, output_key };
      })
      .filter(Boolean);
    return { rows, columns, slots: slots as Array<{ row: number; column: number; pipeline_id: string | null; output_key: string | null }> };
  };

  const resolveTunePipelineGraph = (stream: StreamInfo, pipelineId: string): unknown | null => {
    const manifest = stream.manifest ?? null;
    const binding = Array.isArray(manifest?.pipelines) ? manifest.pipelines.find((entry) => entry?.pipeline_id === pipelineId) : null;
    if (binding?.pipeline_graph) {
      return binding.pipeline_graph;
    }
    const pipeline = deps.pipelines.get().find((entry) => entry.id === pipelineId) ?? null;
    if (pipeline?.graph) {
      return deps.serializeGraphPlan(pipeline.graph);
    }
    return null;
  };

  const resolveTunePipelineOutput = (pipelineId: string): string | null => {
    if (pipelineId === deps.RAW_STREAM_PIPELINE_ID) return null;
    const outputs = deps.outputOptionsForPipeline(pipelineId);
    const fallback = outputs[0] ?? null;
    if (deps.getTuneMultiplexGridIsSingle()) {
      const key = multiplexKey(0, 0);
      const selected = deps.getTuneMultiplexSlotOutputs()[key] ?? null;
      return typeof selected === 'string' && selected.trim().length ? selected.trim() : fallback;
    }
    const slotOutputs = Object.entries(deps.getTuneMultiplexSlots())
      .filter(([, id]) => id === pipelineId)
      .map(([key]) => deps.getTuneMultiplexSlotOutputs()[key])
      .filter((value): value is string => typeof value === 'string' && value.trim().length > 0);
    return slotOutputs[0] ?? fallback;
  };

  const boundTunePipelines = (stream: StreamInfo): Set<string> => {
    const manifest = stream.manifest ?? null;
    const bound = new Set<string>();
    const add = (value: unknown) => {
      const trimmed = typeof value === 'string' ? value.trim() : '';
      if (trimmed) bound.add(trimmed);
    };
    add(manifest?.active_pipeline_id);
    if (Array.isArray(manifest?.pipelines)) {
      manifest.pipelines.forEach((binding) => add(binding?.pipeline_id));
    }
    return bound;
  };

  const ensureTunePipelinesApplied = async (stream: StreamInfo): Promise<boolean> => {
    const required = new Set<string>();
    Object.values(deps.getTuneMultiplexSlots()).forEach((pipelineId) => {
      if (!pipelineId || pipelineId === deps.RAW_STREAM_PIPELINE_ID) return;
      required.add(pipelineId);
    });
    if (required.size === 0) return true;
    const bound = boundTunePipelines(stream);
    for (const pipelineId of required) {
      if (bound.has(pipelineId)) continue;
      const graph = resolveTunePipelineGraph(stream, pipelineId);
      if (!graph) {
        deps.setTuneMultiplexError(`Unable to load graph for pipeline ${pipelineId}.`);
        return false;
      }
      try {
        const output = resolveTunePipelineOutput(pipelineId);
        await deps.StreamsApi.setPipelineGraph({
          id: stream.id,
          requestBody: { graph, pipeline_id: pipelineId, output }
        });
      } catch (error) {
        const message = deps.buildErrorMessage({
          error,
          fallback: `Unable to attach pipeline ${deps.resolvePipelineLabel(pipelineId)}.`
        });
        deps.reportError({
          title: 'Pipeline graph apply failed',
          error,
          fallback: message
        });
        deps.setTuneMultiplexError(
          message
        );
        return false;
      }
    }
    return true;
  };

  const setTuneLivePipelineOutput = async (output: string | null): Promise<void> => {
    const stream = deps.getTunePreviewStream();
    if (!stream) return;
    try {
      await deps.StreamsApi.setPipelineOutput({ id: stream.id, requestBody: { output } });
    } catch (error) {
      console.warn('Failed to set pipeline output', error);
      deps.reportError({
        title: 'Pipeline output failed',
        error,
        fallback: 'Unable to update pipeline output right now.'
      });
    }
  };

  const applyTuneMultiplex = async (options: { quiet?: boolean } = {}): Promise<void> => {
    if (!deps.browser) return;
    const stream = deps.getTunePreviewStream();
    if (!stream) return;
    deps.setTuneMultiplexBusy(true);
    deps.setTuneMultiplexError(null);
    const existing = deps.getTuneMultiplexAutoApplyTimer();
    if (existing) {
      clearTimeout(existing);
      deps.setTuneMultiplexAutoApplyTimer(null);
    }
    try {
      const streamId = stream.id;
      const nextLayout = buildTuneMultiplexLayout();
      const signature = deps.buildTuneMultiplexStateSignature();
      if (!nextLayout) {
        await deps.StreamsApi.setPipelineLayout({ id: streamId, requestBody: { pipeline_layout: null } });
      } else {
        const ensured = await ensureTunePipelinesApplied(stream);
        if (!ensured) return;
        await deps.StreamsApi.setPipelineLayout({ id: streamId, requestBody: { pipeline_layout: nextLayout } });
      }
      const smoke = await deps.StreamsApi.smokePipelineGraph({ id: streamId, timeoutMs: 1500 });
      if (!smoke.ok) {
        const errors = Array.isArray(smoke.errors) ? smoke.errors : [];
        const message = errors.length ? errors.join('\n') : 'Pipeline runtime error.';
        deps.setTuneMultiplexError(message);
        deps.reportError({ title: 'Pipeline runtime error', error: new Error(message), fallback: message });
        return;
      }
      const refreshed = await deps.fetchTuneStreams();
      deps.setTuneMultiplexDirty(false);
      deps.setTuneMultiplexLastAppliedSignature(signature);
      deps.setTuneMultiplexHydratedSignature(signature);
      deps.setTuneMultiplexHydratedStreamId(streamId);
      if (!options.quiet) {
        deps.toaster.success({ title: 'Multiplex updated' });
      }
      void refreshed;
    } finally {
      deps.setTuneMultiplexBusy(false);
    }
  };

  return {
    buildTuneMultiplexLayout,
    resolveTunePipelineGraph,
    resolveTunePipelineOutput,
    boundTunePipelines,
    ensureTunePipelinesApplied,
    setTuneLivePipelineOutput,
    applyTuneMultiplex
  };
};
