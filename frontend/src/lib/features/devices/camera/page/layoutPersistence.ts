import { normalizeGridOutputKeys, normalizeGridSlots } from './cameraPipelineState';
import { normalizeAssignedPipelineIds, normalizePipelineOutputMap } from './cameraPipelineTuningController';
import { readJson, writeJson } from '$lib/utils/storage';

export type PipelineLayoutPersistenceState = {
  pipelineGridRows: number;
  pipelineGridColumns: number;
  pipelineGridSlots: Record<string, string | null>;
  pipelineGridSlotOutputKeys: Record<string, string | null>;
  assignedPipelineIds: string[];
  pipelineOutputByPipelineId: Record<string, string | null>;
  pipelineUiHydrated: boolean;
  stream: { id?: string } | null;
  streamId: string;
};

export function hydratePipelineUi(state: PipelineLayoutPersistenceState, storagePrefix: string, key: string): void {
  try {
    const parsed = readJson<any>(`${storagePrefix}${key}`, null);
    if (!parsed) return;
    const rows = Math.min(Math.max(Number(parsed?.grid?.rows ?? state.pipelineGridRows), 1), 6);
    const columns = Math.min(Math.max(Number(parsed?.grid?.columns ?? state.pipelineGridColumns), 1), 6);
    state.pipelineGridRows = rows;
    state.pipelineGridColumns = columns;
    state.pipelineGridSlots = normalizeGridSlots(rows, columns, parsed?.grid?.slots ?? {});
    state.pipelineGridSlotOutputKeys = normalizeGridOutputKeys(rows, columns, parsed?.grid?.outputKeys ?? {});
    const assigned = normalizeAssignedPipelineIds(parsed?.assignedPipelineIds ?? []);
    if (assigned.length) {
      state.assignedPipelineIds = assigned;
    }
    state.pipelineOutputByPipelineId = normalizePipelineOutputMap(parsed?.pipelineOutputs ?? {});
  } catch (err) {
    console.warn('Failed to load pipeline UI state', err);
  }
}

export function persistPipelineUi(state: PipelineLayoutPersistenceState, storagePrefix: string): void {
  if (!state.pipelineUiHydrated) return;
  try {
    const payload = {
      assignedPipelineIds: normalizeAssignedPipelineIds(state.assignedPipelineIds),
      grid: {
        rows: state.pipelineGridRows,
        columns: state.pipelineGridColumns,
        slots: normalizeGridSlots(state.pipelineGridRows, state.pipelineGridColumns, state.pipelineGridSlots),
        outputKeys: normalizeGridOutputKeys(
          state.pipelineGridRows,
          state.pipelineGridColumns,
          state.pipelineGridSlotOutputKeys
        )
      },
      pipelineOutputs: normalizePipelineOutputMap(state.pipelineOutputByPipelineId)
    };
    const key = String(state.stream?.id ?? '').trim() || state.streamId;
    writeJson(`${storagePrefix}${key}`, payload);
  } catch (err) {
    console.warn('Failed to persist pipeline UI state', err);
  }
}

export function pipelineUiStorageKey(storagePrefix: string, key: string): string {
  return `${storagePrefix}${key}`;
}
