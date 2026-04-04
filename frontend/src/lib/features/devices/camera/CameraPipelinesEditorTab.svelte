<script lang="ts">
  import CameraPipelinesGridSection from '$lib/features/devices/camera/CameraPipelinesGridSection.svelte';
  import CameraPipelinesModals from '$lib/features/devices/camera/CameraPipelinesModals.svelte';
  import PipelineProfilerModal from '$lib/components/pipelines/PipelineProfilerModal.svelte';
  import { floatingPipelineOutputsViewer } from '$lib/stores/floatingPipelineOutputsViewer';
  import { extractGraphOutputPortTypes } from '$lib/features/pipelines/outputFilters';
  import type { PipelineDataType, PipelineTemplateSummary } from '$lib/types/pipeline';
  import { SvelteSet } from 'svelte/reactivity';

  let {
    pipelineGraphError,
    openPipelineAssignModal,
    pipelineGraphLoading,
    assignedPipelineIds,
    handlePipelineDragStart,
    RAW_PIPELINE_ID,
    RAW_PIPELINE_UUID = '',
    pipelineLabel,
    openPipelineTuningPanel,
    openPipelineRemoveModal,
    pipelineGridIsSingle,
    setPipelineGridDimensions,
    showAssignControls = true,
    showRemoveControls = true,
    outputsIconEnabled = false,
    streamId = null,
    streamLabel = null,
    pipelineGridRows = $bindable(),
    pipelineGridColumns = $bindable(),
    pipelineGridRowIndices,
    pipelineGridColumnIndices,
    gridSignature = null,
    pipelineForCell,
    outputDurationForCell = null,
    outputSelectionForPipeline,
    outputKeyForCell,
    pipelineWires = [],
    setFrameSourceForPipelineInstance = undefined,
    pipelineOutputOptionsCache,
    ensurePipelineOutputsLoaded = undefined,
    ensurePipelineGraphAndOutputs = undefined,
    schedulePipelineLayoutApply = undefined,
    allowDrop,
    dropOnCell,
    clearCell,
    refreshPipelineGraphs = undefined,
    selectedPipelineOutput = $bindable(),
    setOutputSelectionForPipeline,
    setOutputKeyForCell,
    setLivePipelineOutput,
    pipelineRemoveModalOpen = $bindable(),
    pipelineRemoveCandidateId = $bindable(),
    closePipelineRemoveModal,
    confirmPipelineRemove,
    pipelineAssignModalOpen = $bindable(),
    pipelineAssignQuery = $bindable(),
    pipelineAssignDraft = $bindable(),
    pipelineAssignFilteredGraphs = null,
    pipelineGraphs,
    closePipelineAssignModal,
    savePipelineAssignModal,
    listPipelineTemplatesForAssign = undefined,
    createPipelineFromTemplateAndAssign = undefined
  } = $props();

  let profilerOpen = $state(false);
  let profilerPipelineId = $state<string | null>(null);
  let profilerPipelineLabel = $state<string | null>(null);

  type PipelineGraphEntry = Record<string, unknown> & {
    id?: string | null;
    name?: string | null;
    issue_count?: number | null;
    issueCount?: number | null;
  };

  type PipelineWireEndpoint = {
    pipeline_id?: string | null;
    output_key?: string | null;
    port?: string | null;
  };

  type EnsurePipelineGraphResult = {
    filtered?: string[] | null;
    types?: Record<string, PipelineDataType | null | undefined> | null;
    graphJson?: unknown;
  };

  type Subscribable<T> = {
    subscribe: (run: (value: T) => void) => (() => void) | { unsubscribe?: () => void } | void;
  };
  type DragStartHandler = (event: DragEvent) => void;
  type GridDropHandler = (event: DragEvent) => void;

  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

  const asGraphEntry = (value: unknown): PipelineGraphEntry | null => {
    const record = asRecord(value);
    return record ? (record as PipelineGraphEntry) : null;
  };

  const isSubscribable = <T,>(value: unknown): value is Subscribable<T> =>
    Boolean(value) && typeof value === 'object' && typeof (value as { subscribe?: unknown }).subscribe === 'function';
  const isFunction = <T extends (...args: never[]) => unknown>(value: unknown): value is T => typeof value === 'function';

  const normalizeStreamId = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');
  const normalizeId = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');
  const resolvePipelineLabel = (pipelineId: string | null): string => {
    const normalizedPipelineId = normalizeId(pipelineId);
    if (typeof pipelineLabel === 'function') {
      const resolved = pipelineLabel(normalizedPipelineId);
      if (typeof resolved === 'string' && resolved.trim().length) {
        return resolved.trim();
      }
    }
    if (normalizedPipelineId === RAW_PIPELINE_ID) return 'Raw stream';
    const graphEntry = Array.isArray(pipelineGraphs)
      ? (pipelineGraphs.map(asGraphEntry).find((entry) => normalizeId(entry?.id) === normalizedPipelineId) ?? null)
      : null;
    const graphName = typeof graphEntry?.name === 'string' ? graphEntry.name.trim() : '';
    if (graphName.length) return graphName;
    return normalizedPipelineId.length ? normalizedPipelineId : 'Pipeline';
  };
  const dragStartHandlerFor = (pipelineId: string, from?: { row: number; column: number }): DragStartHandler => {
    if (!isFunction<(pipelineId: string, from?: { row: number; column: number }) => DragStartHandler>(handlePipelineDragStart)) {
      return () => {};
    }
    const handler = handlePipelineDragStart(pipelineId, from);
    return isFunction<DragStartHandler>(handler) ? handler : () => {};
  };
  const readPipelineForCell = (row: number, column: number): string | null => {
    if (!isFunction<(row: number, column: number) => string | null>(pipelineForCell)) return null;
    return pipelineForCell(row, column);
  };
  const readOutputSelectionForPipeline = (pipelineId: string): string | null => {
    if (!isFunction<(pipelineId: string) => string | null>(outputSelectionForPipeline)) return null;
    return outputSelectionForPipeline(pipelineId);
  };
  const readOutputKeyForCell = (row: number, column: number): string | null => {
    if (!isFunction<(row: number, column: number) => string | null>(outputKeyForCell)) return null;
    return outputKeyForCell(row, column);
  };
  const applyGridDimensions = (rows: number, columns: number): void => {
    if (isFunction<(rows: number, columns: number) => void>(setPipelineGridDimensions)) {
      setPipelineGridDimensions(rows, columns);
    }
  };
  const applyAllowDrop = (event: DragEvent): void => {
    if (isFunction<(event: DragEvent) => void>(allowDrop)) {
      allowDrop(event);
      return;
    }
    event.preventDefault();
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = 'move';
    }
  };
  const dropHandlerForCell = (row: number, column: number): GridDropHandler => {
    if (!isFunction<(row: number, column: number) => GridDropHandler>(dropOnCell)) {
      return (event) => event.preventDefault();
    }
    const handler = dropOnCell(row, column);
    return isFunction<GridDropHandler>(handler) ? handler : (event) => event.preventDefault();
  };
  const clearGridCell = (row: number, column: number): void => {
    if (isFunction<(row: number, column: number) => void>(clearCell)) {
      clearCell(row, column);
    }
  };
  const setOutputSelection = (pipelineId: string, next: string | null): void => {
    if (isFunction<(pipelineId: string, next: string | null) => void>(setOutputSelectionForPipeline)) {
      setOutputSelectionForPipeline(pipelineId, next);
    }
  };
  const setLiveOutputSelection = async (next: string | null): Promise<void> => {
    if (isFunction<(next: string | null) => Promise<unknown> | unknown>(setLivePipelineOutput)) {
      await setLivePipelineOutput(next);
    }
  };
  const normalizedStreamId = $derived.by(() => normalizeStreamId(streamId));
  $effect(() => {
    void pipelineGridRowIndices;
    void pipelineGridColumnIndices;
  });
  const profilerStreamOptions = $derived.by(() => {
    if (!normalizedStreamId) return [];
    const label = typeof streamLabel === 'string' && streamLabel.trim().length ? streamLabel.trim() : normalizedStreamId;
    return [{ id: normalizedStreamId, label }];
  });

  const openProfilerPanel = (pipelineId: string) => (event: Event) => {
    event.stopPropagation();
    event.preventDefault();
    if (!normalizedStreamId) return;
    const normalizedPipelineId = typeof pipelineId === 'string' ? pipelineId.trim() : '';
    if (!normalizedPipelineId) return;
    profilerPipelineId = normalizedPipelineId === RAW_PIPELINE_ID ? RAW_PIPELINE_UUID : normalizedPipelineId;
    profilerPipelineLabel = resolvePipelineLabel(normalizedPipelineId);
    profilerOpen = true;
  };

  const openOutputsViewerForPipeline = async (pipelineId: string | null): Promise<void> => {
    const normalizedStreamId = typeof streamId === 'string' ? streamId.trim() : '';
    if (!normalizedStreamId) return;

    const normalizedPipelineId = typeof pipelineId === 'string' ? pipelineId.trim() : '';

    let portTypesByName: Record<string, PipelineDataType | null | undefined> = {};
    if (
      normalizedPipelineId &&
      normalizedPipelineId !== RAW_PIPELINE_ID &&
      typeof ensurePipelineGraphAndOutputs === 'function'
    ) {
      try {
        const result = (await ensurePipelineGraphAndOutputs(normalizedPipelineId, false)) as EnsurePipelineGraphResult | null;
        const hydrated = result?.types ?? null;
        if (hydrated && typeof hydrated === 'object') {
          portTypesByName = hydrated as Record<string, PipelineDataType | null | undefined>;
        } else {
          const graphJson = result?.graphJson ?? null;
          if (graphJson) {
            portTypesByName = extractGraphOutputPortTypes(graphJson);
          }
        }
      } catch {
        portTypesByName = {};
      }
    }

    floatingPipelineOutputsViewer.open({
      streamId: normalizedStreamId,
      streamLabel: typeof streamLabel === 'string' && streamLabel.trim() ? streamLabel.trim() : null,
      portTypesByName
    });
  };

  let outputOptionsFallback = $state<Record<string, string[]>>({});
  let outputOptionsLoading = new SvelteSet<string>();
  const safeRows = $derived.by(() => Math.min(Math.max(Math.trunc(pipelineGridRows ?? 1), 1), 6));
  const safeColumns = $derived.by(() => Math.min(Math.max(Math.trunc(pipelineGridColumns ?? 1), 1), 6));
  const rowIndices = $derived.by(() => Array.from({ length: safeRows }, (_, i) => i));
  const columnIndices = $derived.by(() => Array.from({ length: safeColumns }, (_, i) => i));
  const dedupePipelineIds = (ids: unknown[]): string[] => {
    const out: string[] = [];
    const seen = new SvelteSet<string>();
    for (const value of ids ?? []) {
      const id = normalizeId(value);
      if (!id.length || seen.has(id)) continue;
      seen.add(id);
      out.push(id);
    }
    return out;
  };
  const dedupeGraphsById = (graphs: unknown[]): PipelineGraphEntry[] => {
    const out: PipelineGraphEntry[] = [];
    const seen = new SvelteSet<string>();
    for (const graph of graphs ?? []) {
      const graphRecord = asGraphEntry(graph);
      const id = normalizeId(graphRecord?.id);
      if (!id.length || seen.has(id)) continue;
      seen.add(id);
      if (graphRecord && graphRecord.id !== id) {
        out.push({ ...graphRecord, id });
      } else if (graphRecord) {
        out.push(graphRecord);
      }
    }
    return out;
  };
  const normalizeKey = (value: unknown): string | null => {
    const normalized = normalizeId(value);
    return normalized.length ? normalized : null;
  };
  const normalizePort = (value: unknown, fallback = 'frame'): string => {
    const normalized = normalizeId(value);
    return normalized.length ? normalized : fallback;
  };
  const hasLoadedOutputOptions = (pipelineId: string): boolean => {
    if (!pipelineId.length) return false;
    const hasPrimary =
      pipelineOutputOptionsCache != null && Object.prototype.hasOwnProperty.call(pipelineOutputOptionsCache, pipelineId);
    if (hasPrimary) return true;
    return Object.prototype.hasOwnProperty.call(outputOptionsFallback, pipelineId);
  };
  const normalizePipelineIdForUi = (value: unknown): string => {
    const normalized = normalizeId(value);
    if (!normalized.length) return '';
    if (normalized === normalizeId(RAW_PIPELINE_UUID)) return RAW_PIPELINE_ID;
    return normalized;
  };
  const normalizedPipelineWires = $derived.by(() => (Array.isArray(pipelineWires) ? pipelineWires : []));
  type GridPipelineEntry = {
    row: number;
    column: number;
    pipelineId: string;
    outputKey: string | null;
    resolvedPort: string;
  };
  const gridPipelineEntries = $derived.by<GridPipelineEntry[]>(() => {
    const entries: GridPipelineEntry[] = [];
    rowIndices.forEach((row) => {
      columnIndices.forEach((column) => {
        const pipelineIdRaw = readPipelineForCell(row, column);
        const pipelineId = normalizeId(pipelineIdRaw);
        if (!pipelineId.length) return;
        const outputKey = normalizeKey(readOutputKeyForCell(row, column));
        entries.push({
          row,
          column,
          pipelineId,
          outputKey,
          resolvedPort: outputKey ?? 'frame'
        });
      });
    });
    return entries;
  });

  const currentInputSelectionForTarget = (targetPipelineId: string, targetOutputKey: string | null): string => {
    const target = normalizePipelineIdForUi(targetPipelineId);
    if (!target.length || target === RAW_PIPELINE_ID) return 'raw|raw';
    const targetWireId = target === RAW_PIPELINE_ID ? normalizeId(RAW_PIPELINE_UUID) : target;
    const frameWire = normalizedPipelineWires.find((wire) => {
      const wireRecord = asRecord(wire);
      const to = asRecord(wireRecord?.to) as PipelineWireEndpoint | null;
      const wireToId = normalizeId(to?.pipeline_id);
      if (!wireToId.length) return false;
      if (wireToId !== targetWireId) return false;
      const wireToPort = normalizePort(to?.port, 'frame').toLowerCase();
      if (wireToPort !== 'frame') return false;
      const wireToKey = normalizeKey(to?.output_key);
      return (wireToKey ?? null) === (targetOutputKey ?? null);
    });
    const frameWireRecord = asRecord(frameWire);
    const from = asRecord(frameWireRecord?.from) as PipelineWireEndpoint | null;
    const fromIdRaw = normalizeId(from?.pipeline_id);
    if (!fromIdRaw.length) return 'raw|raw';
    const fromId = normalizePipelineIdForUi(fromIdRaw);
    if (fromId === RAW_PIPELINE_ID) {
      const selected = normalizeId(from?.port) || normalizeId(from?.output_key) || 'raw';
      const canonical = selected.toLowerCase() === 'frame' ? 'raw' : selected;
      return `raw|${canonical}`;
    }
    const fromKey = normalizeKey(from?.output_key) ?? '';
    const fromPort = normalizePort(from?.port, 'frame');
    return `pipe|${fromId}|${fromKey}|${fromPort}`;
  };

  const applyInputSelectionForTarget = (selection: string, targetPipelineId: string, targetOutputKey: string | null): void => {
    const trimmed = selection.trim();
    if (!trimmed || !setFrameSourceForPipelineInstance) return;
    if (trimmed.startsWith('raw|')) {
      const parts = trimmed.split('|');
      const port = (parts[1] ?? 'raw').trim() || 'raw';
      void setFrameSourceForPipelineInstance({
        to: { pipelineId: targetPipelineId, outputKey: targetOutputKey },
        from: { pipelineId: RAW_PIPELINE_ID, outputKey: null, port }
      });
      return;
    }
    if (trimmed.startsWith('pipe|')) {
      const parts = trimmed.split('|');
      const fromId = (parts[1] ?? '').trim();
      if (!fromId.length) return;
      const fromKey = (parts[2] ?? '').trim() || null;
      const port = (parts[3] ?? '').trim() || null;
      void setFrameSourceForPipelineInstance({
        to: { pipelineId: targetPipelineId, outputKey: targetOutputKey },
        from: { pipelineId: fromId, outputKey: fromKey, port }
      });
    }
  };

  let suppressDrag = $state(false);

  const startTunerInteraction = (event: Event) => {
    event.stopPropagation();
    suppressDrag = true;
  };

  const endTunerInteraction = (event: Event) => {
    event.stopPropagation();
    suppressDrag = false;
  };

  const openTunerPanel = (pipelineId: string) => (event: Event) => {
    event.stopPropagation();
    event.preventDefault();
    openPipelineTuningPanel?.(pipelineId);
  };

  const requestOutputOptions = async (pipelineId: string) => {
    if (!ensurePipelineGraphAndOutputs) return;
    if (outputOptionsLoading.has(pipelineId)) return;
    outputOptionsLoading = new SvelteSet(outputOptionsLoading).add(pipelineId);
    try {
      const result = await ensurePipelineGraphAndOutputs(pipelineId);
      const outputs = Array.isArray(result?.filtered) ? result.filtered : [];
      if (outputs.length) {
        outputOptionsFallback = { ...outputOptionsFallback, [pipelineId]: outputs };
      }
    } catch {
      // ignore fetch errors
    } finally {
      const next = new SvelteSet(outputOptionsLoading);
      next.delete(pipelineId);
      outputOptionsLoading = next;
    }
  };

  $effect(() => {
    if (!ensurePipelineOutputsLoaded) return;
    const missing = new SvelteSet<string>();
    (assignedPipelineIds ?? []).forEach((id) => {
      const normalized = String(id ?? '').trim();
      if (!normalized || normalized === RAW_PIPELINE_ID) return;
      if (!hasLoadedOutputOptions(normalized)) missing.add(normalized);
    });
    rowIndices.forEach((row) => {
      columnIndices.forEach((column) => {
        const pipelineId = readPipelineForCell(row, column);
        const normalized = typeof pipelineId === 'string' ? pipelineId.trim() : '';
        if (!normalized || normalized === RAW_PIPELINE_ID) return;
        if (!hasLoadedOutputOptions(normalized)) missing.add(normalized);
      });
    });
    missing.forEach((pipelineId) => {
      void ensurePipelineOutputsLoaded(pipelineId);
      void requestOutputOptions(pipelineId);
    });
  });

  function issueCountForGraph(graph: PipelineGraphEntry | null | undefined): number {
    const raw = graph?.issue_count ?? graph?.issueCount ?? 0;
    const count = Number(raw);
    return Number.isFinite(count) ? Math.max(0, Math.floor(count)) : 0;
  }

  function issueCountForPipeline(pipelineId: string): number {
    if (!pipelineId) return 0;
    const entry = pipelineGraphsList.find((graph) => String(graph?.id ?? '') === String(pipelineId)) ?? null;
    return entry ? issueCountForGraph(entry) : 0;
  }

  const normalizeGraphList = (value: unknown): PipelineGraphEntry[] | null =>
    (Array.isArray(value) ? dedupeGraphsById(value) : null);

  function filterAndSortGraphs(graphs: PipelineGraphEntry[], query: string): PipelineGraphEntry[] {
    const q = query?.trim?.().toLowerCase?.() ?? '';
    const filtered = dedupeGraphsById(graphs).filter((graph) => {
      const id = String(graph?.id ?? '');
      const name = graph?.name?.trim?.() ? String(graph.name).trim() : '';
      if (!q.length) return true;
      return `${name} ${id}`.toLowerCase().includes(q);
    });
    return filtered.slice().sort((a, b) => String(a?.name ?? a?.id ?? '').localeCompare(String(b?.name ?? b?.id ?? '')));
  }

  let pipelineGraphsSnapshot = $state<PipelineGraphEntry[]>([]);

  $effect(() => {
    if (isSubscribable<unknown>(pipelineGraphs)) {
      const unsubscribe = pipelineGraphs.subscribe((value: unknown) => {
        const next = normalizeGraphList(value) ?? [];
        if (pipelineAssignModalOpen && next.length === 0 && pipelineGraphsSnapshot.length > 0) return;
        pipelineGraphsSnapshot = next;
      });
      return () => {
        if (typeof unsubscribe === 'function') {
          unsubscribe();
          return;
        }
        if (unsubscribe && typeof unsubscribe === 'object') {
          unsubscribe.unsubscribe?.();
        }
      };
    }
    const next = normalizeGraphList(pipelineGraphs) ?? [];
    if (pipelineAssignModalOpen && next.length === 0 && pipelineGraphsSnapshot.length > 0) return;
    pipelineGraphsSnapshot = next;
  });

  const pipelineGraphsList = $derived.by(() => pipelineGraphsSnapshot);
  const assignedPipelineList = $derived.by(() => dedupePipelineIds(assignedPipelineIds ?? []));
  const pipelineAssignList = $derived.by(() => {
    const override = normalizeGraphList(pipelineAssignFilteredGraphs);
    if (override && override.length) return override;
    return filterAndSortGraphs(pipelineGraphsSnapshot, pipelineAssignQuery);
  });

  const normalizeTemplateList = (value: unknown): PipelineTemplateSummary[] => {
    if (!Array.isArray(value)) return [];
    const out: PipelineTemplateSummary[] = [];
    const seen = new SvelteSet<string>();
    for (const entry of value) {
      if (!entry || typeof entry !== 'object') continue;
      const record = entry as Record<string, unknown>;
      const templateId =
        typeof record.templateId === 'string'
          ? record.templateId.trim()
          : typeof record.template_id === 'string'
            ? record.template_id.trim()
            : '';
      const name = typeof record.name === 'string' ? record.name.trim() : '';
      if (!templateId.length || !name.length || seen.has(templateId)) continue;
      seen.add(templateId);
      out.push({
        templateId,
        name,
        summary: typeof record.summary === 'string' || record.summary == null ? (record.summary as string | null) : null
      });
    }
    return out;
  };

  let pipelineTemplateOptions = $state<PipelineTemplateSummary[]>([]);
  let pipelineTemplateSelectedId = $state<string>('');
  let pipelineTemplateLoading = $state(false);
  let pipelineTemplateCreating = $state(false);
  let pipelineTemplateError = $state<string | null>(null);
  let pipelineTemplateStatus = $state<string | null>(null);
  let pipelineTemplateLoadRequested = $state(false);

  async function refreshPipelineGraphsSnapshot(): Promise<void> {
    if (!refreshPipelineGraphs) return;
    const next = await refreshPipelineGraphs();
    pipelineGraphsSnapshot = normalizeGraphList(next) ?? [];
  }

  async function refreshPipelineTemplateOptions(): Promise<void> {
    if (!listPipelineTemplatesForAssign) return;
    pipelineTemplateLoading = true;
    pipelineTemplateError = null;
    try {
      const templates = normalizeTemplateList(await listPipelineTemplatesForAssign());
      pipelineTemplateOptions = templates;
      if (!templates.some((entry) => entry.templateId === pipelineTemplateSelectedId)) {
        pipelineTemplateSelectedId = templates[0]?.templateId ?? '';
      }
    } catch (error) {
      pipelineTemplateError = error instanceof Error ? error.message : 'Unable to load templates.';
      pipelineTemplateOptions = [];
      pipelineTemplateSelectedId = '';
    } finally {
      pipelineTemplateLoading = false;
      pipelineTemplateLoadRequested = true;
    }
  }

  async function createPipelineFromTemplate(): Promise<void> {
    if (!createPipelineFromTemplateAndAssign) return;
    const templateId = pipelineTemplateSelectedId.trim();
    if (!templateId.length) {
      pipelineTemplateError = 'Select a template first.';
      return;
    }
    pipelineTemplateCreating = true;
    pipelineTemplateError = null;
    pipelineTemplateStatus = null;
    try {
      const created = await createPipelineFromTemplateAndAssign(templateId);
      const createdId = normalizeId((created as { id?: string } | null)?.id ?? '');
      if (createdId.length && !pipelineAssignDraft.includes(createdId)) {
        pipelineAssignDraft = Array.from(new SvelteSet([...pipelineAssignDraft, createdId]));
      }
      const createdName = typeof (created as { name?: string } | null)?.name === 'string' ? created.name.trim() : '';
      pipelineTemplateStatus = createdName.length
        ? `Created "${createdName}". Click Save to attach it.`
        : 'Template pipeline created. Click Save to attach it.';
      pipelineAssignQuery = '';
      await refreshPipelineGraphsSnapshot();
    } catch (error) {
      pipelineTemplateError = error instanceof Error ? error.message : 'Unable to create pipeline from template.';
    } finally {
      pipelineTemplateCreating = false;
    }
  }

  let assignModalRefreshRequested = $state(false);
  $effect(() => {
    if (!pipelineAssignModalOpen) {
      assignModalRefreshRequested = false;
      return;
    }
    if (assignModalRefreshRequested) return;
    if (pipelineGraphLoading) return;
    if (pipelineGraphsList.length === 0) {
      assignModalRefreshRequested = true;
      void refreshPipelineGraphsSnapshot();
    }
  });

  $effect(() => {
    if (!pipelineAssignModalOpen) {
      pipelineTemplateLoadRequested = false;
      pipelineTemplateError = null;
      pipelineTemplateStatus = null;
      return;
    }
    if (!listPipelineTemplatesForAssign) return;
    if (pipelineTemplateLoadRequested || pipelineTemplateLoading) return;
    void refreshPipelineTemplateOptions();
  });

  $effect(() => {
    if (!pipelineAssignModalOpen) return;
    if (!pipelineTemplateOptions.some((entry) => entry.templateId === pipelineTemplateSelectedId)) {
      pipelineTemplateSelectedId = pipelineTemplateOptions[0]?.templateId ?? '';
    }
  });

  $effect(() => {
    if (!pipelineAssignModalOpen) return;
    if (!pipelineGraphsSnapshot.length) return;
    const query = pipelineAssignQuery?.trim?.() ?? '';
    if (!query) return;
    const filtered = filterAndSortGraphs(pipelineGraphsSnapshot, query);
    if (filtered.length === 0) {
      pipelineAssignQuery = '';
    }
  });

  function toggleAssignDraft(pipelineId: string, checked: boolean): void {
    const current = new SvelteSet(pipelineAssignDraft);
    if (checked) current.add(pipelineId);
    else current.delete(pipelineId);
    pipelineAssignDraft = Array.from(current);
  }

  const gridSectionState = $derived.by(() => ({
    showAssignControls,
    openPipelineAssignModal,
    pipelineGraphLoading,
    pipelineGraphError,
    assignedPipelineList,
    dragStartHandlerFor,
    RAW_PIPELINE_ID,
    resolvePipelineLabel,
    outputsIconEnabled,
    openOutputsViewerForPipeline,
    openPipelineTuningPanel,
    normalizedStreamId,
    openProfilerPanel,
    showRemoveControls,
    openPipelineRemoveModal,
    get pipelineGridRows() {
      return pipelineGridRows;
    },
    set pipelineGridRows(value: number) {
      pipelineGridRows = value;
    },
    get pipelineGridColumns() {
      return pipelineGridColumns;
    },
    set pipelineGridColumns(value: number) {
      pipelineGridColumns = value;
    },
    applyGridDimensions,
    safeRows,
    safeColumns,
    gridSignature,
    pipelineGridRowIndices: rowIndices,
    pipelineGridColumnIndices: columnIndices,
    readPipelineForCell,
    readOutputKeyForCell,
    readOutputSelectionForPipeline,
    pipelineOutputOptionsCache,
    pipelineGridIsSingle,
    get selectedPipelineOutput() {
      return selectedPipelineOutput;
    },
    set selectedPipelineOutput(value: string | null) {
      selectedPipelineOutput = value;
    },
    pipelineWires: normalizedPipelineWires,
    setFrameSourceForPipelineInstance,
    gridPipelineEntries,
    setOutputKeyForCell,
    clearGridCell,
    applyAllowDrop,
    dropHandlerForCell,
    setLiveOutputSelection
  }));

  const modalState = $derived.by(() => ({
    showRemoveControls,
    pipelineRemoveModalOpen,
    resolvePipelineLabel,
    pipelineRemoveCandidateId,
    closePipelineRemoveModal,
    confirmPipelineRemove,
    showAssignControls,
    pipelineAssignModalOpen,
    get pipelineAssignQuery() {
      return pipelineAssignQuery;
    },
    set pipelineAssignQuery(value: string) {
      pipelineAssignQuery = value;
    },
    refreshPipelineGraphs: () => void refreshPipelineGraphsSnapshot(),
    pipelineAssignFilteredGraphs: pipelineAssignList,
    normalizeId,
    get pipelineAssignDraft() {
      return pipelineAssignDraft;
    },
    set pipelineAssignDraft(value: string[]) {
      pipelineAssignDraft = value;
    },
    toggleAssignDraft,
    get pipelineTemplateSelectedId() {
      return pipelineTemplateSelectedId;
    },
    set pipelineTemplateSelectedId(value: string) {
      pipelineTemplateSelectedId = value;
      pipelineTemplateError = null;
      pipelineTemplateStatus = null;
    },
    pipelineTemplateLoading,
    pipelineTemplateOptions,
    pipelineTemplateBusy: pipelineTemplateLoading || pipelineTemplateCreating,
    pipelineTemplateError: pipelineTemplateError ?? pipelineTemplateStatus,
    handleTemplateAssign: () => createPipelineFromTemplate(),
    closePipelineAssignModal,
    savePipelineAssignModal
  }));
</script>

<div class="flex h-full min-h-0 flex-col gap-3">
  <CameraPipelinesGridSection state={gridSectionState} />
</div>

<PipelineProfilerModal
  open={profilerOpen}
  title="Profiler"
  pipelineId={profilerPipelineId}
  pipelineLabel={profilerPipelineLabel}
  streamId={normalizedStreamId || null}
  streamOptions={profilerStreamOptions}
  onClose={() => {
    profilerOpen = false;
    profilerPipelineId = null;
    profilerPipelineLabel = null;
  }}
/>

<CameraPipelinesModals state={modalState} />
