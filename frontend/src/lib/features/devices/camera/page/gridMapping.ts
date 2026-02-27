export type PipelineGridState = {
  get pipelineGridSlots(): Record<string, string | null>;
  set pipelineGridSlots(value: Record<string, string | null>);
  get pipelineGridSlotOutputKeys(): Record<string, string | null>;
  set pipelineGridSlotOutputKeys(value: Record<string, string | null>);
  get pipelineLayoutTouched(): boolean;
  set pipelineLayoutTouched(value: boolean);
};

export function gridKey(row: number, column: number): string {
  return `${row}:${column}`;
}

export function pipelineForCell(state: PipelineGridState, row: number, column: number): string | null {
  return state.pipelineGridSlots[gridKey(row, column)] ?? null;
}

export function setPipelineForCell(
  state: PipelineGridState,
  row: number,
  column: number,
  pipelineId: string | null,
  ensurePipelineOutputsLoaded?: (pipelineId: string) => void
): void {
  const key = gridKey(row, column);
  const prev = state.pipelineGridSlots[key] ?? null;
  const next = { ...state.pipelineGridSlots, [key]: pipelineId };
  state.pipelineGridSlots = next;
  if (prev !== pipelineId) {
    state.pipelineGridSlotOutputKeys = { ...state.pipelineGridSlotOutputKeys, [key]: null };
  }
  const normalized = typeof pipelineId === 'string' ? pipelineId.trim() : '';
  if (normalized.length) ensurePipelineOutputsLoaded?.(normalized);
}

export function outputKeyForCell(state: PipelineGridState, row: number, column: number): string | null {
  return state.pipelineGridSlotOutputKeys[gridKey(row, column)] ?? null;
}

export function setOutputKeyForCell(
  state: PipelineGridState,
  row: number,
  column: number,
  outputKey: string | null,
  schedulePipelineLayoutApply?: () => void
): void {
  state.pipelineLayoutTouched = true;
  const key = gridKey(row, column);
  const normalized = typeof outputKey === 'string' ? outputKey.trim() : '';
  state.pipelineGridSlotOutputKeys = { ...state.pipelineGridSlotOutputKeys, [key]: normalized.length ? normalized : null };
  schedulePipelineLayoutApply?.();
}
