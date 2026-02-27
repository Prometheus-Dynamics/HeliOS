import { multiplexKey, normalizeMultiplexSlots } from './pipelineMultiplexUtils';

export type TuneMultiplexUiDeps = {
  getTuneMultiplexRows: () => number;
  setTuneMultiplexRows: (next: number) => void;
  getTuneMultiplexColumns: () => number;
  setTuneMultiplexColumns: (next: number) => void;
  getTuneMultiplexSlots: () => Record<string, string | null>;
  setTuneMultiplexSlots: (next: Record<string, string | null>) => void;
  getTuneMultiplexSlotOutputs: () => Record<string, string | null>;
  setTuneMultiplexSlotOutputs: (next: Record<string, string | null>) => void;
  getTuneMultiplexGridIsSingle: () => boolean;
  setTuneMultiplexDragPipelineId: (next: string | null) => void;
  setTuneMultiplexDragSource: (next: { row: number; column: number } | null) => void;
  getTuneMultiplexDragPipelineId: () => string | null;
  getTuneMultiplexDragSource: () => { row: number; column: number } | null;
  outputOptionsForPipeline: (pipelineId: string | null) => string[];
  markTuneMultiplexDirty: () => void;
};

export const createTuneMultiplexUi = (deps: TuneMultiplexUiDeps) => {
  const tunePipelineForCell = (row: number, column: number): string | null =>
    deps.getTuneMultiplexSlots()[multiplexKey(row, column)] ?? null;

  const tuneOutputKeyForCell = (row: number, column: number): string | null =>
    deps.getTuneMultiplexSlotOutputs()[multiplexKey(row, column)] ?? null;

  const setTuneOutputKeyForCell = (row: number, column: number, outputKey: string | null): void => {
    const key = multiplexKey(row, column);
    const normalized = typeof outputKey === 'string' ? outputKey.trim() : '';
    deps.setTuneMultiplexSlotOutputs({ ...deps.getTuneMultiplexSlotOutputs(), [key]: normalized.length ? normalized : null });
    deps.markTuneMultiplexDirty();
  };

  const tuneOutputSelectionForPipeline = (pipelineId: string): string | null => {
    const key = multiplexKey(0, 0);
    const current = deps.getTuneMultiplexSlotOutputs()[key];
    if (current && current.trim().length) return current;
    const options = deps.outputOptionsForPipeline(pipelineId);
    return options[0] ?? null;
  };

  const setTuneOutputSelectionForPipeline = (pipelineId: string, output: string | null): void => {
    const key = multiplexKey(0, 0);
    const options = deps.outputOptionsForPipeline(pipelineId);
    const normalized = typeof output === 'string' && output.trim().length ? output.trim() : options[0] ?? null;
    const current = deps.getTuneMultiplexSlotOutputs()[key] ?? null;
    deps.setTuneMultiplexSlotOutputs({ ...deps.getTuneMultiplexSlotOutputs(), [key]: normalized });
    if (current !== normalized) {
      deps.markTuneMultiplexDirty();
    }
  };

  const setTuneMultiplexGridDimensions = (rows: number, columns: number): void => {
    const nextRows = Math.min(6, Math.max(1, Math.trunc(rows)));
    const nextCols = Math.min(6, Math.max(1, Math.trunc(columns)));
    deps.setTuneMultiplexRows(nextRows);
    deps.setTuneMultiplexColumns(nextCols);
    deps.setTuneMultiplexSlots(normalizeMultiplexSlots(nextRows, nextCols, deps.getTuneMultiplexSlots()));
    deps.setTuneMultiplexSlotOutputs(normalizeMultiplexSlots(nextRows, nextCols, deps.getTuneMultiplexSlotOutputs()));
    deps.markTuneMultiplexDirty();
  };

  const clearTuneMultiplexCell = (row: number, column: number): void => {
    const key = multiplexKey(row, column);
    deps.setTuneMultiplexSlots({ ...deps.getTuneMultiplexSlots(), [key]: null });
    deps.setTuneMultiplexSlotOutputs({ ...deps.getTuneMultiplexSlotOutputs(), [key]: null });
    deps.markTuneMultiplexDirty();
  };

  const tuneAllowDrop = (event: DragEvent): void => {
    event.preventDefault();
  };

  const startTuneMultiplexDrag =
    (pipelineId: string, source?: { row: number; column: number }) =>
    (event: DragEvent): void => {
      deps.setTuneMultiplexDragPipelineId(pipelineId);
      deps.setTuneMultiplexDragSource(source ?? null);
      event.dataTransfer?.setData('text/plain', pipelineId);
    };

  const dropTuneMultiplexOn =
    (row: number, column: number) =>
    (event: DragEvent): void => {
      event.preventDefault();
      const pipelineId = String(event.dataTransfer?.getData('text/plain') ?? deps.getTuneMultiplexDragPipelineId() ?? '').trim();
      if (!pipelineId) return;

      const next: Record<string, string | null> = { ...deps.getTuneMultiplexSlots() };
      const targetKey = multiplexKey(row, column);
      const prevAtTarget = next[targetKey] ?? null;
      next[targetKey] = pipelineId;

      const nextOutputs: Record<string, string | null> = { ...deps.getTuneMultiplexSlotOutputs() };
      if (!deps.getTuneMultiplexDragSource()) {
        nextOutputs[targetKey] = null;
      } else {
        const sourceKey = multiplexKey(deps.getTuneMultiplexDragSource()!.row, deps.getTuneMultiplexDragSource()!.column);
        if (sourceKey !== targetKey) {
          nextOutputs[targetKey] = nextOutputs[sourceKey] ?? null;
          nextOutputs[sourceKey] = null;
        }
      }
      if (prevAtTarget && prevAtTarget !== pipelineId) {
        nextOutputs[targetKey] = null;
      }

      if (deps.getTuneMultiplexDragSource() && (deps.getTuneMultiplexDragSource()!.row !== row || deps.getTuneMultiplexDragSource()!.column !== column)) {
        const sourceKey = multiplexKey(deps.getTuneMultiplexDragSource()!.row, deps.getTuneMultiplexDragSource()!.column);
        if (next[sourceKey] === pipelineId) {
          next[sourceKey] = null;
        }
      }

      deps.setTuneMultiplexSlots(next);
      deps.setTuneMultiplexSlotOutputs(nextOutputs);
      deps.setTuneMultiplexDragPipelineId(null);
      deps.setTuneMultiplexDragSource(null);
      deps.markTuneMultiplexDirty();
    };

  return {
    tunePipelineForCell,
    tuneOutputKeyForCell,
    setTuneOutputKeyForCell,
    tuneOutputSelectionForPipeline,
    setTuneOutputSelectionForPipeline,
    setTuneMultiplexGridDimensions,
    clearTuneMultiplexCell,
    tuneAllowDrop,
    startTuneMultiplexDrag,
    dropTuneMultiplexOn
  };
};
