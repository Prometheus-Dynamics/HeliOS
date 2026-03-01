<script lang="ts">
  import { PipelineIcon } from '$lib';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import PipelineProfilerModal from '$lib/components/pipelines/PipelineProfilerModal.svelte';
  import { faCamera, faCode, faPlus, faSliders, faStopwatch, faTrash, faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';
  import { floatingPipelineOutputsViewer } from '$lib/stores/floatingPipelineOutputsViewer';
  import { extractGraphOutputPortTypes } from '$lib/features/pipelines/outputFilters';
  import type { PipelineDataType, PipelineTemplateSummary } from '$lib/types/pipeline';

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
  void pipelineGridRowIndices;
  void pipelineGridColumnIndices;

  let profilerOpen = $state(false);
  let profilerPipelineId = $state<string | null>(null);
  let profilerPipelineLabel = $state<string | null>(null);

  const normalizeStreamId = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');
  const normalizedStreamId = $derived.by(() => normalizeStreamId(streamId));
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
    profilerPipelineLabel = pipelineLabel(normalizedPipelineId);
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
        const result = await ensurePipelineGraphAndOutputs(normalizedPipelineId, false);
        const hydrated = (result as any)?.types ?? null;
        if (hydrated && typeof hydrated === 'object') {
          portTypesByName = hydrated as Record<string, PipelineDataType | null | undefined>;
        } else {
          const graphJson = (result as any)?.graphJson ?? null;
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
  let outputOptionsLoading = $state<Set<string>>(new Set());
  const safeRows = $derived.by(() => Math.min(Math.max(Math.trunc(pipelineGridRows ?? 1), 1), 6));
  const safeColumns = $derived.by(() => Math.min(Math.max(Math.trunc(pipelineGridColumns ?? 1), 1), 6));
  const rowIndices = $derived.by(() => Array.from({ length: safeRows }, (_, i) => i));
  const columnIndices = $derived.by(() => Array.from({ length: safeColumns }, (_, i) => i));
  const normalizeId = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');
  const dedupePipelineIds = (ids: unknown[]): string[] => {
    const out: string[] = [];
    const seen = new Set<string>();
    for (const value of ids ?? []) {
      const id = normalizeId(value);
      if (!id.length || seen.has(id)) continue;
      seen.add(id);
      out.push(id);
    }
    return out;
  };
  const dedupeGraphsById = (graphs: unknown[]): any[] => {
    const out: any[] = [];
    const seen = new Set<string>();
    for (const graph of graphs ?? []) {
      const id = normalizeId((graph as any)?.id);
      if (!id.length || seen.has(id)) continue;
      seen.add(id);
      if (graph && typeof graph === 'object' && (graph as any).id !== id) {
        out.push({ ...(graph as Record<string, unknown>), id });
      } else {
        out.push(graph);
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
        const pipelineIdRaw = pipelineForCell(row, column);
        const pipelineId = normalizeId(pipelineIdRaw);
        if (!pipelineId.length) return;
        const outputKey = normalizeKey(outputKeyForCell?.(row, column));
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
      const to = (wire as any)?.to ?? null;
      const wireToId = normalizeId(to?.pipeline_id);
      if (!wireToId.length) return false;
      if (wireToId !== targetWireId) return false;
      const wireToPort = normalizePort(to?.port, 'frame').toLowerCase();
      if (wireToPort !== 'frame') return false;
      const wireToKey = normalizeKey(to?.output_key);
      return (wireToKey ?? null) === (targetOutputKey ?? null);
    });
    const from = (frameWire as any)?.from ?? null;
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
    outputOptionsLoading = new Set(outputOptionsLoading).add(pipelineId);
    try {
      const result = await ensurePipelineGraphAndOutputs(pipelineId);
      const outputs = Array.isArray(result?.filtered) ? result.filtered : [];
      if (outputs.length) {
        outputOptionsFallback = { ...outputOptionsFallback, [pipelineId]: outputs };
      }
    } catch {
      // ignore fetch errors
    } finally {
      const next = new Set(outputOptionsLoading);
      next.delete(pipelineId);
      outputOptionsLoading = next;
    }
  };

  $effect(() => {
    if (!ensurePipelineOutputsLoaded) return;
    const missing = new Set<string>();
    (assignedPipelineIds ?? []).forEach((id) => {
      const normalized = String(id ?? '').trim();
      if (!normalized || normalized === RAW_PIPELINE_ID) return;
      if (!hasLoadedOutputOptions(normalized)) missing.add(normalized);
    });
    rowIndices.forEach((row) => {
      columnIndices.forEach((column) => {
        const pipelineId = pipelineForCell(row, column);
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

  function issueCountForGraph(graph: any): number {
    const raw = graph?.issue_count ?? graph?.issueCount ?? 0;
    const count = Number(raw);
    return Number.isFinite(count) ? Math.max(0, Math.floor(count)) : 0;
  }

  function issueCountForPipeline(pipelineId: string): number {
    if (!pipelineId) return 0;
    const entry = pipelineGraphsList.find((graph) => String(graph?.id ?? '') === String(pipelineId)) ?? null;
    return entry ? issueCountForGraph(entry) : 0;
  }

  const normalizeGraphList = (value: unknown): any[] | null =>
    (Array.isArray(value) ? dedupeGraphsById(value) : null);

  function filterAndSortGraphs(graphs: any[], query: string): any[] {
    const q = query?.trim?.().toLowerCase?.() ?? '';
    const filtered = dedupeGraphsById(graphs).filter((graph) => {
      const id = String(graph?.id ?? '');
      const name = graph?.name?.trim?.() ? String(graph.name).trim() : '';
      if (!q.length) return true;
      return `${name} ${id}`.toLowerCase().includes(q);
    });
    return filtered.slice().sort((a, b) => String(a?.name ?? a?.id ?? '').localeCompare(String(b?.name ?? b?.id ?? '')));
  }

  let pipelineGraphsSnapshot = $state<any[]>([]);

  $effect(() => {
    if (pipelineGraphs && typeof pipelineGraphs === 'object' && 'subscribe' in pipelineGraphs) {
      const unsubscribe = (pipelineGraphs as any).subscribe((value: unknown) => {
        const next = normalizeGraphList(value) ?? [];
        if (pipelineAssignModalOpen && next.length === 0 && pipelineGraphsSnapshot.length > 0) return;
        pipelineGraphsSnapshot = next;
      });
      return () => unsubscribe?.();
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
    const seen = new Set<string>();
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
        pipelineAssignDraft = Array.from(new Set([...pipelineAssignDraft, createdId]));
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
</script>

<div class="flex flex-col gap-3 min-h-0 h-full">
  <div class="flex flex-col gap-3 min-h-0 flex-1">
    <div class="rounded border border-surface-800/70 bg-surface-950/70 p-4">
      <div class="flex items-start justify-between gap-3">
        <div>
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Palette</p>
          <p class="mt-1 text-xs text-surface-400">Drag attached pipelines into the grid. Use trash to detach.</p>
          {#if pipelineGraphError}
            <p class="mt-1 text-xs text-error-300">{pipelineGraphError}</p>
          {/if}
        </div>
        {#if showAssignControls}
          <button
            class="btn btn-3xs preset-tonal uppercase tracking-[0.3em] flex items-center gap-2"
            type="button"
            onclick={openPipelineAssignModal}
            disabled={pipelineGraphLoading}
            aria-label="Attach pipeline"
          >
            <FaIcon icon={faPlus} class="h-3.5 w-3.5" />
            Add
          </button>
        {/if}
      </div>

      <div class="mt-3 flex gap-2 overflow-x-auto pb-1" role="list" aria-label="Pipeline palette">
        <div
          class="group flex shrink-0 items-center gap-2 rounded border border-surface-800/70 bg-surface-900/40 px-2 py-1 text-surface-200 hover:border-primary-500/40"
          role="button"
          tabindex="0"
          draggable="true"
          ondragstart={handlePipelineDragStart(RAW_PIPELINE_ID)}
          title="Raw stream"
          aria-label="Raw stream"
        >
          <span class="flex h-8 w-8 items-center justify-center rounded-full border border-white/15 bg-black/40 text-surface-100">
            <FaIcon icon={faCamera} class="h-4 w-4" />
          </span>
          <span class="max-w-[180px] truncate text-xs">Raw stream</span>
        </div>

        {#each assignedPipelineList as pipelineId (pipelineId)}
          {@const issueCount = issueCountForPipeline(pipelineId)}
            <div
              class="group flex shrink-0 items-center gap-2 rounded border border-surface-800/70 bg-surface-900/40 px-2 py-1 text-surface-200 hover:border-primary-500/40"
              role="button"
              tabindex="0"
              draggable="true"
              ondragstart={(event) => {
                if (suppressDrag) {
                  event.preventDefault();
                  return;
                }
                handlePipelineDragStart(pipelineId)(event as DragEvent);
              }}
              title={pipelineId}
              aria-label={pipelineLabel(pipelineId)}
            >
            <PipelineIcon
              pipelineId={pipelineId}
              size="sm"
              className="shrink-0"
              ariaLabel={pipelineLabel(pipelineId)}
              title={pipelineId}
            />
            <span class="max-w-[180px] truncate text-xs">{pipelineLabel(pipelineId)}</span>
            {#if issueCount > 0}
              <span
                class="inline-flex h-6 w-6 items-center justify-center rounded-full border border-amber-500/60 bg-amber-500/10 text-amber-200"
                title={`Pipeline has ${issueCount} validation issue${issueCount === 1 ? '' : 's'}.`}
                aria-label={`${issueCount} validation issue${issueCount === 1 ? '' : 's'}`}
              >
                <FaIcon icon={faTriangleExclamation} class="h-3 w-3" />
              </span>
            {/if}
            {#if openPipelineTuningPanel}
              <button
                class="ml-1 inline-flex h-6 w-6 items-center justify-center rounded border border-transparent text-surface-400 hover:border-primary-500/40 hover:text-primary-200"
                type="button"
                draggable="false"
                onpointerdown={startTunerInteraction}
                onpointerup={endTunerInteraction}
                onpointercancel={endTunerInteraction}
                onpointerleave={endTunerInteraction}
                onclick={openTunerPanel(pipelineId)}
                ondragstart={(e) => e.preventDefault()}
                aria-label="Open pipeline tuner"
                title="Open pipeline tuner"
              >
                <FaIcon icon={faSliders} class="h-3.5 w-3.5" />
              </button>
            {/if}
            <button
              class="ml-1 inline-flex h-6 w-6 items-center justify-center rounded border border-transparent text-surface-400 hover:border-primary-500/40 hover:text-primary-200 disabled:opacity-50"
              type="button"
              draggable="false"
              onpointerdown={startTunerInteraction}
              onpointerup={endTunerInteraction}
              onpointercancel={endTunerInteraction}
              onpointerleave={endTunerInteraction}
              onclick={openProfilerPanel(pipelineId)}
              ondragstart={(e) => e.preventDefault()}
              aria-label="Open profiler"
              title="Open profiler"
              disabled={!normalizedStreamId}
            >
              <FaIcon icon={faStopwatch} class="h-3.5 w-3.5" />
            </button>
            {#if outputsIconEnabled}
              <button
                class="ml-1 inline-flex h-6 w-6 items-center justify-center rounded border border-transparent text-surface-400 hover:border-primary-500/40 hover:text-primary-200"
                type="button"
                draggable="false"
                onpointerdown={startTunerInteraction}
                onpointerup={endTunerInteraction}
                onpointercancel={endTunerInteraction}
                onpointerleave={endTunerInteraction}
                onclick={(e) => {
                  e.stopPropagation();
                  void openOutputsViewerForPipeline(pipelineId);
                }}
                ondragstart={(e) => e.preventDefault()}
                aria-label="Open outputs viewer"
                title="Open outputs viewer"
                disabled={!streamId}
              >
                <FaIcon icon={faCode} class="h-3.5 w-3.5" />
              </button>
            {/if}
            {#if showRemoveControls}
              <button
                class="ml-1 inline-flex h-6 w-6 items-center justify-center rounded border border-transparent text-surface-400 hover:border-error-500/40 hover:text-error-200"
                type="button"
                onclick={(e) => {
                  e.stopPropagation();
                  openPipelineRemoveModal(pipelineId);
                }}
                aria-label="Remove pipeline"
                title="Remove pipeline"
              >
                <FaIcon icon={faTrash} class="h-3.5 w-3.5" />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    </div>

    <div class="rounded border border-surface-800/70 bg-surface-950/70 p-4 flex flex-col min-h-0 flex-1">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Layout</p>
          <p class="mt-1 text-xs text-surface-400">
            {pipelineGridIsSingle ? 'Single view (1×1).' : 'Multiplex view.'} Drag pipelines into slots.
          </p>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => setPipelineGridDimensions(1, 1)}>
            1×1
          </button>
        </div>
      </div>

      <div class="mt-3 grid gap-3 sm:grid-cols-2">
        <label class="text-sm">
          <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Rows</span>
          <input
            class="mt-1 w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2"
            type="number"
            min="1"
            max="6"
            value={pipelineGridRows}
            onchange={(e) => setPipelineGridDimensions(Number((e.currentTarget as HTMLInputElement).value), pipelineGridColumns)}
          />
        </label>
        <label class="text-sm">
          <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Columns</span>
          <input
            class="mt-1 w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2"
            type="number"
            min="1"
            max="6"
            value={pipelineGridColumns}
            onchange={(e) => setPipelineGridDimensions(pipelineGridRows, Number((e.currentTarget as HTMLInputElement).value))}
          />
        </label>
      </div>

      <div
        class="mt-4 grid gap-2 flex-1 min-h-0"
        style={`grid-template-columns: repeat(${safeColumns}, minmax(0, 1fr)); grid-template-rows: repeat(${safeRows}, minmax(140px, 1fr)); min-height: ${safeRows * 140}px; grid-auto-flow: row;`}
        data-grid-signature={gridSignature ?? ''}
        role="grid"
        aria-label="Pipeline layout grid"
      >
        {#each rowIndices as row (row)}
          {#each columnIndices as column (column)}
            {@const cellPipeline = pipelineForCell(row, column)}
            {@const isOutputCell = row === 0 && column === 0}
            {@const isMultiplex = safeRows * safeColumns > 1}
            {@const outputValue = cellPipeline ? (isMultiplex ? outputKeyForCell?.(row, column) : outputSelectionForPipeline(cellPipeline)) : null}
            {@const outputOptions =
              cellPipeline
                ? (cellPipeline === RAW_PIPELINE_ID ? ['raw', 'undistorted'] : pipelineOutputOptionsCache[cellPipeline] ?? outputOptionsFallback[cellPipeline] ?? [])
                : []}
            {@const canonicalOutputValue =
              cellPipeline === RAW_PIPELINE_ID && typeof outputValue === 'string' && outputValue.trim().toLowerCase() === 'frame'
                ? 'raw'
                : outputValue}
            {@const resolvedOutputValue = canonicalOutputValue ?? outputOptions[0] ?? ''}
            {@const targetOutputKey = typeof outputValue === 'string' && outputValue.trim().length ? outputValue.trim() : null}
            {@const currentInputSelection = cellPipeline ? currentInputSelectionForTarget(cellPipeline, targetOutputKey) : 'raw|raw'}
            {@const outputDurationLabel = cellPipeline ? outputDurationForCell?.(row, column, cellPipeline, resolvedOutputValue) ?? null : null}
            <div
              class="group relative overflow-hidden rounded border border-surface-800/70 bg-surface-900/40 min-h-0"
              role="gridcell"
              tabindex="0"
              aria-label={`Pipeline slot ${row + 1}:${column + 1}`}
              draggable={Boolean(cellPipeline)}
              ondragstart={(event) => {
                if (suppressDrag) {
                  event.preventDefault();
                  return;
                }
                if (!cellPipeline) return;
                handlePipelineDragStart(cellPipeline, { row, column })(event as DragEvent);
              }}
              ondragover={allowDrop}
              ondrop={dropOnCell(row, column)}
            >
              <div class="absolute right-2 top-2 flex items-center gap-2">
                {#if cellPipeline}
                  <button
                    class="btn btn-3xs preset-tonal uppercase tracking-[0.3em] opacity-0 group-hover:opacity-100"
                    type="button"
                    onclick={(e) => {
                      e.stopPropagation();
                      clearCell(row, column);
                    }}
                  >
                    Clear
                  </button>
                {/if}
              </div>
              {#if cellPipeline && (outputsIconEnabled || (openPipelineTuningPanel && cellPipeline !== RAW_PIPELINE_ID) || normalizedStreamId)}
                <div class="absolute bottom-2 right-2 flex items-center gap-2">
                  {#if outputsIconEnabled}
                    <button
                      class="inline-flex h-7 w-7 items-center justify-center rounded-full border border-surface-700/70 bg-surface-900/80 text-surface-200 shadow hover:border-primary-400/60 hover:text-primary-200"
                      type="button"
                      draggable="false"
                      onpointerdown={startTunerInteraction}
                      onpointerup={endTunerInteraction}
                      onpointercancel={endTunerInteraction}
                      onpointerleave={endTunerInteraction}
                      onclick={(e) => {
                        e.stopPropagation();
                        void openOutputsViewerForPipeline(cellPipeline);
                      }}
                      ondragstart={(e) => e.preventDefault()}
                      aria-label="Open outputs viewer"
                      title="Open outputs viewer"
                      disabled={!streamId}
                    >
                      <FaIcon icon={faCode} class="h-3.5 w-3.5" />
                    </button>
                  {/if}
                  {#if openPipelineTuningPanel && cellPipeline !== RAW_PIPELINE_ID}
                    <button
                      class="inline-flex h-7 w-7 items-center justify-center rounded-full border border-surface-700/70 bg-surface-900/80 text-surface-200 shadow hover:border-primary-400/60 hover:text-primary-200"
                      type="button"
                      draggable="false"
                      onpointerdown={startTunerInteraction}
                      onpointerup={endTunerInteraction}
                      onpointercancel={endTunerInteraction}
                      onpointerleave={endTunerInteraction}
                      onclick={openTunerPanel(cellPipeline)}
                      ondragstart={(e) => e.preventDefault()}
                      aria-label="Open pipeline controls"
                      title="Pipeline controls"
                    >
                      <FaIcon icon={faSliders} class="h-3.5 w-3.5" />
                    </button>
                  {/if}
                  <button
                    class="inline-flex h-7 w-7 items-center justify-center rounded-full border border-surface-700/70 bg-surface-900/80 text-surface-200 shadow hover:border-primary-400/60 hover:text-primary-200 disabled:opacity-50"
                    type="button"
                    draggable="false"
                    onpointerdown={startTunerInteraction}
                    onpointerup={endTunerInteraction}
                    onpointercancel={endTunerInteraction}
                    onpointerleave={endTunerInteraction}
                    onclick={openProfilerPanel(cellPipeline)}
                    ondragstart={(e) => e.preventDefault()}
                    aria-label="Open profiler"
                    title="Open profiler"
                    disabled={!normalizedStreamId}
                  >
                    <FaIcon icon={faStopwatch} class="h-3.5 w-3.5" />
                  </button>
                </div>
              {/if}

              <div class="flex h-full flex-col gap-2 p-3 min-h-0">
                {#if cellPipeline}
                  <div class="flex flex-1 min-h-0 items-center justify-center">
                    {#if cellPipeline === RAW_PIPELINE_ID}
                      <span class="flex h-16 w-16 items-center justify-center rounded-full border border-white/15 bg-black/40 text-surface-100">
                        <FaIcon icon={faCamera} class="h-8 w-8" />
                      </span>
                    {:else}
                      <PipelineIcon
                        pipelineId={cellPipeline}
                        size="lg"
                        className={pipelineGridIsSingle ? 'h-24 w-24' : 'h-16 w-16'}
                        ariaLabel={pipelineLabel(cellPipeline)}
                        title={cellPipeline}
                      />
                    {/if}
                  </div>
                  <div class="min-w-0">
                    {#if cellPipeline !== RAW_PIPELINE_ID}
                      <span class="block text-micro-tight uppercase tracking-[0.3em] text-surface-500">Input</span>
                      <select
                        class="mt-1 w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        value={currentInputSelection}
                        onclick={(e) => e.stopPropagation()}
                        onchange={(e) => {
                          const selection = (e.currentTarget as HTMLSelectElement).value ?? '';
                          applyInputSelectionForTarget(selection, cellPipeline, targetOutputKey);
                        }}
                      >
                        <option value="raw|raw">Raw stream: raw</option>
                        <option value="raw|undistorted">Raw stream: undistorted</option>
                        {#each gridPipelineEntries as source (`${source.row}:${source.column}`)}
                          {#if source.pipelineId !== cellPipeline && source.pipelineId !== RAW_PIPELINE_ID}
                            <option value={`pipe|${source.pipelineId}|${source.outputKey ?? ''}|${source.resolvedPort}`}>
                              {pipelineLabel(source.pipelineId)} ({source.row + 1}:{source.column + 1}) - {source.resolvedPort}
                            </option>
                          {/if}
                        {/each}
                      </select>
                    {/if}
                    <span class={`block text-micro-tight uppercase tracking-[0.3em] text-surface-500 ${isMultiplex && cellPipeline !== RAW_PIPELINE_ID ? 'mt-2' : ''}`}>Output</span>
                    {#if outputOptions.length > 1}
                      <select
                        class="mt-1 w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        value={isMultiplex ? resolvedOutputValue : (isOutputCell ? selectedPipelineOutput ?? resolvedOutputValue : resolvedOutputValue)}
                        disabled={outputOptions.length === 0}
                        onclick={(e) => e.stopPropagation()}
                        onchange={(e) => {
                          const raw = (e.currentTarget as HTMLSelectElement).value;
                          const trimmed = raw.trim();
                          const next = trimmed.length ? trimmed : outputOptions[0] ?? null;
                          if (isMultiplex) {
                            setOutputKeyForCell?.(row, column, next);
                            schedulePipelineLayoutApply?.();
                          } else {
                            setOutputSelectionForPipeline(cellPipeline, next);
                            if (isOutputCell) {
                              selectedPipelineOutput = next;
                              void setLivePipelineOutput(next);
                            }
                          }
                        }}
                      >
                        {#each outputOptions as port (port)}
                          <option value={port}>{port}</option>
                        {/each}
                      </select>
                    {:else if outputOptions.length === 1}
                      <p class="mt-1 truncate text-xs text-surface-300">{outputOptions[0]}</p>
                    {:else}
                      <p class="mt-1 text-micro-tight text-surface-600">No host output ports</p>
                    {/if}
                    {#if outputDurationLabel}
                      <p class="mt-1 text-micro-tight uppercase tracking-[0.3em] text-surface-400">Avg {outputDurationLabel}</p>
                    {/if}
                  </div>
                  <div class="min-w-0">
                    <p class="truncate text-xs text-surface-200">{pipelineLabel(cellPipeline)}</p>
                    <p class="truncate text-micro-tight text-surface-500">{cellPipeline}</p>
                  </div>
                {:else}
                  <div class="flex flex-1 flex-col items-center justify-center gap-2">
                    <span class="text-xs text-surface-500">Drop pipeline</span>
                    <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-600">{row + 1}:{column + 1}</span>
                  </div>
                {/if}
              </div>
            </div>
          {/each}
        {/each}
      </div>
    </div>
  </div>
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

{#if showRemoveControls && pipelineRemoveModalOpen}
  {@const pipelineId = pipelineRemoveCandidateId}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 px-4 py-6" role="dialog" aria-modal="true" aria-label="Remove pipeline">
    <div class="w-full max-w-md rounded-lg border border-surface-800 bg-surface-950 p-5 shadow-2xl">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Remove pipeline</p>
          <p class="mt-1 text-sm text-surface-200">Remove this pipeline from the stream?</p>
          {#if pipelineId}
            <p class="mt-1 text-xs text-surface-500">{pipelineLabel(pipelineId)}</p>
          {/if}
          <p class="mt-2 text-xs text-surface-500">It will be detached and removed from any grid slots.</p>
        </div>
      </div>

      <div class="mt-4 flex flex-wrap items-center justify-end gap-2">
        <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={closePipelineRemoveModal}>
          Cancel
        </button>
        <button class="btn btn-3xs preset-filled-error-500 uppercase tracking-[0.3em]" type="button" onclick={confirmPipelineRemove}>
          Remove
        </button>
      </div>
    </div>
  </div>
{/if}

{#if showAssignControls && pipelineAssignModalOpen}
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/70 px-4 py-6" role="dialog" aria-modal="true" aria-label="Assign pipelines">
    <div class="w-full max-w-2xl rounded-lg border border-surface-800 bg-surface-950 p-5 shadow-2xl">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Pipelines</p>
          <p class="mt-1 text-sm text-surface-300">Assign pipelines to this camera.</p>
          <p class="text-xs text-surface-500">Drag assigned pipelines into the grid on the Pipelines tab.</p>
        </div>
        <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={closePipelineAssignModal}>
          Close
        </button>
      </div>

      <div class="mt-4 grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
        <label class="text-sm">
          <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Search</span>
          <input
            class="mt-1 w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2"
            type="search"
            value={pipelineAssignQuery}
            placeholder="name or id…"
            oninput={(e) => (pipelineAssignQuery = (e.currentTarget as HTMLInputElement).value)}
          />
        </label>
        <div class="flex items-end gap-2">
          <button
            class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]"
            type="button"
            onclick={() => (pipelineAssignDraft = pipelineGraphsList.map((g) => String(g.id)))}
            disabled={pipelineGraphsList.length === 0}
          >
            All
          </button>
          <button
            class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]"
            type="button"
            onclick={() => (pipelineAssignDraft = [])}
            disabled={pipelineAssignDraft.length === 0}
          >
            None
          </button>
        </div>
      </div>

      {#if listPipelineTemplatesForAssign && createPipelineFromTemplateAndAssign}
        <div class="mt-3 rounded border border-surface-800/70 bg-surface-900/40 p-3">
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Add from template</p>
          <div class="mt-2 grid gap-2 md:grid-cols-[minmax(0,1fr)_auto]">
            <label class="text-sm">
              <span class="sr-only">Template</span>
              <select
                class="w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2 text-surface-100"
                value={pipelineTemplateSelectedId}
                onchange={(event) => {
                  pipelineTemplateSelectedId = (event.currentTarget as HTMLSelectElement).value;
                  pipelineTemplateError = null;
                  pipelineTemplateStatus = null;
                }}
                disabled={pipelineTemplateLoading || pipelineTemplateCreating || pipelineTemplateOptions.length === 0}
              >
                {#if pipelineTemplateOptions.length === 0}
                  <option value="" disabled>
                    {pipelineTemplateLoading ? 'Loading templates…' : 'No templates available'}
                  </option>
                {:else}
                  {#each pipelineTemplateOptions as template (template.templateId)}
                    <option value={template.templateId}>{template.name}</option>
                  {/each}
                {/if}
              </select>
            </label>
            <button
              class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]"
              type="button"
              onclick={() => void createPipelineFromTemplate()}
              disabled={pipelineTemplateLoading || pipelineTemplateCreating || pipelineTemplateOptions.length === 0}
            >
              {pipelineTemplateCreating ? 'Creating…' : 'Create + select'}
            </button>
          </div>
          {#if pipelineTemplateError}
            <p class="mt-2 text-xs text-error-300">{pipelineTemplateError}</p>
          {:else if pipelineTemplateStatus}
            <p class="mt-2 text-xs text-surface-400">{pipelineTemplateStatus}</p>
          {/if}
        </div>
      {/if}

      <div class="mt-4 max-h-[50vh] max-h-[50svh] max-h-[50dvh] overflow-y-auto rounded border border-surface-800/70 bg-surface-900/40 p-3">
        {#if pipelineGraphError}
          <p class="mb-2 text-xs text-error-300">{pipelineGraphError}</p>
        {/if}
        {#if pipelineGraphsList.length === 0}
          {#if pipelineGraphLoading}
            <p class="text-xs text-surface-500">Loading pipelines…</p>
          {:else}
            <p class="text-xs text-surface-500">No graphs found. Create one on the Pipelines page.</p>
          {/if}
        {:else if pipelineAssignList.length === 0}
          <p class="text-xs text-surface-500">No pipelines match your search.</p>
        {:else}
          <div class="space-y-2">
            {#each pipelineAssignList as graph (graph.id)}
              {@const id = String(graph.id)}
              {@const checked = pipelineAssignDraft.includes(id)}
              {@const issueCount = issueCountForGraph(graph)}
              <label class="flex items-start gap-3 rounded border border-surface-800/60 bg-surface-950/30 px-3 py-2 text-sm text-surface-200 hover:border-primary-500/40">
                <input
                  class="mt-1"
                  type="checkbox"
                  checked={checked}
                  onchange={(e) => {
                    const next = (e.currentTarget as HTMLInputElement).checked;
                    const current = new Set(pipelineAssignDraft);
                    if (next) current.add(id);
                    else current.delete(id);
                    pipelineAssignDraft = Array.from(current);
                  }}
                />
                <PipelineIcon
                  pipelineId={id}
                  size="sm"
                  className="mt-0.5 shrink-0"
                  ariaLabel={graph.name?.trim?.() ? graph.name : graph.id}
                  title={graph.id}
                />
                <span class="min-w-0 flex-1">
                  <span class="block truncate">{graph.name?.trim?.() ? graph.name : graph.id}</span>
                  <span class="block truncate text-xs text-surface-500">{graph.id}</span>
                </span>
                {#if issueCount > 0}
                  <span
                    class="mt-0.5 inline-flex h-5 w-5 items-center justify-center rounded-full border border-amber-500/60 bg-amber-500/10 text-amber-200"
                    title={`Pipeline has ${issueCount} validation issue${issueCount === 1 ? '' : 's'}.`}
                    aria-label={`${issueCount} validation issue${issueCount === 1 ? '' : 's'}`}
                  >
                    <FaIcon icon={faTriangleExclamation} class="h-2.5 w-2.5" />
                  </span>
                {/if}
              </label>
            {/each}
          </div>
        {/if}
      </div>

      <div class="mt-4 flex flex-wrap items-center justify-between gap-3">
        <p class="text-xs text-surface-500">{pipelineAssignDraft.length} assigned</p>
        <div class="flex gap-2">
          <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={closePipelineAssignModal}>
            Cancel
          </button>
          <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={savePipelineAssignModal}>
            Save
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}
