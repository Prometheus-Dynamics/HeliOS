import type { StreamInfo, StreamManifest } from '$lib/api/client';
import { apiFetchResponse } from '$lib/api/core/http';
import type {
  StreamPipelineWire
} from '$lib/api/client';
import type { PipelineDataType } from '$lib/types/pipeline';
import { normalizeGridSlots, normalizeGridOutputKeys } from './cameraPipelineState';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  RAW_LOOPBACK_GRAPH,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds
} from './cameraPipelineShared';
import { createPipelineLayoutData } from './cameraPipelineLayoutData';
import type { PipelineLayoutDeps, PipelineLayoutState } from './cameraPipelineLayoutTypes';
import {
  hydratePipelineUi as hydratePipelineUiState,
  persistPipelineUi as persistPipelineUiState,
  pipelineUiStorageKey as pipelineUiStorageKeyState
} from './layoutPersistence';
import {
  buildPipelineLayoutPayload as buildPipelineLayoutPayloadFn,
  ensureAssignedPipelineId,
  extractGraphOutputPorts as extractGraphOutputPortsFn,
  gridHasUnappliedPipelines as gridHasUnappliedPipelinesFn,
  isMultiplexLayout as isMultiplexLayoutFn,
  isPipelineApplied as isPipelineAppliedFn,
  outputSelectionForPipeline as outputSelectionForPipelineFn,
  pipelineLayoutSignature as pipelineLayoutSignatureFn,
  type PipelineLayoutPayload
} from './layoutValidation';
import {
  gridKey as gridKeyFn,
  outputKeyForCell as outputKeyForCellFn,
  pipelineForCell as pipelineForCellFn,
  setOutputKeyForCell as setOutputKeyForCellFn,
  setPipelineForCell as setPipelineForCellFn
} from './gridMapping';

