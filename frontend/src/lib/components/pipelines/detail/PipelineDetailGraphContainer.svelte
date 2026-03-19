<script lang="ts">
  import type { PipelineDetailContext } from '$lib/components/pipelines/types';
  import type { StreamInfo } from '$lib/ts-bindings/http/client';
  import type {
    PipelineDiagnosticWarning,
    PipelineGraphPlan,
    PipelineNodeLayout,
    PipelineNodeSyncConfig,
    PipelineRegistryEntry,
    PipelineStreamNodeMetrics
  } from '$lib/types/pipeline';
  import type { PipelineGraphHeatmap } from '$lib/components/flow/pipeline-graph/types';
  import { detectGpuSegments, gpuSegmentColor } from '$lib/components/flow/pipeline-graph/gpuOverlay';
  import { isPipelineInputNode, isPipelineOutputNode } from '$lib/features/pipelines/boundary';
  import {
    formatHeatDuration,
    formatTimestamp,
    normalizeHeatmapNodeId,
    shouldIncludeHeatmapNode
  } from '$lib/components/pipelines/detail/pipelineDetailMetricsUtils';
  import PipelineDetailGraphSection from '$lib/components/pipelines/detail/PipelineDetailGraphSection.svelte';
  import { resolveStreamLabel } from '$lib/utils/streamLabels';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';

  type PipelineBreadcrumb = {
    id: string;
    name: string;
    status?: 'embedded' | 'linked' | 'mismatch' | 'unresolved';
    targetId?: string | null;
  };

  type HeatmapComputation = {
    maxValue: number;
    minValue: number;
    nodes: PipelineGraphHeatmap['nodes'];
  };

  type HeatmapViewMode = 'all' | 'workload' | 'boundary';

  type HeatmapNodeIndexEntry = {
    id: string;
    label: string;
    keywords: string;
  };

  type HeatmapFilters = {
    viewMode: HeatmapViewMode;
    boundaryIndex: Record<string, boolean>;
    excludedNodes: Record<string, boolean>;
    searchQuery: string;
    nodeIndex: Record<string, HeatmapNodeIndexEntry>;
    minAverageMs: number | null;
    minSampleCount: number | null;
  };

  type HeatmapStreamOption = { id: string; label: string; hasMetrics: boolean };

  type Props = {
    context: PipelineDetailContext;
    graphPlan: PipelineGraphPlan;
    registryEntries: PipelineRegistryEntry[];
    breadcrumbs: PipelineBreadcrumb[];
    canShowEngineConfig: boolean;
    engineConfigOpen: boolean;
    warningsPanelOpen: boolean;
    pipelineWarnings: PipelineDiagnosticWarning[];
    syncOverlayEnabled: boolean;
    normalizedGraphSearchQuery: string;
    metricsInspectorActive: boolean;
    captureDevices: readonly StreamInfo[];
    onExitEmbedded: () => void;
    onCloseEngineConfig: () => void;
    onCloseWarnings: () => void;
    onFocusWarning: (warning: PipelineDiagnosticWarning) => void;
    onRefreshMetrics: () => void;
    onPlanChange: (plan: PipelineGraphPlan) => void;
    onGraphSelect: (payload: { nodeId: string | null; nodes: string[]; edge: unknown }) => void;
    onEnterEmbedded: (nodeId: string) => void;
    onGraphContext: (payload: {
      type: 'pane' | 'node' | 'palette' | 'port';
      position: { x: number; y: number };
      flowPosition: { x: number; y: number };
      nodeId?: string | null;
      port?: string | null;
      direction?: 'input' | 'output';
    }) => void;
    onGraphLayout: (layout: PipelineNodeLayout) => void;
    onRuntime: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
    onSetSyncConfig: (payload: { nodeId: string; config: PipelineNodeSyncConfig | null }) => void;
    onSetDaedalusNodeRuntime: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
    heatmapEnabled?: boolean;
    gpuOverlayEnabled?: boolean;
    graphEditor?: unknown;
  };

  let {
    context,
    graphPlan,
    registryEntries,
    breadcrumbs,
    canShowEngineConfig,
    engineConfigOpen,
    warningsPanelOpen,
    pipelineWarnings,
    syncOverlayEnabled,
    normalizedGraphSearchQuery,
    metricsInspectorActive,
    captureDevices,
    onExitEmbedded,
    onCloseEngineConfig,
    onCloseWarnings,
    onFocusWarning,
    onRefreshMetrics,
    onPlanChange,
    onGraphSelect,
    onEnterEmbedded,
    onGraphContext,
    onGraphLayout,
    onRuntime,
    onSetSyncConfig,
    onSetDaedalusNodeRuntime,
    heatmapEnabled = $bindable(false),
    gpuOverlayEnabled = $bindable(false),
    graphEditor = $bindable(null)
  }: Props = $props();
  const GRAPH_VIEWPORT_MIN_HEIGHT = 400;
  let graphViewportElement = $state<HTMLDivElement | null>(null);
  let graphViewportHeight = $state(GRAPH_VIEWPORT_MIN_HEIGHT);

  const applyGraphViewportHeight = (height: number) => {
    if (!Number.isFinite(height) || height <= 0) return;
    const next = Math.max(GRAPH_VIEWPORT_MIN_HEIGHT, Math.floor(height));
    if (next !== graphViewportHeight) {
      graphViewportHeight = next;
    }
  };

  const measureGraphViewport = () => {
    const measured = graphViewportElement?.getBoundingClientRect().height ?? graphViewportElement?.clientHeight ?? 0;
    if (measured > 0) {
      applyGraphViewportHeight(measured);
    }
  };

  $effect(() => {
    if (!graphViewportElement || typeof window === 'undefined') {
      return;
    }
    let frameHandle: number | null = null;
    const scheduleMeasure = () => {
      if (frameHandle != null) {
        window.cancelAnimationFrame(frameHandle);
      }
      frameHandle = window.requestAnimationFrame(() => {
        frameHandle = null;
        measureGraphViewport();
      });
    };
    scheduleMeasure();
    window.addEventListener('resize', scheduleMeasure, { passive: true });
    return () => {
      if (frameHandle != null) {
        window.cancelAnimationFrame(frameHandle);
      }
      window.removeEventListener('resize', scheduleMeasure);
    };
  });

  $effect(() => {
    void breadcrumbs.length;
    void warningsPanelOpen;
    void engineConfigOpen;
    if (typeof window === 'undefined') return;
    const handle = window.requestAnimationFrame(() => {
      measureGraphViewport();
    });
    return () => window.cancelAnimationFrame(handle);
  });

  let heatmapStreamId = $state<string | null>(null);
  let heatmapStreamInitialized = false;
  let forcedHeatmapRestore: boolean | null = null;
  let heatmapViewMode = $state<HeatmapViewMode>('all');
  let heatmapFilterQuery = $state('');
  let heatmapMinAverageInput = $state('');
  let heatmapMinSamplesInput = $state('');
  let heatmapExcludedNodes = $state<Record<string, boolean>>({});

  const isHostIoNode = (node: PipelineGraphPlan['nodes'][string] | undefined | null): boolean => {
    if (!node) return false;
    const backendId = (node.backendId ?? '').toLowerCase();
    if (isPipelineInputNode(node) || isPipelineOutputNode(node)) return true;
    if (backendId === 'io.host_bridge' || backendId.endsWith(':io.host_bridge')) return true;
    if (backendId === 'io.host_output' || backendId.endsWith(':io.host_output')) return true;
    return false;
  };

  const pipelineBoundaryIndex = $derived.by<Record<string, boolean>>(() => {
    if (!heatmapEnabled) {
      return {};
    }
    const nodes = context.pipeline?.graph?.nodes ?? {};
    const index: Record<string, boolean> = {};
    Object.entries(nodes).forEach(([nodeId, node]) => {
      index[nodeId] = isHostIoNode(node);
    });
    return index;
  });

  const registryByBackendId = $derived.by(() => {
    if (!gpuOverlayEnabled) {
      return new SvelteMap<string, PipelineRegistryEntry>();
    }
    const map = new SvelteMap<string, PipelineRegistryEntry>();
    for (const entry of registryEntries ?? []) {
      const id = entry?.id?.trim();
      if (!id) continue;
      map.set(id, entry);
    }
    return map;
  });

  const heatmapViewModes: Array<{ id: HeatmapViewMode; label: string; description: string }> = [
    { id: 'all', label: 'All', description: 'Show every node' },
    { id: 'workload', label: 'Workload', description: 'Exclude pipeline IO' },
    { id: 'boundary', label: 'Pipeline IO', description: 'Only pipeline IO nodes' }
  ];

  const normalizedHeatmapFilterQuery = $derived.by(() => (heatmapEnabled ? heatmapFilterQuery.trim().toLowerCase() : ''));
  const heatmapMinAverageMs = $derived.by<number | null>(() => {
    const normalized = heatmapMinAverageInput.trim();
    if (normalized.length === 0) return null;
    const parsed = Number(normalized);
    if (!Number.isFinite(parsed) || parsed < 0) return null;
    return parsed;
  });
  const heatmapMinSampleCount = $derived.by<number | null>(() => {
    const normalized = heatmapMinSamplesInput.trim();
    if (normalized.length === 0) return null;
    const parsed = Number(normalized);
    if (!Number.isFinite(parsed) || parsed < 0) return null;
    return Math.floor(parsed);
  });

  const heatmapNodeIndex = $derived.by<Record<string, HeatmapNodeIndexEntry>>(() => {
    if (!heatmapEnabled) {
      return {};
    }
    const index: Record<string, HeatmapNodeIndexEntry> = {};
    const graphNodes = context.pipeline?.graph?.nodes ?? {};
    const trim = (value: unknown) => (typeof value === 'string' ? value.trim() : '');
    const register = (
      id: string | null | undefined,
      label: string,
      aliases: Array<string | null | undefined> = []
    ) => {
      const normalizedId = normalizeHeatmapNodeId(id);
      if (!normalizedId) return;
      const normalizedLabel = label.trim().length > 0 ? label.trim() : normalizedId;
      const existing = index[normalizedId];
      const tokens = new SvelteSet(
        (existing?.keywords ?? '')
          .split(' ')
          .map((token) => token.trim())
          .filter(Boolean)
      );
      [normalizedLabel, normalizedId, ...aliases].forEach((token) => {
        if (typeof token === 'string' && token.trim().length > 0) {
          tokens.add(token.trim().toLowerCase());
        }
      });
      index[normalizedId] = {
        id: normalizedId,
        label: existing?.label ?? normalizedLabel,
        keywords: Array.from(tokens).join(' ')
      };
    };

    Object.entries(graphNodes).forEach(([nodeKey, node]) => {
      const aliases = [
        nodeKey,
        node?.id,
        node?.backendId,
        node?.info?.id,
        node?.source?.id,
        node?.source?.info?.id
      ]
        .map((entry) => normalizeHeatmapNodeId(entry))
        .filter((entry): entry is string => Boolean(entry));
      if (aliases.length === 0) {
        return;
      }
      const label =
        trim(node?.metadata?.name) ||
        trim(node?.source?.metadata?.name) ||
        trim(node?.source?.id) ||
        trim(node?.info?.id) ||
        aliases[0];
      aliases.forEach((alias) => register(alias, label, aliases));
    });

    const metrics = context.metrics ?? [];
    metrics.forEach((entry) => {
      const streamMetrics = entry?.metrics ?? {};
      Object.keys(streamMetrics).forEach((nodeId) => {
        const normalizedId = normalizeHeatmapNodeId(nodeId);
        if (!normalizedId) return;
        register(normalizedId, index[normalizedId]?.label ?? normalizedId, [normalizedId]);
      });
    });

    return index;
  });

  const heatmapExcludedCount = $derived.by(() =>
    Object.values(heatmapExcludedNodes).filter(Boolean).length
  );

  const heatmapActiveFilterCount = $derived.by(() => {
    if (!heatmapEnabled) return 0;
    let count = 0;
    if (normalizedHeatmapFilterQuery) count += 1;
    if (heatmapMinAverageMs != null && heatmapMinAverageMs > 0) count += 1;
    if (heatmapMinSampleCount != null && heatmapMinSampleCount > 0) count += 1;
    return count + heatmapExcludedCount;
  });

  const gpuOverlaySummary = $derived.by(() => {
    if (!gpuOverlayEnabled) {
      return {
        segments: [],
        totalGpuNodes: 0,
        sharedSegments: 0,
        ungroupedLabels: []
      };
    }
    const detected = detectGpuSegments(graphPlan, {
      resolveRegistryEntry: (node) => registryByBackendId.get(node.backendId) ?? null
    });
    const validationSegmentsRaw = context.pipeline?.diagnostics?.gpu?.segments ?? [];
    const validationSegments = validationSegmentsRaw
      .map((segment) => {
        const bufferId = typeof segment?.bufferId === 'number' && Number.isFinite(segment.bufferId) ? segment.bufferId : null;
        const nodes = Array.isArray(segment?.nodes)
          ? segment.nodes.filter((nodeId): nodeId is string => typeof nodeId === 'string' && nodeId.trim().length > 0)
          : [];
        if (bufferId == null || nodes.length === 0) return null;
        return { id: bufferId + 1, nodes };
      })
      .filter((segment): segment is { id: number; nodes: string[] } => Boolean(segment));
    const segments = validationSegments.length > 0 ? validationSegments : detected.segments;
    const gpuNodeIds =
      validationSegments.length > 0
        ? new SvelteSet<string>(validationSegments.flatMap((segment) => segment.nodes))
        : detected.gpuNodeIds;
    const nodes = graphPlan?.nodes ?? {};
    const resolveLabel = (nodeId: string): string => {
      const node = nodes?.[nodeId];
      const candidates = [
        node?.metadata?.name,
        node?.source?.metadata?.name,
        node?.backendId,
        node?.id,
        node?.info?.id,
        node?.source?.id
      ];
      const label = candidates.find(
        (value): value is string => typeof value === 'string' && value.trim().length > 0
      );
      return (label ?? nodeId).trim();
    };
    const assigned = new SvelteSet<string>();
    const decoratedSegments = segments.map((segment) => {
      segment.nodes.forEach((id) => assigned.add(id));
      return {
        id: String(segment.id),
        nodes: segment.nodes,
        labels: segment.nodes.map((id) => resolveLabel(id)),
        shared: segment.nodes.length > 1,
        color: gpuSegmentColor(segment.id)
      };
    });
    const ungrouped = Array.from(gpuNodeIds).filter((id) => !assigned.has(id));
    return {
      segments: decoratedSegments,
      totalGpuNodes: gpuNodeIds.size,
      sharedSegments: decoratedSegments.filter((segment) => segment.shared).length,
      ungroupedLabels: ungrouped.map((id) => resolveLabel(id)).filter(Boolean)
    };
  });

  const gpuOverlaySegments = $derived.by<Array<{ id: number; nodes: string[] }> | null>(() => {
    if (!gpuOverlayEnabled) {
      return null;
    }
    const validationSegmentsRaw = context.pipeline?.diagnostics?.gpu?.segments ?? [];
    const mapped = validationSegmentsRaw
      .map((segment) => {
        const bufferId = typeof segment?.bufferId === 'number' && Number.isFinite(segment.bufferId) ? segment.bufferId : null;
        const nodes = Array.isArray(segment?.nodes)
          ? segment.nodes.filter((nodeId): nodeId is string => typeof nodeId === 'string' && nodeId.trim().length > 0)
          : [];
        if (bufferId == null || nodes.length === 0) return null;
        return { id: bufferId + 1, nodes };
      })
      .filter((segment): segment is { id: number; nodes: string[] } => Boolean(segment));
    return mapped.length > 0 ? mapped : null;
  });

  function toggleHeatmapExclusion(nodeId: string) {
    const normalized = normalizeHeatmapNodeId(nodeId);
    if (!normalized) return;
    const current = { ...heatmapExcludedNodes };
    if (current[normalized]) {
      delete current[normalized];
    } else {
      current[normalized] = true;
    }
    heatmapExcludedNodes = current;
  }

  export function toggleHeatmapNodeExclusion(nodeId: string | null | undefined) {
    if (!nodeId) return;
    toggleHeatmapExclusion(nodeId);
  }

  export function isHeatmapNodeExcluded(nodeId: string | null | undefined): boolean {
    const normalized = normalizeHeatmapNodeId(nodeId);
    if (!normalized) return false;
    return Boolean(heatmapExcludedNodes[normalized]);
  }

  function clearHeatmapExclusions() {
    heatmapExcludedNodes = {};
  }

  function clearHeatmapFilters() {
    heatmapFilterQuery = '';
    heatmapMinAverageInput = '';
    heatmapMinSamplesInput = '';
    clearHeatmapExclusions();
  }

  const HEATMAP_SELECTOR_REFRESH_INTERVAL_MS = 750;
  let lastHeatmapSelectorRefresh = 0;

  function requestHeatmapMetricsRefresh(reason: string) {
    const current = Date.now();
    if (current - lastHeatmapSelectorRefresh < HEATMAP_SELECTOR_REFRESH_INTERVAL_MS && reason !== 'toggle') {
      return;
    }
    lastHeatmapSelectorRefresh = current;
    onRefreshMetrics();
  }

  function handleHeatmapSelectorEnter() {
    requestHeatmapMetricsRefresh('hover-enter');
  }

  function handleHeatmapSelectorLeave() {
    requestHeatmapMetricsRefresh('hover-leave');
  }

  const resolveCaptureDeviceLabel = (
    streamId: string | null | undefined,
    cameraUid: string | null | undefined
  ): string | null => {
    if (!captureDevices || captureDevices.length === 0) {
      return null;
    }
    const normalizedStreamId = typeof streamId === 'string' ? streamId.trim() : null;
    const normalizedCameraUid = typeof cameraUid === 'string' ? cameraUid.trim() : null;
    const match = captureDevices.find((device) => {
      const normalizedDeviceId = typeof device.id === 'string' ? device.id.trim() : '';
      const keys = device.manifest?.identity?.keys ?? [];
      if (normalizedStreamId && normalizedDeviceId === normalizedStreamId) {
        return true;
      }
      if (normalizedStreamId && keys.includes(normalizedStreamId)) {
        return true;
      }
      if (normalizedCameraUid && keys.includes(normalizedCameraUid)) {
        return true;
      }
      return false;
    });
    if (!match) {
      return null;
    }
    const label = resolveStreamLabel(match, '');
    return label ? label : null;
  };

  function streamLabel(entry: PipelineStreamNodeMetrics): string {
    const captureLabel = resolveCaptureDeviceLabel(entry.streamId, null);
    if (captureLabel) {
      return captureLabel;
    }
    if (entry.streamPath && entry.streamPath.trim().length > 0) {
      return entry.streamPath;
    }
    return entry.streamId;
  }

  const heatmapStreamOptions = $derived.by<HeatmapStreamOption[]>(() => {
    if (!heatmapEnabled) {
      return [];
    }
    const options: HeatmapStreamOption[] = [];
    const seen = new SvelteSet<string>();
    const metrics = context.metrics ?? [];

    if (metrics && metrics.length > 0) {
      for (const entry of metrics) {
        if (!entry?.streamId || seen.has(entry.streamId)) {
          continue;
        }
        seen.add(entry.streamId);
        options.push({ id: entry.streamId, label: streamLabel(entry), hasMetrics: true });
      }
    }

    return options;
  });

  $effect(() => {
    const options = heatmapStreamOptions;
    if (options.length === 0) {
      heatmapStreamId = null;
      heatmapStreamInitialized = false;
      return;
    }
    if (!heatmapStreamInitialized) {
      heatmapStreamId = options[0]?.id ?? null;
      heatmapStreamInitialized = true;
      return;
    }
    if (heatmapStreamId && !options.some((option) => option.id === heatmapStreamId)) {
      heatmapStreamId = options[0]?.id ?? null;
    }
  });

  const selectedHeatmapStreamLabel = $derived.by(() => {
    if (!heatmapEnabled) return 'All streams';
    const option = heatmapStreamOptions.find((entry) => entry.id === heatmapStreamId);
    if (option) return option.label;
    return 'All streams';
  });

  function handleHeatmapStreamChange(event: Event) {
    const target = event.currentTarget as HTMLSelectElement | null;
    if (!target) return;
    const value = target.value;
    heatmapStreamId = value || null;
  }

  const runtimeWarnings = $derived.by<Record<string, { message: string; at: number | null }>>(() => {
    const out: Record<string, { message: string; at: number | null }> = {};
    const metrics = context.metrics ?? [];
    metrics.forEach((entry) => {
      Object.entries(entry.metrics ?? {}).forEach(([nodeId, runtime]) => {
        if (!runtime?.lastError) return;
        const at = runtime.lastErrorAt ?? null;
        const current = out[nodeId];
        if (!current || (at ?? 0) > (current.at ?? 0)) {
          out[nodeId] = { message: runtime.lastError ?? '', at };
        }
      });
    });
    return out;
  });

  const heatmapMetricsSource = $derived.by<PipelineStreamNodeMetrics[]>(() => {
    const metrics = context.metrics ?? [];
    if (!metrics || metrics.length === 0) {
      return [];
    }
    if (!heatmapStreamId) {
      return metrics;
    }
    return metrics.filter((entry) => entry.streamId === heatmapStreamId);
  });

  const computeHeatmap = (
    metrics: PipelineStreamNodeMetrics[],
    filters: HeatmapFilters
  ): HeatmapComputation | null => {
    if (!metrics || metrics.length === 0) {
      return null;
    }

    const includeCache = new SvelteMap<string, string | null>();
    const resolveIncludedNodeId = (nodeId: string): string | null => {
      if (includeCache.has(nodeId)) {
        return includeCache.get(nodeId) ?? null;
      }
      const normalized = normalizeHeatmapNodeId(nodeId);
      if (!normalized || !shouldIncludeHeatmapNode(normalized, filters)) {
        includeCache.set(nodeId, null);
        return null;
      }
      includeCache.set(nodeId, normalized);
      return normalized;
    };

    const nodes: PipelineGraphHeatmap['nodes'] = {};
    for (const stream of metrics) {
      const streamMetrics = stream?.metrics ?? {};
      for (const [nodeId, runtime] of Object.entries(streamMetrics)) {
        const stats = runtime?.metrics;
        if (!stats) continue;
        const normalizedNodeId = resolveIncludedNodeId(nodeId);
        if (!normalizedNodeId) continue;

        const average = stats.averageTimeMs ?? null;
        if (average == null || !Number.isFinite(average) || average <= 0) {
          continue;
        }
        const sampleCount = stats.sampleCount ?? 0;
        const entry =
          nodes[normalizedNodeId] ??
          ({
            totalTimeMs: 0,
            peakTimeMs: 0,
            averageFps: null,
            sampleCount: 0,
            streamCount: 0
          } as PipelineGraphHeatmap['nodes'][string]);
        entry.totalTimeMs += average;
        entry.peakTimeMs = Math.max(entry.peakTimeMs, average);
        entry.sampleCount += sampleCount;
        entry.streamCount += 1;
        const fps = stats.averageFps ?? null;
        if (fps != null && Number.isFinite(fps)) {
          entry.averageFps = (entry.averageFps ?? 0) + fps;
        }
        nodes[normalizedNodeId] = entry;
      }
    }
    const filteredNodes: PipelineGraphHeatmap['nodes'] = {};
    let maxValue = 0;
    let minValue = Number.POSITIVE_INFINITY;
    Object.entries(nodes).forEach(([nodeId, entry]) => {
      if (entry.averageFps != null && entry.streamCount > 0) {
        entry.averageFps = entry.averageFps / entry.streamCount;
      } else {
        entry.averageFps = null;
      }
      const averagedTime = entry.streamCount > 0 ? entry.totalTimeMs / entry.streamCount : entry.totalTimeMs;
      entry.totalTimeMs = averagedTime;
      if (filters.minSampleCount != null && entry.sampleCount < filters.minSampleCount) {
        return;
      }
      if (filters.minAverageMs != null && averagedTime < filters.minAverageMs) {
        return;
      }
      if (!Number.isFinite(averagedTime) || averagedTime <= 0) {
        return;
      }
      filteredNodes[nodeId] = entry;
      if (averagedTime > maxValue) {
        maxValue = averagedTime;
      }
      if (averagedTime < minValue) {
        minValue = averagedTime;
      }
    });
    if (Object.keys(filteredNodes).length === 0) {
      return null;
    }
    if (!Number.isFinite(maxValue) || maxValue <= 0) {
      return null;
    }
    if (!Number.isFinite(minValue) || minValue <= 0) {
      minValue = maxValue;
    }
    return { maxValue, minValue, nodes: filteredNodes };
  };

  let heatmapComputationState = $state<HeatmapComputation | null>(null);
  let heatmapComputationHandle: number | null = null;
  let heatmapThrottleHandle: number | null = null;
  let pendingHeatmapPayload: { metrics: PipelineStreamNodeMetrics[]; filters: HeatmapFilters } | null = null;
  let lastHeatmapComputeAt = 0;
  let lastHeatmapSignature = 'null';

  const heatmapNow = () =>
    typeof performance !== 'undefined' && typeof performance.now === 'function'
      ? performance.now()
      : Date.now();

  const HEATMAP_MIN_INTERVAL_MS = 520;

  const signatureForHeatmap = (payload: HeatmapComputation | null): string => {
    if (!payload) return 'null';
    let count = 0;
    let total = 0;
    for (const entry of Object.values(payload.nodes ?? {})) {
      count += 1;
      total += Number(entry.totalTimeMs ?? 0);
    }
    return `${payload.minValue.toFixed(3)}|${payload.maxValue.toFixed(3)}|${count}|${total.toFixed(3)}`;
  };

  const applyHeatmapState = (next: HeatmapComputation | null) => {
    const signature = signatureForHeatmap(next);
    if (signature === lastHeatmapSignature) {
      return;
    }
    lastHeatmapSignature = signature;
    heatmapComputationState = next;
  };

  function cancelPendingHeatmapComputation() {
    if (heatmapComputationHandle != null && typeof window !== 'undefined') {
      window.cancelAnimationFrame(heatmapComputationHandle);
    }
    if (heatmapThrottleHandle != null && typeof window !== 'undefined') {
      window.clearTimeout(heatmapThrottleHandle);
    }
    heatmapComputationHandle = null;
    heatmapThrottleHandle = null;
    pendingHeatmapPayload = null;
  }

  function scheduleHeatmapComputation(
    metrics: PipelineStreamNodeMetrics[],
    filters: HeatmapFilters
  ) {
    pendingHeatmapPayload = { metrics, filters };

    const run = () => {
      const payload = pendingHeatmapPayload;
      heatmapComputationHandle = null;
      pendingHeatmapPayload = null;
      if (!payload) return;
      lastHeatmapComputeAt = heatmapNow();
      applyHeatmapState(computeHeatmap(payload.metrics, payload.filters));
    };

    if (typeof window === 'undefined') {
      run();
      return;
    }
    const elapsed = Math.max(0, heatmapNow() - lastHeatmapComputeAt);
    if (elapsed >= HEATMAP_MIN_INTERVAL_MS) {
      heatmapComputationHandle = window.requestAnimationFrame(run);
      return;
    }
    if (heatmapThrottleHandle != null) {
      window.clearTimeout(heatmapThrottleHandle);
    }
    heatmapThrottleHandle = window.setTimeout(() => {
      heatmapThrottleHandle = null;
      heatmapComputationHandle = window.requestAnimationFrame(run);
    }, HEATMAP_MIN_INTERVAL_MS - elapsed);
  }

  $effect(() => {
    const metrics = heatmapMetricsSource;
    const enabled = heatmapEnabled;
    const viewMode = heatmapViewMode;
    const boundarySignature = pipelineBoundaryIndex;
    const filters: HeatmapFilters = {
      viewMode,
      boundaryIndex: boundarySignature,
      excludedNodes: heatmapExcludedNodes,
      searchQuery: normalizedHeatmapFilterQuery,
      nodeIndex: heatmapNodeIndex,
      minAverageMs: heatmapMinAverageMs,
      minSampleCount: heatmapMinSampleCount
    };
    if (!enabled) {
      cancelPendingHeatmapComputation();
      applyHeatmapState(null);
      return;
    }
    cancelPendingHeatmapComputation();
    scheduleHeatmapComputation(metrics, filters);
    return () => cancelPendingHeatmapComputation();
  });

  const graphHeatmap = $derived.by<PipelineGraphHeatmap | null>(() => {
    if (!heatmapEnabled || !heatmapComputationState) {
      return null;
    }
    return {
      enabled: true,
      maxValue: heatmapComputationState.maxValue,
      minValue: heatmapComputationState.minValue,
      nodes: heatmapComputationState.nodes
    };
  });

  $effect(() => {
    if (metricsInspectorActive) {
      if (!heatmapEnabled) {
        if (forcedHeatmapRestore === null) {
          forcedHeatmapRestore = heatmapEnabled;
        }
        heatmapEnabled = true;
      }
    } else if (forcedHeatmapRestore !== null) {
      heatmapEnabled = forcedHeatmapRestore;
      forcedHeatmapRestore = null;
    }
  });

  let previousPipelineId: string | null = null;
  $effect(() => {
    const currentPipelineId = context.pipeline?.id ?? null;
    if (currentPipelineId !== previousPipelineId) {
      heatmapEnabled = false;
      heatmapStreamId = null;
      heatmapStreamInitialized = false;
      heatmapViewMode = 'all';
      heatmapFilterQuery = '';
      heatmapMinAverageInput = '';
      heatmapMinSamplesInput = '';
      heatmapExcludedNodes = {};
      previousPipelineId = currentPipelineId;
    }
  });
