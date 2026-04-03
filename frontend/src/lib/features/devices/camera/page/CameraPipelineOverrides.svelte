<script lang="ts">
  import PipelineUiOverridesPanel from '$lib/components/pipelines/PipelineUiOverridesPanel.svelte';
  import PipelineStreamOverridesPanel from '$lib/components/pipelines/PipelineStreamOverridesPanel.svelte';
  import PipelineIcon from '$lib/components/pipelines/PipelineIcon.svelte';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faCamera } from '@fortawesome/free-solid-svg-icons';
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUi, PipelineUiNodeDescriptor } from '$lib/features/pipelines/pipelineUiTypes';
  import type { PipelineTuningConstantEntry } from '$lib/components/pipelines/types';
  import {
    buildTuneConstantGroups,
    buildTuneConstantSearchTokens,
    filterTuneConstantGroups
  } from '$lib/features/pipelines/page/pipelineTuneDerived';
  import type { StreamPipelineLayout, StreamPipelineWire } from '$lib/api/client';

  type Props = {
    open: boolean;
    pipelineId: string | null;
    position: { x: number; y: number };
    size: { width: number; height: number };
    rawPipelineId: string;
    rawPipelineUuid: string;
    pipelineLabel: (pipelineId: string | null) => string;
    canShowEngineConfig: boolean;
    engineConfigOpen: boolean;
    onSetEngineConfigOpen: (next: boolean) => void;
    onClose: () => void;
    onPositionChange: (next: { x: number; y: number }) => void;
    onSizeChange: (next: { width: number; height: number }) => void;
    loading: boolean;
    error: string | null;
    nodeDescriptors: PipelineUiNodeDescriptor[];
    ui: PipelineUi;
    streamLabel: string;
    streamId: string | null;
    streamLayout?: StreamPipelineLayout | null;
    streamWires?: StreamPipelineWire[];
    setFrameSourceForPipelineInstance?: ((args: {
      to: { pipelineId: string; outputKey?: string | null };
      from: { pipelineId: string; outputKey?: string | null; port?: string | null } | null;
    }) => Promise<void> | void) | null;
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors: Record<string, Record<string, string | null>>;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    applyBusy: boolean;
    enginePlan: unknown;
    onEnginePlanChange: (plan: unknown) => void;
  };

  let {
    open,
    pipelineId,
    position,
    size,
    rawPipelineId,
    rawPipelineUuid,
    pipelineLabel,
    canShowEngineConfig,
    engineConfigOpen,
    onSetEngineConfigOpen,
    onClose,
    onPositionChange,
    onSizeChange,
    loading,
    error,
    nodeDescriptors,
    ui,
    streamLabel,
    streamId,
    streamLayout = null,
    streamWires = [],
    setFrameSourceForPipelineInstance = null,
    streamNodeOverrides,
    streamNodeErrors,
    readNodeDraft,
    updateStreamNodeValue,
    applyBusy,
    enginePlan,
    onEnginePlanChange
  }: Props = $props();
  $effect(() => {
    void canShowEngineConfig;
    void engineConfigOpen;
    void onSetEngineConfigOpen;
    void enginePlan;
    void onEnginePlanChange;
  });

  const hasUiItems = $derived.by(() => {
    if (!ui) return false;
    if (ui.layout?.type === 'tabs') {
      return (ui.layout.tabs ?? []).some((tab) => (tab.content ?? []).length > 0);
    }
    if (ui.layout?.type === 'stack') {
      return (ui.layout.items ?? []).length > 0;
    }
    return (ui.groups ?? []).some((group) => (group.controls ?? []).length > 0);
  });

  let constantSearch = $state('');
  let uiSearch = $state('');
  const constantEntries = $derived.by<PipelineTuningConstantEntry[]>(() =>
    (nodeDescriptors ?? [])
      .map((desc) => ({
        nodeId: desc.nodeId,
        nodeLabel: desc.nodeLabel,
        portKey: desc.portKey,
        dataType: desc.dataType ?? null,
        baseValue: desc.defaultValue ?? null,
        overrideValue: desc.overrideValue ?? null,
        metadata: desc.metadata ?? null
      }))
      .filter((entry) => Boolean(entry.nodeId))
  );
  const constantGroups = $derived.by(() => buildTuneConstantGroups(constantEntries));
  const constantSearchTokens = $derived.by(() => buildTuneConstantSearchTokens(constantSearch));
  const filteredConstantGroups = $derived.by(() =>
    filterTuneConstantGroups(constantGroups, constantSearchTokens)
  );
  const normalizeId = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');
  const resolvePipelineLabel = (value: string | null): string => {
    const normalized = normalizeId(value);
    if (typeof pipelineLabel === 'function') {
      const resolved = pipelineLabel(normalized || null);
      if (typeof resolved === 'string' && resolved.trim().length) {
        return resolved.trim();
      }
    }
    if (normalized === rawPipelineId) return 'Raw stream';
    return normalized.length ? normalized : 'Pipeline';
  };
  const normalizeKey = (value: unknown): string | null => {
    const normalized = normalizeId(value);
    return normalized.length ? normalized : null;
  };
  const normalizePort = (value: unknown, fallback = 'frame'): string => {
    const normalized = normalizeId(value);
    return normalized.length ? normalized : fallback;
  };
  const normalizedLayoutSlots = $derived.by(() => {
    const slots = Array.isArray(streamLayout?.slots) ? streamLayout.slots : [];
    return slots
      .map((slot) => {
        const row = Math.trunc(Number(slot.row));
        const column = Math.trunc(Number(slot.column));
        if (!Number.isInteger(row) || !Number.isInteger(column)) return null;
        const rawId = normalizeId(slot.pipeline_id);
        if (!rawId.length) return null;
        const pipelineId = rawId === rawPipelineUuid ? rawPipelineId : rawId;
        const outputKey = normalizeKey(slot.output_key);
        return { row, column, pipelineId, outputKey, resolvedPort: outputKey ?? 'frame' };
      })
      .filter(Boolean) as Array<{ row: number; column: number; pipelineId: string; outputKey: string | null; resolvedPort: string }>;
  });
  const showInputSelector = $derived.by(() =>
    Boolean(pipelineId && pipelineId !== rawPipelineId && setFrameSourceForPipelineInstance)
  );
  const inputTargetOutputKey = $derived.by(() => {
    if (!pipelineId) return null;
    const match = normalizedLayoutSlots.find((slot) => slot.pipelineId === pipelineId) ?? null;
    return match?.outputKey ?? null;
  });
  const inputCurrentSelection = $derived.by(() => {
    if (!pipelineId) return 'raw|raw';
    const targetWireId = pipelineId === rawPipelineId ? rawPipelineUuid : pipelineId;
    const wires = Array.isArray(streamWires) ? streamWires : [];
    const frameWire = wires.find((wire) => {
      const to = wire.to ?? null;
      const wireToId = normalizeId(to?.pipeline_id);
      if (!wireToId.length) return false;
      if (wireToId !== targetWireId) return false;
      const wireToPort = normalizePort(to?.port, 'frame').toLowerCase();
      if (wireToPort !== 'frame') return false;
      const wireToKey = normalizeKey(to?.output_key);
      return (wireToKey ?? null) === (inputTargetOutputKey ?? null);
    });
    const from = frameWire?.from ?? null;
    const fromIdRaw = normalizeId(from?.pipeline_id);
    if (!fromIdRaw.length) return 'raw|raw';
    const fromId = fromIdRaw === rawPipelineUuid ? rawPipelineId : fromIdRaw;
    if (fromId === rawPipelineId) {
      const selected = normalizeId(from?.port) || normalizeId(from?.output_key) || 'raw';
      const canonical = selected.toLowerCase() === 'frame' ? 'raw' : selected;
      return `raw|${canonical}`;
    }
    const fromKey = normalizeKey(from?.output_key) ?? '';
    const fromPort = normalizePort(from?.port, 'frame');
    return `pipe|${fromId}|${fromKey}|${fromPort}`;
  });

  const applyInputSelection = (selection: string): void => {
    if (!pipelineId || !setFrameSourceForPipelineInstance) return;
    const trimmed = selection.trim();
    if (!trimmed) return;
    if (trimmed.startsWith('raw|')) {
      const parts = trimmed.split('|');
      const port = (parts[1] ?? 'raw').trim() || 'raw';
      void setFrameSourceForPipelineInstance({
        to: { pipelineId, outputKey: inputTargetOutputKey },
        from: { pipelineId: rawPipelineId, outputKey: null, port }
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
        to: { pipelineId, outputKey: inputTargetOutputKey },
        from: { pipelineId: fromId, outputKey: fromKey, port }
      });
    }
  };

  type DragState = { startX: number; startY: number; originX: number; originY: number };
  type ResizeState = { startX: number; startY: number; originW: number; originH: number };
  let dragState = $state(null as DragState | null);
  let resizeState = $state(null as ResizeState | null);

  const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);

  const shouldIgnoreDragTarget = (target: HTMLElement | null) => {
    if (!target) return false;
    return Boolean(target.closest('button, input, select, textarea, a'));
  };

  function handleDragStart(event: PointerEvent | MouseEvent) {
    if (resizeState) return;
    const target = event.target as HTMLElement | null;
    if (shouldIgnoreDragTarget(target)) return;
    dragState = {
      startX: event.clientX,
      startY: event.clientY,
      originX: position.x,
      originY: position.y
    };
  }

  function handleResizeStart(event: PointerEvent) {
    if (dragState) return;
    event.preventDefault();
    event.stopPropagation();
    resizeState = {
      startX: event.clientX,
      startY: event.clientY,
      originW: size.width,
      originH: size.height
    };
  }

  function handlePointerMove(event: PointerEvent | MouseEvent) {
    if (typeof window === 'undefined') return;
    if (!open || !pipelineId) return;
    if (dragState) {
      const dx = event.clientX - dragState.startX;
      const dy = event.clientY - dragState.startY;
      const maxX = Math.max(0, window.innerWidth - size.width - 12);
      const maxY = Math.max(0, window.innerHeight - size.height - 12);
      onPositionChange({
        x: clamp(dragState.originX + dx, 8, maxX),
        y: clamp(dragState.originY + dy, 8, maxY)
      });
      return;
    }
    if (resizeState) {
      const dx = event.clientX - resizeState.startX;
      const dy = event.clientY - resizeState.startY;
      const minW = 460;
      const minH = 420;
      const maxW = Math.max(minW, window.innerWidth - position.x - 12);
      const maxH = Math.max(minH, window.innerHeight - position.y - 12);
      onSizeChange({
        width: clamp(resizeState.originW + dx, minW, maxW),
        height: clamp(resizeState.originH + dy, minH, maxH)
      });
    }
  }

  function handlePointerUp() {
    dragState = null;
    resizeState = null;
  }

  $effect(() => {
    if (!open || !pipelineId) {
      dragState = null;
      resizeState = null;
    }
  });
