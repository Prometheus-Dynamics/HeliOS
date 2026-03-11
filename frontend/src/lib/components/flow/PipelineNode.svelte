<script lang="ts">
import { canonicalPortName } from '$lib/features/pipelines/workbench/inspector/syncPolicyUtils';
  import type {
    PipelineDataType,
    PipelineGraphNode,
    PipelineNodeStyle,
    PipelineRegistryEntry,
    PipelineSyncGroupConfig
  } from '$lib/types/pipeline';
import { buildNodePalette, mergeNodeStyles } from './nodePalette';
import { resolveNodeTextPalette } from './nodeColors';
import type { PipelineNodeDiagnostics, PipelineNodeHeatmapPayload } from './pipeline-graph/types';
import { typeKey } from './pipeline-graph/utils';
import { gpuSegmentColor } from './pipeline-graph/gpuOverlay';
import PipelinePortList from './pipeline-node/PipelinePortList.svelte';
import PipelinePortHandles from './pipeline-node/PipelinePortHandles.svelte';
import NodeHeader from './pipeline-node/NodeHeader.svelte';
import NodePorts from './pipeline-node/NodePorts.svelte';
import NodeStatus from './pipeline-node/NodeStatus.svelte';
import NodeActions from './pipeline-node/NodeActions.svelte';
import { pixelColorInputAction } from './pipeline-node/pixelInputs';
import { formatHeatTime, formatHeatDelta } from './pipeline-node/heatUtils';
import { buildNodeHeatmapState } from './pipeline-node/nodeHeatmapState';
import {
  buildCompactPortHandleList,
  buildPortList,
  normalizePortName,
  normalizeString
} from './pipeline-node/portUtils';
import { createPortHandlers } from './pipeline-node/portHandlers';
import {
  type CompactPortHandle,
  type PipelineNodeData,
  type PortSyncAssignments
} from './pipeline-node/types';
import { SvelteMap } from 'svelte/reactivity';
const SYNC_GROUP_COLORS = ['#38bdf8', '#f472b6', '#a855f7', '#f97316', '#22d3ee', '#facc15'];