</script>

<PipelineDetailGraphSection
  {context}
  {graphPlan}
  {registryEntries}
  bind:graphEditor={graphEditor}
  bind:graphViewportElement={graphViewportElement}
  {graphViewportHeight}
  {normalizedGraphSearchQuery}
  {graphHeatmap}
  {heatmapEnabled}
  {gpuOverlayEnabled}
  {gpuOverlaySegments}
  {runtimeWarnings}
  {syncOverlayEnabled}
  {warningsPanelOpen}
  {pipelineWarnings}
  {canShowEngineConfig}
  {engineConfigOpen}
  {heatmapStreamId}
  {heatmapStreamOptions}
  {selectedHeatmapStreamLabel}
  {heatmapViewModes}
  {heatmapViewMode}
  {heatmapActiveFilterCount}
  bind:heatmapFilterQuery={heatmapFilterQuery}
  bind:heatmapMinAverageInput={heatmapMinAverageInput}
  bind:heatmapMinSamplesInput={heatmapMinSamplesInput}
  {heatmapExcludedCount}
  {gpuOverlaySummary}
  metricsUpdatedAt={context.metricsUpdatedAt}
  metricsStatus={context.metricsStatus}
  {breadcrumbs}
  onExitEmbedded={onExitEmbedded}
  onCloseEngineConfig={onCloseEngineConfig}
  onCloseWarnings={onCloseWarnings}
  onFocusWarning={onFocusWarning}
  onHeatmapStreamChange={handleHeatmapStreamChange}
  onHeatmapSelectorEnter={handleHeatmapSelectorEnter}
  onHeatmapSelectorLeave={handleHeatmapSelectorLeave}
  onSetHeatmapViewMode={(mode) => (heatmapViewMode = mode as HeatmapViewMode)}
  onClearHeatmapFilters={clearHeatmapFilters}
  {formatHeatDuration}
  {formatTimestamp}
  onPlanChange={onPlanChange}
  onGraphSelect={onGraphSelect}
  onEnterEmbedded={onEnterEmbedded}
  onGraphContext={onGraphContext}
  onGraphLayout={onGraphLayout}
  onRuntime={onRuntime}
  onSetSyncConfig={onSetSyncConfig}
  onSetDaedalusNodeRuntime={onSetDaedalusNodeRuntime}
/>
