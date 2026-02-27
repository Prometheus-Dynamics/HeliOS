<script lang="ts">
  import type { PipelineDetailContext } from '$lib';
  import type { PipelineNodeMetricMap, PipelineNodeRuntimeMetrics } from '$lib/types/pipeline';

  const {
    context,
    onRefresh
  }: {
    context: PipelineDetailContext;
    onRefresh?: () => void;
  } = $props();

  const formatTimestamp = (timestamp: number | null | undefined): string => {
    if (!timestamp) return '—';
    const date = new Date(timestamp * 1000);
    return date.toLocaleString();
  };

  const formatRelativeTime = (timestamp: number | null | undefined): string => {
    if (!timestamp) return '—';
    const now = Date.now();
    const diffMs = now - timestamp * 1000;
    if (diffMs < 0) return 'just now';
    const minutes = Math.floor(diffMs / 60000);
    const hours = Math.floor(diffMs / 3600000);
    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes}m ago`;
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  };

  const formatAverageTime = (value: number | undefined): string => {
    if (value == null || Number.isNaN(value)) return '—';
    return `${value.toFixed(2)} ms`;
  };

  const formatAverageFps = (value: number | undefined): string => {
    if (value == null || Number.isNaN(value)) return '—';
    return `${value.toFixed(1)} fps`;
  };

  const formatCount = (value: number | null | undefined): string => {
    if (value == null || Number.isNaN(value)) return '—';
    return Math.round(value).toLocaleString();
  };

  const formatBytes = (value: number | null | undefined): string => {
    if (value == null || Number.isNaN(value)) return '—';
    if (value < 1024) return `${value} B`;
    if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
    return `${(value / (1024 * 1024)).toFixed(1)} MB`;
  };

  const pipeline = context.pipeline;
  const METRICS_ROW_HEIGHT = 28;
  const METRICS_OVERSCAN = 8;
  let metricsScroll = $state<Record<string, number>>({});
  let metricsViewport = $state<Record<string, number>>({});

  function metricsViewportObserver(node: HTMLDivElement, streamId: string) {
    const update = () => {
      metricsViewport = { ...metricsViewport, [streamId]: node.clientHeight };
    };
    update();
    const observer = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(update) : null;
    if (observer) observer.observe(node);
    return {
      destroy() {
        observer?.disconnect();
      }
    };
  }

  function handleMetricsScroll(streamId: string, event: Event): void {
    const node = event.currentTarget as HTMLDivElement | null;
    if (!node) return;
    metricsScroll = { ...metricsScroll, [streamId]: node.scrollTop };
  }

  const countWarnings = (metrics: PipelineNodeMetricMap | null | undefined): number => {
    if (!metrics) return 0;
    let total = 0;
    Object.values(metrics).forEach((entry) => {
      if (!entry) return;
      if (entry.lastError) total += 1;
      if (entry.children) total += countWarnings(entry.children);
    });
    return total;
  };

  const flattenGroupMetrics = (
    groups: PipelineNodeMetricMap | null | undefined,
    depth = 0,
  ): Array<{ id: string; metrics: PipelineNodeRuntimeMetrics; depth: number }> => {
    if (!groups) return [];
    const rows: Array<{ id: string; metrics: PipelineNodeRuntimeMetrics; depth: number }> = [];
    Object.entries(groups).forEach(([id, metrics]) => {
      rows.push({ id, metrics, depth });
      if (metrics.children) {
        rows.push(...flattenGroupMetrics(metrics.children, depth + 1));
      }
    });
    return rows;
  };
</script>

{#if !pipeline}
  <section class="flex h-full min-h-0 items-center justify-center rounded border border-surface-800/80 bg-surface-950/80 p-6 text-center text-surface-400">
    Select a pipeline to view runtime metrics.
  </section>
{:else}
  <div class="flex h-full min-h-0 flex-col rounded border border-surface-800/80 bg-surface-950/80 p-5 text-sm text-surface-200">
    <header class="flex items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.35em] text-surface-500">Metrics</p>
        <h2 class="text-lg font-semibold text-white">{pipeline.name}</h2>
        <p class="text-xs text-surface-400">
          Status: {context.metricsStatus}
          {#if context.metricsUpdatedAt}
            · Updated {formatTimestamp(context.metricsUpdatedAt)}
          {/if}
        </p>
      </div>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={() => onRefresh?.()}>
        Refresh
      </button>
    </header>

    {#if !context.metrics || context.metrics.length === 0}
      <p class="mt-4 text-xs text-surface-500">No metrics available. Attach the pipeline to a stream to collect runtime data.</p>
    {:else}
      <div class="mt-4 min-h-0 flex-1 overflow-y-auto pr-1">
        <div class="space-y-4 text-xs">
          {#each context.metrics as entry (entry.streamId)}
            {@const warningCount = countWarnings(entry.metrics) + countWarnings(entry.groups)}
            <div class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
              <header class="flex flex-wrap items-center justify-between gap-2">
                <p class="text-sm font-semibold text-white">{entry.streamId}</p>
                <div class="flex items-center gap-2 text-micro uppercase tracking-[0.3em] text-surface-500">
                  <span>{entry.metrics ? Object.keys(entry.metrics).length : 0} nodes</span>
                  {#if warningCount > 0}
                    <span class="rounded-full border border-amber-400/60 bg-amber-500/10 px-2 py-[2px] text-amber-100">
                      {warningCount} warning{warningCount === 1 ? '' : 's'}
                    </span>
                  {/if}
                </div>
              </header>
              {#if entry.perf || entry.flamegraph}
                <div class="mt-2 flex flex-wrap gap-3 text-micro text-surface-500">
                  {#if entry.perf}
                    <span>cache misses avg: {formatCount(entry.perf.averageCacheMisses)}</span>
                    <span>branch instr avg: {formatCount(entry.perf.averageBranchInstructions)}</span>
                    <span>branch misses avg: {formatCount(entry.perf.averageBranchMisses)}</span>
                  {/if}
                  {#if entry.flamegraph}
                    <span>flamegraph: {formatBytes(entry.flamegraph.sizeBytes)}</span>
                    <span class="truncate" title={entry.flamegraph.path}>path: {entry.flamegraph.path}</span>
                  {/if}
                </div>
              {/if}
              {#if entry.metrics}
                {@const metricsRows = Object.entries(entry.metrics ?? {})}
                {@const scrollTop = metricsScroll[entry.streamId] ?? 0}
                {@const viewportHeight = metricsViewport[entry.streamId] ?? 240}
                {@const totalHeight = metricsRows.length * METRICS_ROW_HEIGHT}
                {@const startIndex = Math.max(0, Math.floor(scrollTop / METRICS_ROW_HEIGHT) - METRICS_OVERSCAN)}
                {@const endIndex = Math.min(metricsRows.length, Math.ceil((scrollTop + viewportHeight) / METRICS_ROW_HEIGHT) + METRICS_OVERSCAN)}
                {@const slice = metricsRows.slice(startIndex, endIndex)}
                {@const offset = startIndex * METRICS_ROW_HEIGHT}
                <div class="mt-2">
                  <div class="overflow-x-auto">
                    <div class="min-w-[44rem]">
                      <div class="grid grid-cols-[minmax(160px,2fr)_minmax(100px,1fr)_minmax(100px,1fr)_minmax(90px,1fr)_minmax(200px,2fr)] text-micro text-surface-500">
                        <div class="text-left font-semibold">Node</div>
                        <div class="text-right font-semibold">Avg Time</div>
                        <div class="text-right font-semibold">Avg FPS</div>
                        <div class="text-right font-semibold">Samples</div>
                        <div class="text-left font-semibold">Last Warning</div>
                      </div>
                      <div
                        class="mt-2 max-h-64 overflow-y-auto overflow-x-hidden"
                        use:metricsViewportObserver={entry.streamId}
                        onscroll={(event) => handleMetricsScroll(entry.streamId, event)}
                      >
                        <div class="relative" style={`height: ${totalHeight}px;`}>
                          <div class="absolute left-0 right-0" style={`transform: translateY(${offset}px);`}>
                            {#each slice as [nodeId, metrics]}
                              {@const perf = metrics?.metrics}
                              {@const warning = metrics?.lastError ?? null}
                              {@const warningAt = metrics?.lastErrorAt ?? null}
                              <div class="grid grid-cols-[minmax(160px,2fr)_minmax(100px,1fr)_minmax(100px,1fr)_minmax(90px,1fr)_minmax(200px,2fr)] py-1 text-micro text-surface-300">
                                <div class="text-left truncate">{nodeId}</div>
                                <div class="text-right">{formatAverageTime(perf?.averageTimeMs)}</div>
                                <div class="text-right">{formatAverageFps(perf?.averageFps)}</div>
                                <div class="text-right">{perf?.sampleCount ?? '—'}</div>
                                <div class="text-left">
                                  {#if warning}
                                    <span class="rounded border border-amber-400/40 bg-amber-500/10 px-2 py-[2px] text-amber-100" title={warning}>
                                      {formatRelativeTime(warningAt)} · {warning.length > 72 ? `${warning.slice(0, 72)}…` : warning}
                                    </span>
                                  {:else}
                                    <span class="text-surface-500">—</span>
                                  {/if}
                                </div>
                              </div>
                            {/each}
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              {/if}

              {#if entry.groups && Object.keys(entry.groups).length > 0}
                {@const groupRows = flattenGroupMetrics(entry.groups)}
                <div class="mt-4">
                  <div class="overflow-x-auto">
                    <div class="min-w-[44rem]">
                      <div class="grid grid-cols-[minmax(160px,2fr)_minmax(100px,1fr)_minmax(100px,1fr)_minmax(90px,1fr)_minmax(200px,2fr)] text-micro text-surface-500">
                        <div class="text-left font-semibold">Group / Node</div>
                        <div class="text-right font-semibold">Avg Time</div>
                        <div class="text-right font-semibold">Avg FPS</div>
                        <div class="text-right font-semibold">Samples</div>
                        <div class="text-left font-semibold">Last Warning</div>
                      </div>
                      <div class="mt-2 space-y-1">
                        {#each groupRows as row (row.id + '-' + row.depth)}
                          {@const perf = row.metrics?.metrics}
                          {@const warning = row.metrics?.lastError ?? null}
                          {@const warningAt = row.metrics?.lastErrorAt ?? null}
                          <div class="grid grid-cols-[minmax(160px,2fr)_minmax(100px,1fr)_minmax(100px,1fr)_minmax(90px,1fr)_minmax(200px,2fr)] items-center gap-2">
                            <div class="flex items-center gap-2 text-left text-surface-200">
                              <span style={`padding-left: ${row.depth * 12}px;`}>{row.id}</span>
                              {#if row.metrics?.children}
                                <span class="rounded-full border border-surface-700 px-2 py-[1px] text-micro-tight uppercase tracking-[0.2em] text-surface-400">
                                  Group
                                </span>
                              {/if}
                            </div>
                            <div class="text-right">{formatAverageTime(perf?.averageTimeMs)}</div>
                            <div class="text-right">{formatAverageFps(perf?.averageFps)}</div>
                            <div class="text-right">{perf?.sampleCount ?? 0}</div>
                            <div class="truncate text-left text-micro text-surface-500" title={warning ?? ''}>
                              {warning ? `${warning} · ${formatRelativeTime(warningAt)}` : '—'}
                            </div>
                          </div>
                        {/each}
                      </div>
                    </div>
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
{/if}
