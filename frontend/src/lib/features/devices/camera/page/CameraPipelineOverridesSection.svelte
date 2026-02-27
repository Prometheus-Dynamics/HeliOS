<script lang="ts">
  import CameraPipelineOverrides from './CameraPipelineOverrides.svelte';
  import { extractNodeValueDescriptors } from './cameraPipelineTuningController';
  import { getDataTypeVariants, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
  import { resolveStreamLabel } from '$lib/utils/streamLabels';

  const { ctx } = $props<{ ctx: any }>();
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
  let localRegistrySnapshot = $state<any | null>(null);
  $effect(() => {
    if (localRegistrySnapshot || !ctx?.ensurePipelineRegistry) return;
    let cancelled = false;
    ctx.ensurePipelineRegistry()
      .then((snapshot: any) => {
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
      const key = (resolveDataTypeKey(dataType as any) ?? '').toLowerCase();
      return !key || ['generic', 'any', 'unknown', 'dynamic'].includes(key);
    };

    const fallbackMap = new Map<string, any>();
    for (const entry of fallback) {
      const nodeId = String(entry?.nodeId ?? '');
      const portKey = normalizePortKey(String(entry?.portKey ?? ''));
      if (!nodeId || !portKey) continue;
      fallbackMap.set(`${nodeId}:${portKey}`, entry);
    }

    const merged = base.map((entry: any) => {
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

    const mergedKeys = new Set(merged.map((entry: any) => {
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
  streamWires={((ctx.stream?.manifest as any)?.pipeline_wires ?? (ctx.stream?.manifest as any)?.pipelineWires ?? [])}
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