export function createPipelineLayoutController(state: PipelineLayoutState, deps: PipelineLayoutDeps) {
  const apiOptions = { baseUrl: deps.apiBase };
  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

  function extractGraphOutputPorts(graph: unknown): string[] {
    return extractGraphOutputPortsFn(graph);
  }

  function outputSelectionForPipeline(pipelineId: string): string | null {
    return outputSelectionForPipelineFn(state, pipelineId);
  }

  const {
    ensurePipelineRegistry,
    ensurePipelineGraphAndOutputs,
    ensurePipelineOutputsLoaded,
    refreshPipelineGraphs,
    refreshPipelineOutputs,
    listPipelineTemplatesForAssign,
    createPipelineFromTemplateAndAssign
  } = createPipelineLayoutData(state, deps);

  function isMultiplexLayout(rows = state.pipelineGridRows, columns = state.pipelineGridColumns): boolean {
    return isMultiplexLayoutFn(rows, columns);
  }

  function isPipelineApplied(pipelineId: string | null): boolean {
    return isPipelineAppliedFn(state, pipelineId);
  }

  function gridHasUnappliedPipelines(): boolean {
    return gridHasUnappliedPipelinesFn(state);
  }

  function buildPipelineLayoutPayload(): PipelineLayoutPayload | null {
    return buildPipelineLayoutPayloadFn(state);
  }

  async function applyPipelineGraphForId(pipelineId: string): Promise<boolean> {
    if (!state.stream?.id) return false;
    const normalized = String(pipelineId ?? '').trim();
    if (!normalized.length) return false;
    try {
      const output = outputSelectionForPipelineFn(state, normalized);
      if (normalized === RAW_PIPELINE_ID) {
        await deps.streamsApi.setPipelineGraph({
          id: state.stream.id,
          requestBody: { graph: RAW_LOOPBACK_GRAPH, pipeline_id: RAW_PIPELINE_UUID, output: output ?? null }
        }, apiOptions);
      } else {
        const doc = await deps.pipelinesApi.fetchGraph({ id: normalized }, apiOptions);
        const graph = doc.graph ?? null;
        if (!graph) {
          throw new Error(`Pipeline graph missing: ${normalized}`);
        }
        const graphWithOverrides = deps.applyPipelineOverridesToGraph(normalized, graph);
        await deps.streamsApi.setPipelineGraph({
          id: state.stream.id,
          requestBody: { graph: graphWithOverrides, pipeline_id: normalized, output: output ?? null }
        }, apiOptions);
      }
      await deps.refresh();
      return true;
    } catch (err) {
      console.warn('Failed to apply pipeline graph', normalized, err);
      deps.reportError({
        title: 'Pipeline apply failed',
        error: err,
        fallback: 'Unable to apply pipeline changes right now.'
      });
      return false;
    }
  }

  async function applyUnappliedPipelines(): Promise<boolean> {
    const missing = new Set<string>();
    for (const pipelineId of Object.values(state.pipelineGridSlots)) {
      const normalized = typeof pipelineId === 'string' ? pipelineId.trim() : '';
      if (!normalized.length) continue;
      if (!isPipelineAppliedFn(state, normalized)) {
        missing.add(normalized);
      }
    }
    for (const pipelineId of missing) {
      const ok = await applyPipelineGraphForId(pipelineId);
      if (!ok) return false;
    }
    return true;
  }

  // Note: we intentionally do not auto-collapse multiplex grids back to 1x1 raw. In multiplex,
  // empty cells should render as empty (black) until the user explicitly places a pipeline (raw
  // or otherwise) into a cell.

  function schedulePipelineLayoutApply(): void {
    if (!state.stream?.id) return;
    const rows = Math.min(Math.max(Math.trunc(state.pipelineGridRows), 1), 6);
    const columns = Math.min(Math.max(Math.trunc(state.pipelineGridColumns), 1), 6);
    const multiplex = isMultiplexLayoutFn(rows, columns);
    let nextLayout: PipelineLayoutPayload | null = null;

    if (!multiplex) {
      const normalizedSlots = normalizeGridSlots(rows, columns, state.pipelineGridSlots);
      const normalizedOutputs = normalizeGridOutputKeys(rows, columns, state.pipelineGridSlotOutputKeys);
      state.pipelineGridSlots = normalizedSlots;
      state.pipelineGridSlotOutputKeys = normalizedOutputs;
      const outputCellPipeline = normalizedSlots[PIPELINE_OUTPUT_CELL_KEY];
      const hasAssignedPipeline = typeof outputCellPipeline === 'string' && outputCellPipeline.trim().length > 0;
      if (hasAssignedPipeline) {
        const normalizedId = outputCellPipeline.trim();
        let output_key = normalizedOutputs[PIPELINE_OUTPUT_CELL_KEY] ?? outputSelectionForPipelineFn(state, normalizedId);
        if (normalizedId === RAW_PIPELINE_ID && typeof output_key === 'string' && output_key.trim().toLowerCase() === 'frame') {
          output_key = 'raw';
        }
        nextLayout = {
          rows,
          columns,
          slots: [
            {
              row: 0,
              column: 0,
              pipeline_id: normalizedId === RAW_PIPELINE_ID ? RAW_PIPELINE_UUID : normalizedId,
              output_key: typeof output_key === 'string' && output_key.trim().length ? output_key.trim() : null
            }
          ]
        };
      } else {
        // Keep clear behavior consistent: even in 1x1, an explicitly empty grid should stay blank.
        nextLayout = { rows, columns, slots: [] };
      }
    } else {
      nextLayout = buildPipelineLayoutPayloadFn(state);
      if (!nextLayout) return;
    }

    if (state.pipelineLayoutApplyTimer != null) {
      clearTimeout(state.pipelineLayoutApplyTimer);
    }
    state.pipelineLayoutApplyTimer = window.setTimeout(async () => {
      state.pipelineLayoutApplyTimer = null;
      if (gridHasUnappliedPipelinesFn(state)) {
        const applied = await applyUnappliedPipelines();
        if (!applied) return;
      }
      try {
        await deps.streamsApi.setPipelineLayout({ id: state.stream!.id, requestBody: { pipeline_layout: nextLayout } }, apiOptions);
        await deps.refresh();
        deps.onExternalLayoutApplied?.();
      } catch (err) {
        console.warn('Failed to update pipeline layout', err);
        deps.reportError({
          title: 'Pipeline layout failed',
          error: err,
          fallback: 'Unable to update the pipeline layout right now.'
        });
        await deps.refresh();
      }
    }, deps.layoutDebounceMs);
  }

  function setOutputSelectionForPipeline(pipelineId: string, output: string | null): void {
    const normalizedId = String(pipelineId ?? '').trim();
    if (!normalizedId.length) return;
    let normalizedOutput = typeof output === 'string' && output.trim().length ? output.trim() : null;
    if (normalizedId === RAW_PIPELINE_ID && normalizedOutput && normalizedOutput.toLowerCase() === 'frame') {
      normalizedOutput = 'raw';
    }
    const prevRaw = state.pipelineOutputByPipelineId[normalizedId];
    const prev = typeof prevRaw === 'string' && prevRaw.trim().length ? prevRaw.trim() : null;
    if (prev === normalizedOutput) return;
    state.pipelineOutputByPipelineId = { ...state.pipelineOutputByPipelineId, [normalizedId]: normalizedOutput };
  }

  async function setLivePipelineOutput(output: string | null): Promise<void> {
    if (!state.stream?.id) return;
    try {
      const resp = await apiFetchResponse(deps.apiPath(`/streams/${encodeURIComponent(state.stream.id)}/pipeline/output`), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ output })
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Request failed (${resp.status})`);
      }
    } catch (err) {
      console.warn('Failed to set pipeline output', err);
      deps.reportError({
        title: 'Pipeline output failed',
        error: err,
        fallback: 'Unable to update pipeline output right now.'
      });
    }
  }

  function currentPipelineWires(): StreamPipelineWire[] {
    const manifest = asRecord(state.manifestState);
    const wires = manifest?.pipeline_wires ?? null;
    return Array.isArray(wires) ? wires : [];
  }

  async function setFrameSourceForPipelineInstance(args: {
    to: { pipelineId: string; outputKey?: string | null };
    from: { pipelineId: string; outputKey?: string | null; port?: string | null } | null;
  }): Promise<void> {
    if (!state.stream?.id) return;
    const toId = String(args.to?.pipelineId ?? '').trim();
    if (!toId.length) return;
    const toUuid = toId === RAW_PIPELINE_ID ? RAW_PIPELINE_UUID : toId;
    const toOutputKey = typeof args.to.outputKey === 'string' && args.to.outputKey.trim().length ? args.to.outputKey.trim() : null;

    const normalizeId = (value: string): string => (value === RAW_PIPELINE_ID ? RAW_PIPELINE_UUID : value);
    const normalizeKey = (value: unknown): string | null => (typeof value === 'string' && value.trim().length ? value.trim() : null);
    const normalizePort = (value: unknown, fallback: string): string => {
      const raw = typeof value === 'string' ? value.trim() : '';
      return raw.length ? raw : fallback;
    };

    const prev = currentPipelineWires();
    const next = prev.filter((wire) => {
      const to = wire?.to ?? null;
      const wireToId = typeof to?.pipeline_id === 'string' ? to.pipeline_id.trim() : '';
      const wireToKey = normalizeKey(to?.output_key);
      const wireToPort = normalizePort(to?.port, 'frame').toLowerCase();
      if (!wireToId.length) return true;
      if (wireToPort !== 'frame') return true;
      if (wireToId !== toUuid) return true;
      // Match instance key (output_key) when provided; otherwise treat null/empty as the default instance.
      if ((wireToKey ?? null) !== (toOutputKey ?? null)) return true;
      return false;
    });

    if (args.from) {
      const fromId = String(args.from.pipelineId ?? '').trim();
      if (fromId.length) {
        const fromUuid = normalizeId(fromId);
        const fromOutputKey = typeof args.from.outputKey === 'string' && args.from.outputKey.trim().length ? args.from.outputKey.trim() : null;
        const fromPort = typeof args.from.port === 'string' && args.from.port.trim().length ? args.from.port.trim() : null;
        next.push({
          from: { pipeline_id: fromUuid, output_key: fromOutputKey, port: fromPort },
          to: { pipeline_id: toUuid, output_key: toOutputKey, port: 'frame' }
        });
      }
    }

    try {
      await deps.streamsApi.setPipelineWires({ id: state.stream.id, requestBody: { wires: next } }, apiOptions);
      await deps.refresh();
    } catch (err) {
      console.warn('Failed to set pipeline wires', err);
      deps.reportError({
        title: 'Pipeline wiring failed',
        error: err,
        fallback: 'Unable to update pipeline wiring right now.'
      });
      await deps.refresh();
    }
  }

  function pipelineLabel(pipelineId: string): string {
    if (pipelineId === RAW_PIPELINE_ID) return 'Raw stream';
    const entry = state.pipelineGraphs.find((g) => String(g?.id ?? '') === pipelineId) ?? null;
    const name = entry?.name?.trim?.() ? String(entry.name).trim() : '';
    return name.length ? name : pipelineId;
  }

  function openPipelineAssignModal(): void {
    state.pipelineAssignDraft = normalizeAssignedPipelineIds(state.assignedPipelineIds);
    state.pipelineAssignQuery = '';
    state.pipelineAssignModalOpen = true;
    if (!state.pipelineGraphs.length && !state.pipelineGraphLoading) {
      void refreshPipelineGraphs();
    }
  }

  function closePipelineAssignModal(): void {
    state.pipelineAssignModalOpen = false;
  }

  function savePipelineAssignModal(): void {
    const normalized = normalizeAssignedPipelineIds(state.pipelineAssignDraft);
    applyAssignedPipelineIds(normalized);
    state.pipelineAssignModalOpen = false;
    deps.scheduleStreamPresetApply();
  }

  function dropPipelineEverywhere(pipelineId: string): void {
    const next: Record<string, string | null> = { ...state.pipelineGridSlots };
    Object.keys(next).forEach((key) => {
      if (next[key] === pipelineId) next[key] = null;
    });
    state.pipelineGridSlots = next;
    state.pipelineGridSlotOutputKeys = normalizeGridOutputKeys(state.pipelineGridRows, state.pipelineGridColumns, state.pipelineGridSlotOutputKeys);
    if (state.pipelineOutputByPipelineId[pipelineId] !== undefined) {
      const outputs = { ...state.pipelineOutputByPipelineId };
      delete outputs[pipelineId];
      state.pipelineOutputByPipelineId = outputs;
    }
  }

  function applyAssignedPipelineIds(nextIds: string[]): void {
    const normalized = normalizeAssignedPipelineIds(nextIds);
    if (normalized.join('|') !== normalizeAssignedPipelineIds(state.assignedPipelineIds).join('|')) {
      state.pipelineLayoutTouched = true;
      state.pipelineAssignmentsTouched = true;
    }
    state.assignedPipelineIds = normalized;
    state.pipelineAssignDraft = normalized;
    state.pipelineGridSlots = normalizeGridSlots(state.pipelineGridRows, state.pipelineGridColumns, state.pipelineGridSlots);
    Object.values(state.pipelineGridSlots).forEach((pipelineId) => {
      if (pipelineId && pipelineId !== RAW_PIPELINE_ID && !normalized.includes(pipelineId)) {
        dropPipelineEverywhere(pipelineId);
      }
    });
    state.pipelineOutputByPipelineId = Object.fromEntries(
      Object.entries(state.pipelineOutputByPipelineId).filter(([id]) => normalized.includes(id))
    );
    if (state.selectedPipelineId && !normalized.includes(state.selectedPipelineId)) {
      const stillInGrid = Object.values(state.pipelineGridSlots ?? {}).some((id) => id === state.selectedPipelineId);
      if (!stillInGrid) {
        state.selectedPipelineId = null;
        state.selectedPipelineOutput = null;
      }
    }
  }

  function openPipelineRemoveModal(pipelineId: string): void {
    state.pipelineRemoveCandidateId = pipelineId;
    state.pipelineRemoveModalOpen = true;
  }

  function closePipelineRemoveModal(): void {
    state.pipelineRemoveModalOpen = false;
    state.pipelineRemoveCandidateId = null;
  }

  function confirmPipelineRemove(): void {
    const pipelineId = state.pipelineRemoveCandidateId;
    if (!pipelineId) {
      closePipelineRemoveModal();
      return;
    }
    applyAssignedPipelineIds(state.assignedPipelineIds.filter((id) => id !== pipelineId));
    closePipelineRemoveModal();
    deps.scheduleStreamPresetApply();
  }

  function setPipelineGridDimensions(rows: number, columns: number): void {
    state.pipelineLayoutTouched = true;
    const prevRows = Math.min(Math.max(Math.trunc(state.pipelineGridRows ?? 1), 1), 6);
    const prevColumns = Math.min(Math.max(Math.trunc(state.pipelineGridColumns ?? 1), 1), 6);
    const prevMultiplex = isMultiplexLayoutFn(prevRows, prevColumns);
    const prevSlots = normalizeGridSlots(prevRows, prevColumns, state.pipelineGridSlots);

    const nextRows = Math.min(Math.max(Math.trunc(rows), 1), 6);
    const nextColumns = Math.min(Math.max(Math.trunc(columns), 1), 6);
    const nextMultiplex = isMultiplexLayoutFn(nextRows, nextColumns);
    state.pipelineGridRows = nextRows;
    state.pipelineGridColumns = nextColumns;
    const nextSlots = normalizeGridSlots(nextRows, nextColumns, state.pipelineGridSlots);
    state.pipelineGridSlotOutputKeys = normalizeGridOutputKeys(nextRows, nextColumns, state.pipelineGridSlotOutputKeys);
    // When expanding from 1x1 into multiplex, do not implicitly carry over the raw stream into the
    // output cell. Multiplex grids should be empty unless the user explicitly places a pipeline.
    const expandingToMultiplex = nextMultiplex && !prevMultiplex;
    const prevWasJustRaw =
      Object.values(prevSlots).filter(Boolean).length === 1 && prevSlots[PIPELINE_OUTPUT_CELL_KEY] === RAW_PIPELINE_ID;
    state.pipelineGridSlots = expandingToMultiplex && prevWasJustRaw ? {} : nextSlots;

    // Only auto-ensure a pipeline in single-cell layouts. Multiplex grids are allowed to be empty.
    if (!nextMultiplex) {
      ensureAssignedPipelineId(state);
    }
    schedulePipelineLayoutApply();
  }

  const pipelineForCellAt = (row: number, column: number): string | null => pipelineForCellFn(state, row, column);
  const outputKeyForCellAt = (row: number, column: number): string | null => outputKeyForCellFn(state, row, column);
  const setPipelineForCellAt = (row: number, column: number, pipelineId: string | null): void =>
    setPipelineForCellFn(state, row, column, pipelineId, ensurePipelineOutputsLoaded);
  const setOutputKeyForCellAt = (row: number, column: number, outputKey: string | null): void =>
    setOutputKeyForCellFn(state, row, column, outputKey, schedulePipelineLayoutApply);

  function gridKey(row: number, column: number): string {
    return gridKeyFn(row, column);
  }

  function pipelineForCell(row: number, column: number): string | null {
    return pipelineForCellAt(row, column);
  }

  function setPipelineForCell(row: number, column: number, pipelineId: string | null): void {
    setPipelineForCellAt(row, column, pipelineId);
  }

  function outputKeyForCell(row: number, column: number): string | null {
    return outputKeyForCellAt(row, column);
  }

  function setOutputKeyForCell(row: number, column: number, outputKey: string | null): void {
    setOutputKeyForCellAt(row, column, outputKey);
  }

  function handlePipelineDragStart(pipelineId: string, from?: { row: number; column: number }): (event: DragEvent) => void {
    return (event: DragEvent) => {
      state.pipelineDragPayload = { pipelineId, from };
      try {
        event.dataTransfer?.setData('text/plain', JSON.stringify({ pipelineId, from }));
        event.dataTransfer?.setData('application/json', JSON.stringify({ pipelineId, from }));
      } catch {
        // ignore
      }
      if (event.dataTransfer) {
        event.dataTransfer.effectAllowed = 'move';
      }
    };
  }

  function readDragPayload(event: DragEvent): { pipelineId: string; from?: { row: number; column: number } } | null {
    if (state.pipelineDragPayload) return state.pipelineDragPayload;
    try {
      const raw = event.dataTransfer?.getData('application/json') || event.dataTransfer?.getData('text/plain');
      if (!raw) return null;
      const parsed = asRecord(JSON.parse(raw));
      const pipelineId = typeof parsed?.pipelineId === 'string' ? parsed.pipelineId.trim() : '';
      if (!pipelineId) return null;
      const fromRecord = asRecord(parsed?.from);
      const fromRow = typeof fromRecord?.row === 'number' ? fromRecord.row : null;
      const fromColumn = typeof fromRecord?.column === 'number' ? fromRecord.column : null;
      const from =
        fromRow != null && fromColumn != null && Number.isInteger(fromRow) && Number.isInteger(fromColumn)
          ? { row: fromRow, column: fromColumn }
          : undefined;
      return { pipelineId, from };
    } catch {
      return null;
    }
  }

  function allowDrop(event: DragEvent): void {
    event.preventDefault();
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = 'move';
    }
  }

  function dropOnCell(row: number, column: number): (event: DragEvent) => void {
    return (event: DragEvent) => {
      event.preventDefault();
      state.pipelineLayoutTouched = true;
      const payload = readDragPayload(event);
      state.pipelineDragPayload = null;
      if (!payload?.pipelineId) return;
      const pipelineId = payload.pipelineId;
      if (pipelineId === RAW_PIPELINE_ID) {
        setPipelineForCellAt(row, column, RAW_PIPELINE_ID);
        if (payload.from && (payload.from.row !== row || payload.from.column !== column)) {
          setPipelineForCellAt(payload.from.row, payload.from.column, null);
        }
        schedulePipelineLayoutApply();
        return;
      }
      setPipelineForCellAt(row, column, pipelineId);
      if (payload.from && (payload.from.row !== row || payload.from.column !== column)) {
        setPipelineForCellAt(payload.from.row, payload.from.column, null);
      }
      if (!state.assignedPipelineIds.includes(pipelineId)) {
        state.assignedPipelineIds = normalizeAssignedPipelineIds([pipelineId, ...state.assignedPipelineIds]);
      }
      schedulePipelineLayoutApply();
    };
  }

  function clearCell(row: number, column: number): void {
    state.pipelineLayoutTouched = true;
    setPipelineForCellAt(row, column, null);
    schedulePipelineLayoutApply();
  }

  function hydratePipelineUi(key: string): void {
    hydratePipelineUiState(state, deps.storagePrefix, key);
  }

  function persistPipelineUi(): void {
    persistPipelineUiState(state, deps.storagePrefix);
  }

  function pipelineUiStorageKey(key: string): string {
    return pipelineUiStorageKeyState(deps.storagePrefix, key);
  }

  function pipelineLayoutSignature(): string | null {
    return pipelineLayoutSignatureFn(state);
  }

  return {
    extractGraphOutputPorts,
    ensurePipelineRegistry,
    ensurePipelineGraphAndOutputs,
    ensurePipelineOutputsLoaded,
    outputSelectionForPipeline,
    isMultiplexLayout,
    gridHasUnappliedPipelines,
    isPipelineApplied,
    buildPipelineLayoutPayload,
    applyPipelineGraphForId,
    applyUnappliedPipelines,
    schedulePipelineLayoutApply,
    setOutputSelectionForPipeline,
    refreshPipelineGraphs,
    refreshPipelineOutputs,
    setLivePipelineOutput,
    setFrameSourceForPipelineInstance,
    pipelineLabel,
    listPipelineTemplatesForAssign,
    createPipelineFromTemplateAndAssign,
    openPipelineAssignModal,
    closePipelineAssignModal,
    savePipelineAssignModal,
    dropPipelineEverywhere,
    applyAssignedPipelineIds,
    openPipelineRemoveModal,
    closePipelineRemoveModal,
    confirmPipelineRemove,
    setPipelineGridDimensions,
    gridKey,
    pipelineForCell,
    setPipelineForCell,
    outputKeyForCell,
    setOutputKeyForCell,
    handlePipelineDragStart,
    readDragPayload,
    allowDrop,
    dropOnCell,
    clearCell,
    hydratePipelineUi,
    persistPipelineUi,
    pipelineUiStorageKey,
    pipelineLayoutSignature
  };
}