</script>

<svelte:window
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  onpointercancel={handlePointerUp}
  onmousemove={handlePointerMove}
  onmouseup={handlePointerUp}
/>

{#if open && pipelineId}
  <div
    class="fixed z-50 overflow-hidden rounded-xl border border-surface-800/70 bg-surface-950/90 shadow-2xl shadow-black/40 backdrop-blur"
    style={`left:${position.x}px; top:${position.y}px; width:${size.width}px; height:${size.height}px;`}
    data-testid="pipeline-tuning-panel"
  >
    <div
      class="flex cursor-move items-center justify-between border-b border-surface-800/70 px-3 py-2 select-none touch-none"
      onpointerdown={handleDragStart}
      onmousedown={handleDragStart}
      role="presentation"
      data-testid="pipeline-tuning-header"
    >
      <div class="min-w-0 flex-1 flex items-center gap-2">
        {#if pipelineId === rawPipelineId}
          <span class="flex h-8 w-8 items-center justify-center rounded-full border border-white/10 bg-black/40 text-surface-100">
            <FaIcon icon={faCamera} class="h-4 w-4" />
          </span>
        {:else}
          <PipelineIcon
            pipelineId={pipelineId}
            size="sm"
            className="shrink-0"
            ariaLabel={resolvePipelineLabel(pipelineId)}
          />
        {/if}
        <div class="min-w-0">
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Pipeline Tuning</p>
          <p class="truncate text-sm text-surface-100">{resolvePipelineLabel(pipelineId)}</p>
        </div>
      </div>
      <div class="flex items-center gap-2">
        <button
          class="btn btn-2xs preset-outline"
          type="button"
          onpointerdown={(event) => event.stopPropagation()}
          onclick={(event) => {
            event.stopPropagation();
            onClose();
          }}
        >
          Close
        </button>
      </div>
    </div>
    <div class="flex h-full flex-col gap-3 p-3 min-h-0">
      <div class="flex-1 min-h-0 overflow-y-auto space-y-3 pb-6">
        {#if loading}
          <div class="rounded border border-primary-500/40 bg-primary-500/10 px-3 py-2 text-xs text-primary-100">
            Loading pipeline graph…
          </div>
        {:else if error}
          <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-100">
            {error}
          </div>
        {:else if !hasUiItems}
          <PipelineStreamOverridesPanel
            streamLabel={streamLabel}
            streamId={streamId}
            constantSearch={constantSearch}
            constantGroups={constantGroups}
            filteredConstantGroups={filteredConstantGroups}
            streamNodeOverrides={streamNodeOverrides}
            streamNodeErrors={streamNodeErrors}
            streamError={null}
            readNodeDraft={readNodeDraft}
            updateStreamNodeValue={updateStreamNodeValue}
            onSearch={(value) => (constantSearch = value)}
          />
        {:else}
          {#if showInputSelector}
            <div>
              <span class="block text-micro-tight uppercase tracking-[0.3em] text-surface-500">Input</span>
              <select
                class="mt-1 w-full rounded border border-surface-800/70 bg-surface-900/60 px-3 py-2 text-xs text-surface-200"
                value={inputCurrentSelection}
                onchange={(event) => applyInputSelection(event.currentTarget.value ?? '')}
              >
                <option value="raw|raw">Raw stream: raw</option>
                <option value="raw|undistorted">Raw stream: undistorted</option>
                {#each normalizedLayoutSlots as source (`${source.row}:${source.column}`)}
                  {#if source.pipelineId !== pipelineId && source.pipelineId !== rawPipelineId}
                    <option value={`pipe|${source.pipelineId}|${source.outputKey ?? ''}|${source.resolvedPort}`}>
                      {resolvePipelineLabel(source.pipelineId)} ({source.row + 1}:{source.column + 1}) - {source.resolvedPort}
                    </option>
                  {/if}
                {/each}
              </select>
            </div>
          {/if}
          <input
            class="w-full rounded border border-surface-800/70 bg-surface-900/60 px-3 py-2 text-xs text-surface-200"
            type="search"
            placeholder="Search controls…"
            value={uiSearch}
            oninput={(event) => (uiSearch = event.currentTarget.value)}
          />
          <PipelineUiOverridesPanel
            ui={ui}
            streamLabel={streamLabel}
            streamId={streamId}
            pipelineId={pipelineId}
            rawPipelineId={rawPipelineId}
            rawPipelineUuid={rawPipelineUuid}
            streamLayout={streamLayout}
            nodeDescriptors={nodeDescriptors}
            streamNodeOverrides={streamNodeOverrides}
            streamNodeErrors={streamNodeErrors}
            streamError={error}
            readNodeDraft={readNodeDraft}
            updateStreamNodeValue={updateStreamNodeValue}
            searchQuery={uiSearch}
          />
        {/if}
      </div>

      <div class="flex items-center justify-between gap-3 text-micro-tight text-surface-500">
        <span>{applyBusy ? 'Applying updates…' : 'Overrides apply live.'}</span>
      </div>
    </div>
    <div
      class="absolute bottom-1 right-1 h-4 w-4 cursor-se-resize rounded bg-surface-800/60"
      onpointerdown={handleResizeStart}
      title="Resize"
      role="presentation"
    ></div>
  </div>
{/if}