const props = $props<{ selected?: boolean; data?: PipelineNodeData }>();
const selected = $derived(props.selected ?? false);
const data = $derived(props.data);
const node = $derived(data?.node);
const apiNode = $derived(data?.apiNode ?? null);
const registryEntry = $derived((data?.registryEntry ?? null) as PipelineRegistryEntry | null);
const nodeHeatmapMode = $derived(Boolean(data?.heatmapMode));
const nodeDetailLevel = $derived((data?.detailLevel ?? 'full') as 'minimal' | 'full');
const nodeGpuSegment = $derived((data?.gpuSegment ?? null) as number | null);
const nodeGpuPeers = $derived((data?.gpuPeers ?? null) as number | null);
const gpuOverlayEnabled = $derived(Boolean(data?.gpuOverlay));
const nodeGpuBuffered = $derived(Boolean(gpuOverlayEnabled && nodeGpuSegment));
const nodeGpuMuted = $derived(Boolean(gpuOverlayEnabled && !nodeGpuSegment));
const gpuSegmentColorValue = $derived(gpuSegmentColor(nodeGpuSegment));
const gpuSharedBuffer = $derived(Boolean(nodeGpuPeers && nodeGpuPeers > 1));
const gpuOutlineStyle = $derived.by(() => {
  if (!nodeGpuBuffered) return '';
  const outlineWidth = gpuSharedBuffer ? 3 : 2;
  const outlineColor = `color-mix(in srgb, ${gpuSegmentColorValue} 85%, transparent)`;
  const halo = gpuSharedBuffer
    ? `box-shadow: 0 0 0 6px color-mix(in srgb, ${gpuSegmentColorValue} 18%, transparent);`
    : `box-shadow: 0 0 0 4px color-mix(in srgb, ${gpuSegmentColorValue} 12%, transparent);`;
  return [
    `--pipeline-node-gpu-color:${gpuSegmentColorValue}`,
    `outline:${outlineWidth}px dashed ${outlineColor}`,
    'outline-offset: 3px',
    halo
  ].join(';');
});
const nodeDiagnostics = $derived((data?.diagnostics ?? null) as PipelineNodeDiagnostics | null);
const nodeHasDiagnostics = $derived(
  Boolean(
    nodeDiagnostics &&
      (nodeDiagnostics.hasNodeIssue ||
        nodeDiagnostics.inputPorts.length > 0 ||
        nodeDiagnostics.outputPorts.length > 0)
  )
);
const inputPortIssueIds = $derived((nodeDiagnostics?.inputPorts ?? []) as string[]);
const outputPortIssueIds = $derived((nodeDiagnostics?.outputPorts ?? []) as string[]);
const portIssueMessages = $derived((nodeDiagnostics?.portMessages ?? null) as Record<string, string[]> | null);
const nodeHighlight = $derived(
  (data?.highlight ?? null) as { port: string | null; token: number | null } | null
);
const nodeHighlightActive = $derived(Boolean(nodeHighlight));
const nodeSyncOverlay = $derived(
  (data?.syncOverlay ?? null) as { enabled: boolean; focus: boolean } | null
);
const nodeSyncModeActive = $derived(Boolean(nodeSyncOverlay?.enabled));
const nodeSyncFocusActive = $derived(Boolean(nodeSyncOverlay?.enabled && nodeSyncOverlay.focus));
const activeConnection = $derived(data?.activeConnection ?? null);
const searchTokens = $derived((data?.searchTokens ?? []) as string[]);
const searchActive = $derived(Boolean(data?.searchActive) && searchTokens.length > 0);
const nodeSearchMatch = $derived(Boolean(data?.searchMatch));
const showDetailedNode = $derived(
  selected ||
    nodeDetailLevel === 'full' ||
    nodeHighlightActive ||
    Boolean(activeConnection) ||
    searchActive
);
type DaedalusSyncGroup = { name?: string; ports?: string[] };
const nodeSyncPortAssignments = $derived.by<PortSyncAssignments>(() => {
  const assignments = new SvelteMap<string, { groupId: string; color: string }>();
  if (!showDetailedNode || !nodeSyncOverlay?.enabled) {
    return assignments;
  }
  const legacyGroups = node?.sync?.groups ?? null;
  if (legacyGroups && legacyGroups.length > 0) {
    legacyGroups.forEach((group: PipelineSyncGroupConfig, index: number) => {
      const groupId = group.id ?? `group-${index + 1}`;
      const color = SYNC_GROUP_COLORS[index % SYNC_GROUP_COLORS.length];
      group.ports?.forEach((port: string) => {
        const normalized = canonicalPortName(port);
        if (normalized) {
          assignments.set(normalized, { groupId, color });
        }
      });
    });
    return assignments;
  }
  const daedalusGroups = (node?.source as { sync_groups?: unknown } | null | undefined)?.sync_groups;
  if (Array.isArray(daedalusGroups)) {
    (daedalusGroups as DaedalusSyncGroup[]).forEach((group, index) => {
      const name = typeof group?.name === 'string' ? group.name.trim() : '';
      if (!name) return;
      const ports = Array.isArray(group.ports) ? group.ports : [];
      if (ports.length === 0) return;
      const color = SYNC_GROUP_COLORS[index % SYNC_GROUP_COLORS.length];
      ports.forEach((port) => {
        const normalized = canonicalPortName(port);
        if (normalized) {
          assignments.set(normalized, { groupId: name, color });
        }
      });
    });
  }
  return assignments;
});
const highlightedPortName = $derived(normalizePortName(nodeHighlight?.port));
const nodeStyle = $derived(
  (() => {
    const merged = mergeNodeStyles(
      (node?.metadata?.style as PipelineNodeStyle | null) ?? null,
      (apiNode?.metadata?.style as PipelineNodeStyle | null) ?? null,
      (registryEntry?.metadata?.style as PipelineNodeStyle | null) ?? null
    );
    return merged ?? null;
  })()
);
const nodePalette = $derived(buildNodePalette(nodeStyle, nodeHeatmapMode));
const nodeHeatmap = $derived((data?.heatmap ?? null) as PipelineNodeHeatmapPayload | null);
const nodeCardBackground = $derived((() => nodePalette.background)());
const nodeBorderColor = $derived((() => nodePalette.border)());
const nodeHeaderColor = $derived((() => nodePalette.header)());
const nodeHeatState = $derived.by(() =>
  buildNodeHeatmapState({
    heatmap: nodeHeatmap,
    heatmapMode: nodeHeatmapMode,
    borderColor: nodeBorderColor
  })
);
const nodeTextPalette = $derived(resolveNodeTextPalette(nodeCardBackground));
const nodeCardStyle = $derived(
  (() => {
    const textPalette = nodeTextPalette;
    const styles = [
      `background-color:${nodeCardBackground}`,
      `border-color:${nodeHeatState.effectiveBorderColor}`,
      `--pipeline-node-border-color:${nodeHeatState.effectiveBorderColor}`,
      `--pipeline-node-heat-color:${nodeHeatState.color ?? 'transparent'}`,
      `--pipeline-node-heat-opacity:${nodeHeatState.overlayOpacity}`,
      `--pipeline-node-heat-border-color:${nodeHeatState.color ?? 'transparent'}`,
      `--pipeline-node-heat-border-size:${nodeHeatState.borderSize}`,
      `--pipeline-node-heat-border-opacity:${nodeHeatState.borderOpacity}`,
      `--pipeline-node-text-strong:${textPalette.strong}`,
      `--pipeline-node-text-emphasis:${textPalette.emphasis}`,
      `--pipeline-node-text-muted:${textPalette.muted}`,
      `--pipeline-node-text-subtle:${textPalette.subtle}`,
      `--pipeline-node-badge-bg:${textPalette.badgeBackground}`,
      `--pipeline-node-badge-border:${textPalette.badgeBorder}`,
      `--pipeline-node-badge-text:${textPalette.badgeText}`
    ];
    if (gpuOutlineStyle) {
      styles.push(gpuOutlineStyle);
    }
    return styles.join(';');
  })()
);
const nodeHeaderStyle = $derived(
  (() => `background-color:${nodeHeaderColor};border-color:${nodeHeatState.effectiveBorderColor}`)()
);
const nodeWarningMessages = $derived(nodeDiagnostics?.nodeMessages ?? []);
const runtimeWarning = $derived((data?.runtimeWarning ?? null) as { message: string; at: number | null } | null);
const runtimeWarningTitle = $derived(
  (() => {
    if (!runtimeWarning) return '';
    const when = runtimeWarning.at ? new Date(runtimeWarning.at * 1000).toLocaleString() : null;
    return when ? `${runtimeWarning.message} · ${when}` : runtimeWarning.message;
  })()
);
const nodeHeatLegend = $derived.by(() => {
  if (!nodeHeatState.active) {
    return null;
  }
  if (!nodeHeatState.hasMetrics || !nodeHeatmap) {
    return {
      averageLabel: '—',
      streamsLabel: 'Awaiting metrics',
      fpsLabel: null,
      deltaLabel: '—',
      hasDelta: false,
      tooltip: 'Metrics have not been reported for this node yet'
    };
  }
  const averageTime = nodeHeatState.averageTime;
  const peakTime =
    Number.isFinite(nodeHeatmap.peakTimeMs) && nodeHeatmap.peakTimeMs > 0 ? nodeHeatmap.peakTimeMs : null;
  const fallbackTime = averageTime ?? peakTime ?? nodeHeatmap.totalTimeMs;
  const averageLabel = formatHeatTime(averageTime ?? fallbackTime);
  const deltaDisplay =
    typeof nodeHeatState.averageDelta === 'number' ? formatHeatDelta(nodeHeatState.averageDelta) : null;
  const deltaLabel = deltaDisplay ?? '—';
  const streamLabel = `${nodeHeatmap.streamCount} ${nodeHeatmap.streamCount === 1 ? 'stream' : 'streams'}`;
  const fpsValue =
    nodeHeatmap.averageFps != null && Number.isFinite(nodeHeatmap.averageFps)
      ? nodeHeatmap.averageFps
      : null;
  const fpsLabel = fpsValue != null ? `${fpsValue.toFixed(1)} fps` : null;
  const globalAverageLabel = formatHeatTime(nodeHeatState.globalAverage);
  const tooltipParts: string[] = [];
  if (averageLabel) {
    tooltipParts.push(`Avg ${averageLabel}`);
  }
  if (globalAverageLabel) {
    tooltipParts.push(`Graph avg ${globalAverageLabel}`);
  }
  if (deltaDisplay) {
    tooltipParts.push(`Δ vs avg ${deltaDisplay}`);
  }
  if (Number.isFinite(nodeHeatmap.totalTimeMs) && peakTime && peakTime > (averageTime ?? 0)) {
    tooltipParts.push(`Peak ${formatHeatTime(peakTime)}`);
  }
  tooltipParts.push(`${nodeHeatmap.sampleCount} sample${nodeHeatmap.sampleCount === 1 ? '' : 's'}`);
  tooltipParts.push(streamLabel);
  if (fpsLabel) {
    tooltipParts.push(`≈ ${fpsLabel}`);
  }
  return {
    averageLabel,
    streamsLabel: streamLabel,
    fpsLabel,
    deltaLabel,
    hasDelta: Boolean(deltaDisplay),
    tooltip: tooltipParts.join(' · ')
  };
});
const gpuBadge = $derived.by(() => {
  if (!nodeGpuSegment) return null;
  const peersLabel = nodeGpuPeers && nodeGpuPeers > 1 ? `${nodeGpuPeers} nodes` : 'solo';
  return {
    label: `GPU S${nodeGpuSegment}`,
    tooltip: `GPU segment ${nodeGpuSegment} (${peersLabel})`,
    shared: gpuSharedBuffer
  };
});
const gpuBadgeStyle = $derived.by(() => {
  if (!gpuBadge || !gpuOverlayEnabled) return '';
  const halo = gpuSharedBuffer
    ? `box-shadow: 0 0 0 1px color-mix(in srgb, ${gpuSegmentColorValue} 40%, transparent);`
    : '';
  return [
    `--pipeline-node-gpu-color:${gpuSegmentColorValue}`,
    `border-color: color-mix(in srgb, ${gpuSegmentColorValue} 75%, transparent)`,
    `background-color: color-mix(in srgb, ${gpuSegmentColorValue} 16%, transparent)`,
    halo
  ].join(';');
});
const nodeBaseId = $derived(node?.id ?? apiNode?.id ?? node?.info?.id ?? apiNode?.info?.id ?? '');
const displayBackendId = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const trimmed = value.trim();
  const lower = trimmed.toLowerCase();
  if (lower === 'io.host_bridge' || lower.endsWith(':io.host_bridge')) return 'pipeline:input';
  if (lower === 'io.host_output' || lower.endsWith(':io.host_output')) return 'pipeline:output';
  return trimmed;
};
const nodeName = $derived(
  normalizeString(apiNode?.metadata?.name) ??
    node?.metadata?.name ??
    displayBackendId(apiNode?.backendId) ??
    displayBackendId(node?.backendId) ??
    'Pipeline Node'
);
const nodeHasEmbeddedGraph = $derived.by(() => {
  if (node?.embedded) return true;
  const rawMeta = (node?.source as { metadata?: Record<string, unknown> } | null | undefined)?.metadata ?? null;
  return Boolean(rawMeta && typeof rawMeta === 'object' && 'daedalus.embedded_graph' in rawMeta);
});
const nodeIsGroup = $derived(
  node?.backendId?.toLowerCase() === 'pipeline:child' || nodeHasEmbeddedGraph
);
const nodeIdentity = $derived(
  node?.backendId ??
    normalizeString(apiNode?.metadata?.provider) ??
    node?.info?.id ??
    apiNode?.info?.id ??
    apiNode?.id ??
    node?.id ??
    ''
);
const summary = $derived(normalizeString(apiNode?.metadata?.summary) ?? node?.metadata?.summary ?? null);
const metaTags = $derived(
  (() => {
    if (Array.isArray(apiNode?.metadata?.tags)) {
      const mapped = (apiNode.metadata.tags as unknown[])
        .map((tag: unknown) => {
          if (typeof tag === 'string') return normalizeString(tag);
          if (tag == null) return null;
          return normalizeString(String(tag));
        })
        .filter((tag): tag is string => Boolean(tag));
      if (mapped.length > 0) return mapped;
    }
    return [...(node?.metadata?.tags ?? [])];
  })()
);
const childLinkState = $derived(
  (() => {
    if (!node || node.backendId?.toLowerCase() !== 'pipeline:child') return null;
    const external = node.external ?? (apiNode?.external as PipelineGraphNode['external']);
    const alias = external?.alias?.trim() || null;
    const targetId = external?.pipelineId?.trim() || null;
    const messages = nodeDiagnostics?.nodeMessages ?? [];
    const hasEmbedded = Boolean(node.embedded);
    const mismatch = messages.some((msg) => /mismatch/i.test(msg));
    const unresolved = messages.some((msg) => /resolve|missing|cycle/i.test(msg));
    const status: 'embedded' | 'linked' | 'mismatch' | 'unresolved' =
      hasEmbedded && !external
        ? 'embedded'
        : mismatch
          ? 'mismatch'
          : unresolved
            ? 'unresolved'
            : 'linked';
    return {
      status,
      alias: alias || targetId,
      targetId,
      hasExternal: Boolean(external),
      hasEmbedded,
      warnings: messages
    };
  })()
);
type BadgeTone = 'default' | 'info' | 'warning' | 'error';
type Badge = { label: string; tone: BadgeTone; tooltip?: string };
const badges = $derived(
  (() => {
    if (!showDetailedNode) return [];
    const items: Badge[] = (metaTags ?? []).map((label) => ({ label, tone: 'default' as BadgeTone }));
    if (childLinkState) {
      const statusLabel = (() => {
        switch (childLinkState.status) {
          case 'embedded':
            return 'Inline group';
          case 'linked':
            return `External${childLinkState.alias ? ` · ${childLinkState.alias}` : ''}`;
          case 'mismatch':
            return `Signature mismatch${childLinkState.alias ? ` · ${childLinkState.alias}` : ''}`;
          case 'unresolved':
          default:
            return `Missing${childLinkState.alias ? ` · ${childLinkState.alias}` : ''}`;
        }
      })();
      const tone: BadgeTone =
        childLinkState.status === 'linked'
          ? 'info'
          : childLinkState.status === 'embedded'
            ? 'default'
            : childLinkState.status === 'mismatch'
              ? 'warning'
              : 'error';
      const tooltip =
        childLinkState.status === 'linked'
          ? 'Linked external pipeline'
          : childLinkState.status === 'embedded'
            ? 'Inline embedded pipeline'
            : childLinkState.status === 'mismatch'
              ? 'External pipeline signature or revision mismatch'
              : 'Referenced pipeline is missing or unresolved';
      items.push({ label: statusLabel, tone, tooltip });
    }
    return items;
  })()
);
const buildPorts = (direction: 'input' | 'output') =>
  buildPortList({
    direction,
    node,
    apiNode,
    registryEntry,
    issuePortSet: new Set(direction === 'input' ? inputPortIssueIds : outputPortIssueIds),
    issuePortMessages: portIssueMessages,
    highlightedPortName,
    searchTokens,
    order: direction === 'input' ? data?.inputOrder : data?.outputOrder,
    nodeBaseId,
    syncAssignments: nodeSyncPortAssignments,
    syncModeActive: nodeSyncModeActive
  });

