<script lang="ts">
  import CameraPipelineOverrides from './CameraPipelineOverrides.svelte';
  import { extractNodeValueDescriptors, type PipelineNodeValueDescriptor } from './cameraPipelineTuningController';
  import { getDataTypeVariants, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
  import { resolveStreamLabel } from '$lib/utils/streamLabels';
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
  import type { StreamInfo, StreamPipelineEndpoint, StreamPipelineWire } from '$lib/ts-bindings/http/client';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';

  type PipelineTuningPanelState = {
    pipelineTuningPanelOpen: boolean;
    pipelineTuningPipelineId: string | null;
    pipelineTuningUiOverride: PipelineUi;
    pipelineTuningGraphOverride: unknown;
    pipelineTuningPosition: { x: number; y: number };
    pipelineTuningSize: { width: number; height: number };
    pipelineNodeOverridesById?: Record<string, Record<string, PipelineNodeValue>>;
  };

  type PipelineTuningContext = {
    pipelineState: PipelineTuningPanelState;
    pipelineTuningPanelOpen: boolean;
    pipelineTuningPipelineId: string | null;
    pipelineTuningUi: PipelineUi;
    pipelineTuningNodeDescriptors?: PipelineNodeValueDescriptor[];
    pipelineTuningGraph?: unknown;
    pipelineTuningEffectiveNodeOverrides?: Record<string, Record<string, PipelineNodeValue>>;
    ensurePipelineRegistry?: () => Promise<unknown>;
    pipelineRegistrySnapshot?: unknown;
    RAW_PIPELINE_ID: string;
    RAW_PIPELINE_UUID: string;
    pipelineLabel: (pipelineId: string | null) => string;
    canShowTuningEngineConfig: boolean;
    pipelineTuningEngineConfigOpen: boolean;
    closePipelineTuningPanel: () => void;
    pipelineTuningLoading: boolean;
    pipelineTuningError: string | null;
    stream?: StreamInfo | null;
    streamId?: string | null;
    setFrameSourceForPipelineInstance?: ((args: {
      to: { pipelineId: string; outputKey?: string | null };
      from: { pipelineId: string; outputKey?: string | null; port?: string | null } | null;
    }) => Promise<void> | void) | null;
    pipelineNodeErrorsById?: Record<string, Record<string, Record<string, string | null>>>;
    readPipelineNodeDraft: (pipelineId: string, nodeId: string, portKey: string) => string | null;
    updatePipelineNodeValue: (
      pipelineId: string,
      nodeId: string,
      portKey: string,
      dataType: PipelineDataType | null,
      raw: string
    ) => void;
    pipelineTuningApplyBusy: boolean;
    pipelineTuningPlan: unknown;
    serializeGraphPlan: (plan: unknown) => unknown;
    pipelineGraphCache: Record<string, unknown>;
  };

  const { ctx } = $props<{ ctx: PipelineTuningContext }>();
  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;
  const asPipelineDataType = (value: unknown): PipelineDataType | undefined =>
    typeof value === 'string' || (value && typeof value === 'object')
      ? (value as PipelineDataType)
      : undefined;
  const isPipelineEndpoint = (value: unknown): value is StreamPipelineEndpoint => {
    const record = asRecord(value);
    return typeof record?.pipeline_id === 'string';
  };
  const isPipelineWire = (value: unknown): value is StreamPipelineWire => {
    const record = asRecord(value);
    return isPipelineEndpoint(record?.from) && isPipelineEndpoint(record?.to);
  };
  const panelOpen = $derived.by(() => ctx.pipelineState?.pipelineTuningPanelOpen ?? ctx.pipelineTuningPanelOpen);
  const panelPipelineId = $derived.by(() => ctx.pipelineState?.pipelineTuningPipelineId ?? ctx.pipelineTuningPipelineId);
  const panelUi = $derived.by(() => ctx.pipelineState?.pipelineTuningUiOverride ?? ctx.pipelineTuningUi);
  const panelNodeOverrides = $derived.by(() => {
    if (!panelPipelineId) return {};
    return (
      ctx.pipelineState?.pipelineNodeOverridesById?.[panelPipelineId] ??
      ctx.pipelineNodeOverridesById?.[panelPipelineId] ??
      {}
    );
  });
  let localRegistrySnapshot = $state<unknown | null>(null);
  $effect(() => {
    if (localRegistrySnapshot || !ctx?.ensurePipelineRegistry || !panelOpen) return;
    let cancelled = false;
    ctx.ensurePipelineRegistry()
      .then((snapshot: unknown) => {
        if (!cancelled) {
          localRegistrySnapshot = snapshot ?? null;
        }
      })
      .catch(() => {
        if (!cancelled) {
          localRegistrySnapshot = null;
        }
      });
    return () => {
      cancelled = true;
    };
  });
  const resolvedRegistrySnapshot = $derived.by(() => ctx.pipelineRegistrySnapshot ?? localRegistrySnapshot);
  const panelNodeDescriptors = $derived.by(() => {
    const base = ctx.pipelineTuningNodeDescriptors ?? [];
    const graph = ctx.pipelineState?.pipelineTuningGraphOverride ?? ctx.pipelineTuningGraph ?? null;
    if (!graph) return base;
    const fallback = extractNodeValueDescriptors(
      graph,
      ctx.pipelineTuningEffectiveNodeOverrides ?? {},
      resolvedRegistrySnapshot
    );
    if (!base.length) return fallback;
    if (!fallback.length) return base;

    const normalizePortKey = (value: string) => value.trim().toLowerCase();
    const isGenericType = (dataType: unknown): boolean => {
      const key = (resolveDataTypeKey(asPipelineDataType(dataType)) ?? '').toLowerCase();
      return !key || ['generic', 'any', 'unknown', 'dynamic'].includes(key);
    };

    const fallbackMap = new SvelteMap<string, PipelineNodeValueDescriptor>();
    for (const entry of fallback) {
      const nodeId = String(entry?.nodeId ?? '');
      const portKey = normalizePortKey(String(entry?.portKey ?? ''));
      if (!nodeId || !portKey) continue;
      fallbackMap.set(`${nodeId}:${portKey}`, entry);
    }

    const merged = base.map((entry) => {
      const nodeId = String(entry?.nodeId ?? '');
      const portKey = normalizePortKey(String(entry?.portKey ?? ''));
      if (!nodeId || !portKey) return entry;
      const fallbackEntry = fallbackMap.get(`${nodeId}:${portKey}`) ?? null;
      if (!fallbackEntry) return entry;
      const baseType = entry?.dataType ?? null;
      const fallbackType = fallbackEntry?.dataType ?? null;
      const baseVariants = getDataTypeVariants(baseType ?? undefined);
      const fallbackVariants = getDataTypeVariants(fallbackType ?? undefined);
      const shouldUseFallbackType =
        !baseType ||
        isGenericType(baseType) ||
        (baseVariants.length === 0 && fallbackVariants.length > 0);

      const baseAllowed = entry?.metadata?.allowedValues ?? [];
      const fallbackAllowed = fallbackEntry?.metadata?.allowedValues ?? [];
      const shouldUseFallbackMeta =
        (!entry?.metadata && fallbackEntry?.metadata) ||
        (Array.isArray(baseAllowed) && baseAllowed.length === 0 && Array.isArray(fallbackAllowed) && fallbackAllowed.length > 0);

      return {
        ...entry,
        dataType: shouldUseFallbackType ? fallbackType : baseType,
        metadata: shouldUseFallbackMeta ? fallbackEntry?.metadata ?? null : entry?.metadata ?? null,
        defaultValue: entry?.defaultValue ?? fallbackEntry?.defaultValue ?? null,
        overrideValue: entry?.overrideValue ?? fallbackEntry?.overrideValue ?? null
      };
    });

    const mergedKeys = new SvelteSet(merged.map((entry) => {
      const nodeId = String(entry?.nodeId ?? '');
      const portKey = normalizePortKey(String(entry?.portKey ?? ''));
      return nodeId && portKey ? `${nodeId}:${portKey}` : '';
    }));

    for (const entry of fallback) {
      const nodeId = String(entry?.nodeId ?? '');
      const portKey = normalizePortKey(String(entry?.portKey ?? ''));
      if (!nodeId || !portKey) continue;
      const key = `${nodeId}:${portKey}`;
      if (!mergedKeys.has(key)) {
        merged.push(entry);
      }
    }

    return merged;
  });
  const streamWires = $derived.by<StreamPipelineWire[]>(() => {
    const manifest = ctx.stream?.manifest ?? null;
    const manifestRecord = asRecord(manifest);
    if (Array.isArray(manifest?.pipeline_wires)) {
      return manifest.pipeline_wires;
    }
    const legacyWires = manifestRecord?.pipelineWires;
    return Array.isArray(legacyWires) ? legacyWires.filter(isPipelineWire) : [];
  });
</script>

<CameraPipelineOverrides
  open={panelOpen}
  pipelineId={panelPipelineId}
  position={ctx.pipelineState.pipelineTuningPosition}
  size={ctx.pipelineState.pipelineTuningSize}
  rawPipelineId={ctx.RAW_PIPELINE_ID}
  rawPipelineUuid={ctx.RAW_PIPELINE_UUID}
  pipelineLabel={ctx.pipelineLabel}
  canShowEngineConfig={ctx.canShowTuningEngineConfig}
  engineConfigOpen={ctx.pipelineTuningEngineConfigOpen}
  onSetEngineConfigOpen={(next) => (ctx.pipelineTuningEngineConfigOpen = next)}
  onClose={ctx.closePipelineTuningPanel}
  onPositionChange={(next) => {
    ctx.pipelineState.pipelineTuningPosition = next;
  }}
  onSizeChange={(next) => {
    ctx.pipelineState.pipelineTuningSize = next;
  }}
  loading={ctx.pipelineTuningLoading}
  error={ctx.pipelineTuningError}
  nodeDescriptors={panelNodeDescriptors}
  ui={panelUi}
  streamLabel={resolveStreamLabel(ctx.stream ?? { id: ctx.streamId }, ctx.streamId ?? 'Stream')}
  streamId={ctx.stream?.id ?? ctx.streamId}
  streamLayout={ctx.stream?.manifest?.pipeline_layout ?? null}
  {streamWires}
  setFrameSourceForPipelineInstance={ctx.setFrameSourceForPipelineInstance}
  streamNodeOverrides={panelNodeOverrides}
  streamNodeErrors={panelPipelineId ? ctx.pipelineNodeErrorsById?.[panelPipelineId] ?? {} : {}}
  readNodeDraft={(nodeId, portKey) =>
    panelPipelineId ? ctx.readPipelineNodeDraft(panelPipelineId, nodeId, portKey) : null
  }
  updateStreamNodeValue={(nodeId, portKey, dataType, raw) => {
    if (!panelPipelineId) return;
    ctx.updatePipelineNodeValue(panelPipelineId, nodeId, portKey, dataType, raw);
  }}
  applyBusy={ctx.pipelineTuningApplyBusy}
  enginePlan={ctx.pipelineTuningPlan}
  onEnginePlanChange={(plan) => {
    if (!ctx.pipelineTuningPipelineId) return;
    const graph = ctx.serializeGraphPlan(plan);
    ctx.pipelineGraphCache = { ...ctx.pipelineGraphCache, [ctx.pipelineTuningPipelineId]: graph };
  }}
/>
