import type { StreamManifest, StreamInfo } from '$lib/api/httpClient';
import { normalizeGridOutputKeys, normalizeGridSlots, layoutSignature } from './cameraPipelineState';
import { extractGraphOutputPorts as extractGraphOutputPortsImpl } from '$lib/features/pipelines/graphOutputPorts';
import { PIPELINE_OUTPUT_CELL_KEY, RAW_PIPELINE_ID, RAW_PIPELINE_UUID } from './cameraPipelineTuningController';

export type PipelineLayoutPayload = {
  rows: number;
  columns: number;
  slots: Array<{ row: number; column: number; pipeline_id: string | null; output_key: string | null }>;
};

export type PipelineLayoutValidationState = {
  stream: StreamInfo | null;
  manifestState: StreamManifest | null;
  pipelineGridRows: number;
  pipelineGridColumns: number;
  pipelineGridSlots: Record<string, string | null>;
  pipelineGridSlotOutputKeys: Record<string, string | null>;
  pipelineOutputByPipelineId: Record<string, string | null>;
  assignedPipelineIds: string[];
};

export function extractGraphOutputPorts(graph: any): string[] {
  return extractGraphOutputPortsImpl(graph);
}

export function outputSelectionForPipeline(
  state: Pick<PipelineLayoutValidationState, 'pipelineOutputByPipelineId'>,
  pipelineId: string
): string | null {
  const normalized = String(pipelineId ?? '').trim();
  if (!normalized.length) return null;
  const raw = state.pipelineOutputByPipelineId[normalized];
  const value = typeof raw === 'string' && raw.trim().length ? raw.trim() : null;
  if (normalized === RAW_PIPELINE_ID && value?.toLowerCase() === 'frame') {
    return 'raw';
  }
  return value;
}

export function isMultiplexLayout(rows: number, columns: number): boolean {
  return Math.trunc(rows) * Math.trunc(columns) > 1;
}

export function isPipelineApplied(state: PipelineLayoutValidationState, pipelineId: string | null): boolean {
  if (!pipelineId) return false;
  if (pipelineId === RAW_PIPELINE_ID) return true;
  const normalized = String(pipelineId).trim();
  if (!normalized.length) return false;
  const pipelines = (state.manifestState as any)?.pipelines ?? [];
  if (Array.isArray(pipelines)) {
    return pipelines.some((entry) => String(entry?.pipeline_id ?? '').trim() === normalized);
  }
  const legacy = (state.manifestState as any)?.pipeline_id;
  return typeof legacy === 'string' && legacy.trim() === normalized;
}

export function gridHasUnappliedPipelines(state: PipelineLayoutValidationState): boolean {
  return Object.values(state.pipelineGridSlots).some((pipelineId) => {
    if (!pipelineId) return false;
    return !isPipelineApplied(state, pipelineId);
  });
}

export function buildPipelineLayoutPayload(state: PipelineLayoutValidationState): PipelineLayoutPayload | null {
  const rows = Math.min(Math.max(Math.trunc(state.pipelineGridRows), 1), 6);
  const columns = Math.min(Math.max(Math.trunc(state.pipelineGridColumns), 1), 6);
  const multiplex = isMultiplexLayout(rows, columns);
  if (!multiplex) return null;
  const normalizedSlots = normalizeGridSlots(rows, columns, state.pipelineGridSlots);
  const normalizedOutputs = normalizeGridOutputKeys(rows, columns, state.pipelineGridSlotOutputKeys);
  state.pipelineGridSlots = normalizedSlots;
  state.pipelineGridSlotOutputKeys = normalizedOutputs;
  const slots = Object.entries(normalizedSlots)
    .map(([key, pipelineId]) => {
      if (!pipelineId) return null;
      const [rRaw, cRaw] = key.split(':');
      const row = Math.trunc(Number(rRaw));
      const column = Math.trunc(Number(cRaw));
      if (!Number.isInteger(row) || !Number.isInteger(column)) return null;
      if (row < 0 || column < 0 || row >= rows || column >= columns) return null;
      let output_key = normalizedOutputs[key] ?? null;
      if (pipelineId === RAW_PIPELINE_ID) {
        if (typeof output_key === 'string' && output_key.trim().toLowerCase() === 'frame') {
          output_key = 'raw';
        }
        // RAW stream pipeline supports multiple view ports (e.g. `raw` vs `undistorted`).
        return { row, column, pipeline_id: RAW_PIPELINE_UUID, output_key };
      }
      return { row, column, pipeline_id: pipelineId, output_key };
    })
    .filter(Boolean) as PipelineLayoutPayload['slots'];
  return { rows, columns, slots };
}

export function pipelineLayoutSignature(state: PipelineLayoutValidationState): string | null {
  return layoutSignature(
    state.pipelineGridRows,
    state.pipelineGridColumns,
    state.pipelineGridSlots,
    state.pipelineGridSlotOutputKeys
  );
}

export function ensureAssignedPipelineId(state: PipelineLayoutValidationState): void {
  if (!Object.values(state.pipelineGridSlots).some(Boolean) && state.assignedPipelineIds.length) {
    state.pipelineGridSlots = { ...state.pipelineGridSlots, [PIPELINE_OUTPUT_CELL_KEY]: state.assignedPipelineIds[0] };
  }
}