const inputPorts = $derived.by(() => (showDetailedNode ? buildPorts('input') : []));
const outputPorts = $derived.by(() => (showDetailedNode ? buildPorts('output') : []));
const buildCompactHandles = (direction: 'input' | 'output'): CompactPortHandle[] =>
  buildCompactPortHandleList({
    direction,
    node,
    apiNode,
    order: direction === 'input' ? data?.inputOrder : data?.outputOrder,
    nodeBaseId,
    issuePortNames: direction === 'input' ? inputPortIssueIds : outputPortIssueIds,
    highlightedPortName
  });
const compactInputHandles = $derived.by(() => (showDetailedNode ? [] : buildCompactHandles('input')));
const compactOutputHandles = $derived.by(() => (showDetailedNode ? [] : buildCompactHandles('output')));
const portSearchMatch = $derived.by(
  () =>
    showDetailedNode &&
    searchActive &&
    (inputPorts.some((port) => port.isSearchMatch) || outputPorts.some((port) => port.isSearchMatch))
);
const nodeSearchHit = $derived(searchActive && (nodeSearchMatch || portSearchMatch));
const nodeSearchDim = $derived(searchActive && !nodeSearchHit);
const showInteractivePortEditors = $derived(
  selected || nodeHighlightActive || Boolean(activeConnection)
);

