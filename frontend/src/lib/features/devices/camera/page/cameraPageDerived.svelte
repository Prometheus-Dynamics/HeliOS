<script lang="ts" module>
  import type { PipelineDataType, PipelineGraphPlan, PipelineNodeValue, PipelinePortMetadata } from '$lib/types/pipeline';
  import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
  import type { ResourceSample } from '$lib/api/telemetry';
  import type { DaedalusRegistryResponse } from '$lib/api/client';
  import { resourceTelemetryStore } from '$lib/api/telemetry';
  import { get } from 'svelte/store';
  import { fromApiGraphPlan } from '$lib/features/pipelines/graphConverters';
  import { createRegistryResolver } from '$lib/components/flow/pipeline-graph/registry';
  import { getDataTypeVariants, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
  import { normalizeDaedalusRegistry } from '$lib/features/pipelines/controller/daedalusRegistry/normalization';
  import { hydrateGraphWithRegistry } from '$lib/features/pipelines/styleHydration';
  import { parseMetadataValue } from './cameraStateUtils';
  import {
    extractNodeOverrides,
    extractNodeValueDescriptors,
    mergeNodeOverrides,
    registryPortMetadataFor,
    registryPortTypeFor,
    type PipelineNodeValueDescriptor
  } from './cameraPipelineTuningController';

  type PipelineGraphListEntry = Record<string, unknown> & {
    id?: string;
    name?: string | null;
  };

  type DerivedState = {
    get selectedPipelineId(): string | null;
    get assignedPipelineIds(): string[];
    get pipelineGridSlots(): Record<string, string | null>;
    get manifestState(): unknown;
    get pipelineGridRows(): number;
    get pipelineGridColumns(): number;
    get pipelineAssignQuery(): string;
    get pipelineGraphs(): PipelineGraphListEntry[];
    get pipelineTuningPipelineId(): string | null;
    get pipelineGraphCache(): Record<string, unknown>;
    get pipelineTuningLiveGraph(): unknown;
    get pipelineTuningUiOverride(): PipelineUi | null;
    get pipelineTuningGraphOverride(): unknown;
    get pipelineNodeOverridesById(): Record<string, Record<string, Record<string, PipelineNodeValue>>>;
    get pipelineRegistrySnapshot(): DaedalusRegistryResponse | null;
    get PIPELINE_UI_METADATA_KEY(): string;
    get DEFAULT_PIPELINE_UI(): PipelineUi;
    get RAW_PIPELINE_ID(): string;
    get RAW_PIPELINE_UUID(): string;
    get RAW_LOOPBACK_GRAPH(): unknown;
  };

  export function createCameraPageDerived(state: DerivedState) {
    const normalizePortKey = (value: string) => value.trim().toLowerCase();

    const valueForPort = (
      values: Record<string, PipelineNodeValue> | null | undefined,
      portKey: string
    ): PipelineNodeValue | null => {
      if (!values) return null;
      const normalized = normalizePortKey(portKey);
      const exact = values[portKey];
      if (exact) return exact;
      if (normalized && values[normalized]) return values[normalized] ?? null;
      const fallbackKey = Object.keys(values).find((key) => normalizePortKey(key) === normalized);
      return fallbackKey ? values[fallbackKey] ?? null : null;
    };

    const mergeMetadata = (
      base: PipelinePortMetadata | null,
      live: PipelinePortMetadata | null
    ): PipelinePortMetadata | null => {
      if (!base && !live) return null;
      if (!base) return live ?? null;
      if (!live) return base ?? null;
      const liveAllowed = Array.isArray(live.allowedValues) && live.allowedValues.length > 0 ? live.allowedValues : null;
      const baseAllowed = Array.isArray(base.allowedValues) && base.allowedValues.length > 0 ? base.allowedValues : null;
      return {
        ...base,
        ...live,
        ...(liveAllowed || baseAllowed ? { allowedValues: liveAllowed ?? baseAllowed ?? undefined } : {})
      };
    };

    const mergeDataType = (base: PipelineDataType | null, live: PipelineDataType | null): PipelineDataType | null => {
      if (!base) return live ?? null;
      if (!live) return base ?? null;
      const baseVariants = getDataTypeVariants(base ?? undefined);
      const liveVariants = getDataTypeVariants(live ?? undefined);
      if (liveVariants.length > 0) {
        if (baseVariants.length === 0) return live;
        if (baseVariants.join('|') !== liveVariants.join('|')) return live;
      }
      const baseKey = (resolveDataTypeKey(base ?? undefined) ?? '').toLowerCase();
      const liveKey = (resolveDataTypeKey(live ?? undefined) ?? '').toLowerCase();
      const genericKeys = new Set(['generic', 'any', 'unknown', 'dynamic']);
      if (genericKeys.has(baseKey) && liveKey && !genericKeys.has(liveKey)) return live;
      return base;
    };

    const resolveRegistrySnapshotNodeId = (
      snapshot: DaedalusRegistryResponse | null | undefined,
      backendId: string | null | undefined
    ): string | null => {
      if (!snapshot || !backendId) return null;
      const nodes = Array.isArray(snapshot?.nodes) ? snapshot.nodes : [];
      if (!nodes.length) return null;
      const normalize = (value: string) => {
        const lower = value.toLowerCase();
        const atIndex = lower.indexOf('@');
        return atIndex >= 0 ? lower.slice(0, atIndex) : lower;
      };
      const normalized = normalize(String(backendId));
      let best: { id: string; score: number } | null = null;
      for (const node of nodes) {
        const rawId = typeof node?.id === 'string' ? node.id : String(node?.id ?? '');
        if (!rawId) continue;
        const idLower = rawId.toLowerCase();
        if (idLower === normalized) return rawId;
        const normalizedId = normalize(rawId);
        if (normalizedId === normalized) return rawId;
        let score = 0;
        if (normalized && idLower.includes(normalized)) {
          score = normalized.length / Math.max(idLower.length, 1);
        } else if (normalized && normalized.includes(idLower)) {
          score = idLower.length;
        } else {
          continue;
        }
        if (!best || score > best.score) {
          best = { id: rawId, score };
        }
      }
      return best?.id ?? null;
    };

    const coercePlanFromGraph = (graph: unknown): PipelineGraphPlan | null => {
      if (!graph || typeof graph !== 'object') return null;
      const nodes = (graph as { nodes?: unknown }).nodes;
      const connections = (graph as { connections?: unknown }).connections;
      if (nodes && typeof nodes === 'object' && !Array.isArray(nodes) && Array.isArray(connections)) {
        return graph as PipelineGraphPlan;
      }
      try {
        const plan = fromApiGraphPlan(graph);
        return Object.keys(plan?.nodes ?? {}).length ? plan : null;
      } catch {
        return null;
      }
    };

    const buildDescriptorMap = (
      plan: PipelineGraphPlan | null,
      resolveRegistryEntryForNode: ReturnType<typeof createRegistryResolver> | null
    ): Map<string, PipelineNodeValueDescriptor> => {
      const map = new Map<string, PipelineNodeValueDescriptor>();
      if (!plan) return map;
      const overrides = extractNodeOverrides(plan);
      for (const [nodeId, node] of Object.entries(plan.nodes ?? {})) {
        if (!node) continue;
        const nodeLabel = node.metadata?.name ?? nodeId;
        const inputs = node.inputs ?? {};
        const portMeta = node.metadata?.inputPorts ?? {};
        const registryEntry = resolveRegistryEntryForNode ? resolveRegistryEntryForNode(node) : null;
        const registryInputs = registryEntry?.inputs ?? {};
        const registryPortMeta = registryEntry?.metadata?.inputPorts ?? {};
        const source = node.source as {
          id?: string;
          info?: { values?: Record<string, PipelineNodeValue> };
          values?: Record<string, PipelineNodeValue>;
        } | null;
        const sourceId =
          typeof source?.id === 'string'
            ? source.id
            : typeof node.info?.id === 'string'
              ? node.info.id
              : null;
        const infoValues = node.info?.values ?? null;
        const sourceValues = source?.info?.values ?? source?.values ?? null;
        const values =
          (infoValues && Object.keys(infoValues).length ? infoValues : sourceValues) ??
          {};
        const portKeys = new Set<string>([
          ...Object.keys(inputs),
          ...Object.keys(registryInputs),
          ...Object.keys(portMeta),
          ...Object.keys(registryPortMeta),
          ...Object.keys(values)
        ]);
        const lookupId = sourceId ?? node.backendId ?? nodeId;
        const snapshotNodeId = resolveRegistrySnapshotNodeId(state.pipelineRegistrySnapshot, lookupId);
        for (const portKey of portKeys) {
          const normalized = normalizePortKey(portKey);
          if (!normalized) continue;
          const dataType = (inputs as Record<string, PipelineDataType | undefined>)[portKey] ?? null;
          const registryType = (registryInputs as Record<string, PipelineDataType | undefined>)[portKey] ?? null;
          const nodeMeta =
            portMeta[portKey] ??
            portMeta[portKey.toLowerCase()] ??
            (normalized !== portKey ? portMeta[normalized] ?? portMeta[normalized.toLowerCase()] : null) ??
            null;
          const registryMeta =
            registryPortMeta[portKey] ??
            registryPortMeta[portKey.toLowerCase()] ??
            (normalized !== portKey
              ? registryPortMeta[normalized] ?? registryPortMeta[normalized.toLowerCase()]
              : null) ??
            null;
          const snapshotMeta =
            snapshotNodeId ? registryPortMetadataFor(state.pipelineRegistrySnapshot, snapshotNodeId, portKey) : null;
          const snapshotType =
            snapshotNodeId ? registryPortTypeFor(state.pipelineRegistrySnapshot, snapshotNodeId, portKey) : null;
          const baseValue = valueForPort(values ?? null, portKey);
          const valueType = baseValue?.dataType ?? null;
          const overrideRecord = overrides?.[nodeId] ?? {};
          const overrideValue = overrideRecord?.[normalized] ?? null;
          const resolvedType = mergeDataType(
            mergeDataType(mergeDataType(dataType, registryType), snapshotType),
            valueType
          );
          map.set(`${nodeId}:${normalized}`, {
            nodeId,
            backendId: node.backendId ?? null,
            sourceId,
            nodeLabel: nodeLabel ?? nodeId,
            portKey,
            dataType: resolvedType,
            defaultValue: baseValue ?? null,
            overrideValue: overrideValue ?? null,
            metadata: mergeMetadata(
              mergeMetadata((nodeMeta ?? null) as PipelinePortMetadata | null, registryMeta ?? null),
              snapshotMeta
            )
          });
        }
      }
      return map;
    };

    const parseMaybeNestedJson = (value: string): unknown => {
      const parsed = parseMetadataValue(value);
      if (typeof parsed !== 'string') return parsed;
      const trimmed = parsed.trim();
      if (!trimmed) return parsed;
      if ((trimmed.startsWith('{') && trimmed.endsWith('}')) || (trimmed.startsWith('[') && trimmed.endsWith(']'))) {
        return parseMetadataValue(trimmed);
      }
      return parsed;
    };

    const decodeMetadataValue = (raw: unknown): unknown => {
      if (raw === null || raw === undefined) return null;
      if (typeof raw === 'string') {
        return parseMaybeNestedJson(raw);
      }
      if (raw && typeof raw === 'object' && 'value' in (raw as Record<string, unknown>)) {
        const value = (raw as { value?: unknown }).value;
        if (typeof value === 'string') {
          return parseMaybeNestedJson(value);
        }
        return value ?? null;
      }
      return raw;
    };

    const telemetrySample = $derived.by<ResourceSample>(() => get(resourceTelemetryStore) as ResourceSample);

    const activePipelineIds = $derived.by(() => {
      const seen = new Set<string>();
      const primary: string[] = [];
      const fallback: string[] = [];
      const normalizeId = (value: unknown): string | null => {
        const raw = typeof value === 'string' ? value.trim() : '';
        if (!raw) return null;
        const normalized = raw === state.RAW_PIPELINE_UUID ? state.RAW_PIPELINE_ID : raw;
        if (!normalized || normalized === state.RAW_PIPELINE_ID) return null;
        return normalized;
      };
      const push = (target: string[], value: unknown) => {
        const normalized = normalizeId(value);
        if (!normalized || seen.has(normalized)) return;
        seen.add(normalized);
        target.push(normalized);
      };

      Object.values(state.pipelineGridSlots ?? {}).forEach((id) => push(primary, id));

      const manifest = state.manifestState;
      if (manifest && typeof manifest === 'object') {
        const record = manifest as {
          active_pipeline_id?: unknown;
          pipeline_id?: unknown;
          pipelines?: unknown;
        };
        push(primary, record.active_pipeline_id);
        push(primary, record.pipeline_id);
        if (Array.isArray(record.pipelines)) {
          record.pipelines.forEach((entry) => {
            if (!entry || typeof entry !== 'object') return;
            const entryRecord = entry as {
              pipeline_id?: unknown;
              pipelineId?: unknown;
              id?: unknown;
            };
            push(primary, entryRecord.pipeline_id ?? entryRecord.pipelineId ?? entryRecord.id);
          });
        }
      }

      push(primary, state.selectedPipelineId);
      if (primary.length) return primary;

      for (const pipelineId of state.assignedPipelineIds ?? []) {
        push(fallback, pipelineId);
      }
      return fallback;
    });

    const pipelineGridRowIndices = $derived(
      (() => Array.from({ length: Math.min(Math.max(Math.trunc(state.pipelineGridRows), 1), 6) }, (_, i) => i))()
    );
    const pipelineGridColumnIndices = $derived(
      (() => Array.from({ length: Math.min(Math.max(Math.trunc(state.pipelineGridColumns), 1), 6) }, (_, i) => i))()
    );
    const pipelineGridIsSingle = $derived(
      (() => Math.trunc(state.pipelineGridRows) === 1 && Math.trunc(state.pipelineGridColumns) === 1)()
    );
    const pipelineGridIsMultiplex = $derived(
      (() => Math.trunc(state.pipelineGridRows) * Math.trunc(state.pipelineGridColumns) > 1)()
    );

    const pipelineAssignFilteredGraphs = $derived((() => {
      const q = state.pipelineAssignQuery.trim().toLowerCase();
      return (state.pipelineGraphs ?? [])
        .filter((g) => {
          const id = String(g?.id ?? '');
          const name = g?.name?.trim?.() ? String(g.name).trim() : '';
          if (!q.length) return true;
          return `${name} ${id}`.toLowerCase().includes(q);
        })
        .slice()
        .sort((a, b) => String(a?.name ?? a?.id ?? '').localeCompare(String(b?.name ?? b?.id ?? '')));
    })());

    const pipelineTuningGraph = $derived((() => {
      if (state.pipelineTuningGraphOverride) return state.pipelineTuningGraphOverride;
      if (!state.pipelineTuningPipelineId) return null;
      if (state.pipelineTuningPipelineId === state.RAW_PIPELINE_ID) return state.RAW_LOOPBACK_GRAPH;
      return state.pipelineGraphCache[state.pipelineTuningPipelineId] ?? null;
    })());

    const pipelineTuningUi = $derived.by<PipelineUi>(() => {
      if (state.pipelineTuningUiOverride) {
        return state.pipelineTuningUiOverride;
      }
      const graph = pipelineTuningGraph as
        | { metadata?: Record<string, unknown>; daedalus?: { metadata?: Record<string, unknown> } }
        | null;
      const metadata =
        graph?.metadata ??
        graph?.daedalus?.metadata ??
        (graph as { graph?: { metadata?: Record<string, unknown> } } | null)?.graph?.metadata ??
        (graph as { pipeline_graph?: { metadata?: Record<string, unknown> } } | null)?.pipeline_graph?.metadata ??
        null;
      const raw =
        metadata?.[state.PIPELINE_UI_METADATA_KEY] ??
        metadata?.['helios.pipeline_ui'] ??
        metadata?.['pipeline.ui'] ??
        null;
      const decoded = decodeMetadataValue(raw);
      return (decoded as PipelineUi | null) ?? state.DEFAULT_PIPELINE_UI;
    });

    const pipelineTuningPlan = $derived.by<PipelineGraphPlan | null>(() => {
      const graph = state.pipelineTuningGraphOverride ?? pipelineTuningGraph;
      if (!graph) return null;
      return coercePlanFromGraph(graph);
    });

    const canShowTuningEngineConfig = $derived.by(() => pipelineTuningPlan?.format === 'daedalus');

    const pipelineTuningBaseNodeOverrides = $derived.by<Record<string, Record<string, PipelineNodeValue>>>(() =>
      extractNodeOverrides(state.pipelineTuningGraphOverride ?? pipelineTuningGraph)
    );

    const pipelineTuningCameraNodeOverrides = $derived.by<Record<string, Record<string, PipelineNodeValue>>>(() => {
      if (!state.pipelineTuningPipelineId) return {};
      return state.pipelineNodeOverridesById[state.pipelineTuningPipelineId] ?? {};
    });

    const pipelineTuningEffectiveNodeOverrides = $derived.by<Record<string, Record<string, PipelineNodeValue>>>(() =>
      mergeNodeOverrides(pipelineTuningBaseNodeOverrides, pipelineTuningCameraNodeOverrides)
    );

    const pipelineTuningNodeDescriptors = $derived.by<PipelineNodeValueDescriptor[]>(() => {
      const tuningGraph = state.pipelineTuningGraphOverride ?? pipelineTuningGraph;
      const basePlan = coercePlanFromGraph(tuningGraph);
      const livePlan = coercePlanFromGraph(state.pipelineTuningLiveGraph ?? tuningGraph);
      const registryEntries =
        state.pipelineRegistrySnapshot && Array.isArray(state.pipelineRegistrySnapshot.nodes)
          ? normalizeDaedalusRegistry(
              state.pipelineRegistrySnapshot.nodes,
              state.pipelineRegistrySnapshot.types ?? undefined
            )
          : [];
      const resolveRegistryEntryForNode =
        registryEntries.length > 0 ? createRegistryResolver(registryEntries) : null;
      if (registryEntries.length > 0) {
        hydrateGraphWithRegistry(basePlan, registryEntries);
        hydrateGraphWithRegistry(livePlan, registryEntries);
      }
      const fallbackDescriptors = extractNodeValueDescriptors(
        tuningGraph,
        pipelineTuningEffectiveNodeOverrides,
        state.pipelineRegistrySnapshot
      );
      if (!basePlan) {
        return fallbackDescriptors;
      }
      const baseMap = buildDescriptorMap(basePlan, resolveRegistryEntryForNode);
      if (baseMap.size === 0) {
        return fallbackDescriptors;
      }
      const liveMap = buildDescriptorMap(livePlan, resolveRegistryEntryForNode);
      const output: PipelineNodeValueDescriptor[] = [];
      for (const [key, desc] of baseMap.entries()) {
        const live = liveMap.get(key) ?? null;
        output.push({
          ...desc,
          dataType: mergeDataType(desc.dataType ?? null, live?.dataType ?? null),
          metadata: mergeMetadata(desc.metadata ?? null, live?.metadata ?? null),
          defaultValue: desc.defaultValue ?? live?.defaultValue ?? null,
          overrideValue: desc.overrideValue ?? live?.overrideValue ?? null
        });
      }
      return output.sort((a, b) => {
        const nodeOrder = a.nodeLabel.localeCompare(b.nodeLabel);
        return nodeOrder !== 0 ? nodeOrder : a.portKey.localeCompare(b.portKey);
      });
    });

    return {
      get telemetrySample() {
        return telemetrySample;
      },
      get activePipelineIds() {
        return activePipelineIds;
      },
      get pipelineGridRowIndices() {
        return pipelineGridRowIndices;
      },
      get pipelineGridColumnIndices() {
        return pipelineGridColumnIndices;
      },
      get pipelineGridIsSingle() {
        return pipelineGridIsSingle;
      },
      get pipelineGridIsMultiplex() {
        return pipelineGridIsMultiplex;
      },
      get pipelineAssignFilteredGraphs() {
        return pipelineAssignFilteredGraphs;
      },
      get pipelineTuningGraph() {
        return pipelineTuningGraph;
      },
      get pipelineTuningUi() {
        return pipelineTuningUi;
      },
      get pipelineTuningPlan() {
        return pipelineTuningPlan;
      },
      get canShowTuningEngineConfig() {
        return canShowTuningEngineConfig;
      },
      get pipelineTuningBaseNodeOverrides() {
        return pipelineTuningBaseNodeOverrides;
      },
      get pipelineTuningCameraNodeOverrides() {
        return pipelineTuningCameraNodeOverrides;
      },
      get pipelineTuningEffectiveNodeOverrides() {
        return pipelineTuningEffectiveNodeOverrides;
      },
      get pipelineTuningNodeDescriptors() {
        return pipelineTuningNodeDescriptors;
      }
    };
  }
</script>