function resolvePortStateClass(direction: 'input' | 'output', type: PipelineDataType): string {
  if (!activeConnection) return '';
  const expectsDirection = activeConnection.handleType === 'source' ? 'input' : 'output';
  if (direction !== expectsDirection) return '';
  const activeKey = activeConnection.typeKey;
  if (!activeKey) {
    return 'opacity-60';
  }
  const portKey = typeKey(type);
  if (!portKey) return 'opacity-40';
  if (portKey !== activeKey) {
    return 'opacity-40';
  }
  return 'ring-1 ring-primary-300/60 border-primary-300/60 text-primary-100';
}

const portHandlers = $derived.by(() =>
  showDetailedNode ? createPortHandlers({ data: data ?? undefined, node: node ?? undefined, nodeBaseId }) : null
);
</script>

<article
  class={`pipeline-node relative w-fit max-w-none overflow-visible rounded-[6px] border text-left shadow-xl shadow-surface-950/30 transition ${
    selected ? 'ring-2 ring-primary-400' : 'ring-1 ring-transparent'
  } ${nodeHasDiagnostics ? 'pipeline-node--issue' : ''} ${nodeHighlightActive ? 'pipeline-node--focus' : ''} ${
    nodeSearchHit ? 'pipeline-node--search-match' : ''
  } ${nodeSearchDim ? 'pipeline-node--search-dim' : ''}`}
  style={nodeCardStyle}
  data-heat-active={nodeHeatState.active ? 'true' : undefined}
  data-heat-performance={nodeHeatState.perfMode ? 'true' : undefined}
  data-has-issues={nodeHasDiagnostics ? 'true' : undefined}
  data-sync-mode={nodeSyncModeActive ? 'true' : undefined}
  data-sync-focus={nodeSyncFocusActive ? 'true' : undefined}
  data-gpu-overlay={gpuOverlayEnabled ? 'true' : undefined}
  data-gpu-buffered={nodeGpuBuffered ? 'true' : undefined}
  data-gpu-muted={nodeGpuMuted ? 'true' : undefined}
  data-node-kind={nodeIsGroup ? 'child' : undefined}
  data-search-active={searchActive ? 'true' : undefined}
  data-search-match={nodeSearchHit ? 'true' : undefined}
>
  {#if showDetailedNode && badges.length}
    <div class="pointer-events-none absolute right-16 top-2 flex max-w-[9rem] flex-wrap justify-end gap-[2px] text-[0.45rem] font-semibold uppercase tracking-[0.18em] pipeline-node__badge-list">
      {#each badges as badge (badge.label)}
        <span
          class="inline-block rounded-sm border px-[0.3rem] py-[1px] pipeline-node__badge"
          data-tone={badge.tone}
          title={badge.tooltip ?? badge.label}
        >
          {badge.label}
        </span>
      {/each}
    </div>
  {/if}

  <header
    class="flex flex-col gap-1 rounded-t-[6px] border-b px-3 py-2.5 pr-20"
    style={nodeHeaderStyle}
  >
    <NodeHeader
      title={nodeName}
      eyebrow={nodeIdentity ?? ''}
      className="gap-1"
      titleClassName="text-[1.05rem] font-semibold tracking-wide pipeline-node__title"
      eyebrowClassName="order-first select-none text-[0.45rem] font-semibold uppercase tracking-[0.25em] pipeline-node__identity"
      metaClassName="text-micro uppercase tracking-[0.12em]"
    >
      {#snippet meta()}
        {#if showDetailedNode && gpuOverlayEnabled && gpuBadge}
          <span
            class="gpu-badge"
            title={gpuBadge.tooltip}
            style={gpuBadgeStyle}
            data-shared={gpuBadge.shared ? 'true' : undefined}
          >
            {gpuBadge.label}
            {#if gpuBadge.shared}
              <span class="gpu-badge__dot" aria-hidden="true"></span>
              <span class="gpu-badge__count">{nodeGpuPeers}</span>
            {/if}
          </span>
        {/if}
        {#if showDetailedNode && runtimeWarning}
          <span class="runtime-warning-chip" title={runtimeWarningTitle}>
            ⚠ Runtime
          </span>
        {/if}
        {#if showDetailedNode && nodeWarningMessages.length > 0}
          <NodeStatus label={`⚠ ${nodeWarningMessages.length}`} tone="warn" title={nodeWarningMessages.join('\n')} />
        {/if}
      {/snippet}
    </NodeHeader>
    {#if showDetailedNode && summary}
      <p class="break-words text-xs font-medium leading-4 pipeline-node__summary">{summary}</p>
    {/if}
    {#if showDetailedNode && runtimeWarning}
      <p class="break-words text-[0.72rem] leading-4 text-amber-100/90">
        {runtimeWarning.message}
      </p>
    {/if}
    {#if showDetailedNode && nodeWarningMessages.length > 0}
      <p class="break-words text-[0.72rem] leading-4 text-amber-100/90">
        {nodeWarningMessages[0]}
      </p>
    {/if}
    {#if showDetailedNode && childLinkState}
      <p class="text-micro font-semibold uppercase tracking-[0.18em] text-surface-300/80">
        {#if childLinkState.alias}
          Pipeline: {childLinkState.alias}
        {:else if childLinkState.hasExternal}
          External pipeline
        {:else}
          Embedded pipeline
        {/if}
      </p>
    {/if}
    {#if showDetailedNode && nodeHeatLegend}
      <div
        class="mt-1 flex flex-wrap items-center justify-between gap-2 text-[0.58rem] uppercase tracking-[0.18em] pipeline-node__heat"
        title={nodeHeatLegend.tooltip}
      >
        <span class="text-[0.8rem] font-semibold pipeline-node__heat-value">Avg {nodeHeatLegend.averageLabel}</span>
        <span class="text-micro font-semibold pipeline-node__heat-delta" data-state={nodeHeatLegend.hasDelta ? 'active' : 'idle'}>
          Δ avg {nodeHeatLegend.deltaLabel}
        </span>
        <span class="text-[0.48rem] pipeline-node__heat-meta">
          {#if nodeHeatLegend.fpsLabel}
            {nodeHeatLegend.fpsLabel}
            <span class="mx-1 pipeline-node__heat-separator">•</span>
          {/if}
          {nodeHeatLegend.streamsLabel}
        </span>
      </div>
    {/if}
  </header>

  {#if showDetailedNode}
    <NodeActions
      isGroup={nodeIsGroup}
    />
  {/if}

  <NodePorts className="px-2 pb-2.5 pt-2" showLabels={false}>
    {#snippet inputs()}
      {#if showDetailedNode && portHandlers}
        <PipelinePortList
          direction="input"
          ports={inputPorts}
          handlers={portHandlers}
          interactive={showInteractivePortEditors}
          resolvePortStateClass={resolvePortStateClass}
          pixelColorInputAction={pixelColorInputAction}
          emptyLabel="No inputs"
        />
      {:else}
        <PipelinePortHandles
          direction="input"
          handles={compactInputHandles}
          emptyLabel="No inputs"
        />
      {/if}
    {/snippet}
    {#snippet outputs()}
      {#if showDetailedNode && portHandlers}
        <PipelinePortList
          direction="output"
          ports={outputPorts}
          handlers={portHandlers}
          interactive={showInteractivePortEditors}
          resolvePortStateClass={resolvePortStateClass}
          pixelColorInputAction={pixelColorInputAction}
          emptyLabel="No outputs"
        />
      {:else}
        <PipelinePortHandles
          direction="output"
          handles={compactOutputHandles}
          emptyLabel="No outputs"
        />
      {/if}
    {/snippet}
  </NodePorts>
</article>

<style>
  @import './PipelineNode.css';
</style>
