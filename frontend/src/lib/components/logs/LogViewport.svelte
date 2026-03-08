<script lang="ts">
  import type { LogEntry } from '$lib/features/logs/types';
  import { onDestroy, onMount } from 'svelte';

  type Props = {
    logs: LogEntry[];
    loading: boolean;
    entryLevelColor: (entry: LogEntry) => string;
    badgeBackground: (color?: string) => string | undefined;
    emptyMessage?: string;
    loadingMessage?: string;
  };

  const {
    logs,
    loading,
    entryLevelColor,
    badgeBackground,
    emptyMessage = 'No entries for the selected filter.',
    loadingMessage = 'Fetching latest log lines…'
  }: Props = $props();

  const LOG_ROW_ESTIMATE_PX = 72;
  const LOG_OVERSCAN = 6;

  let logsViewport = $state<HTMLDivElement | null>(null);
  let logScrollTop = $state(0);
  let logViewportHeight = $state(0);
  let logViewportObserver: ResizeObserver | null = null;

  const useVirtualLogs = $derived(logs.length > 200);
  const logStartIndex = $derived.by(() =>
    useVirtualLogs ? Math.max(0, Math.floor(logScrollTop / LOG_ROW_ESTIMATE_PX) - LOG_OVERSCAN) : 0
  );
  const logEndIndex = $derived.by(() => {
    if (!useVirtualLogs) return logs.length;
    const estimatedVisible = Math.ceil(logViewportHeight / LOG_ROW_ESTIMATE_PX) + LOG_OVERSCAN;
    return Math.min(logs.length, logStartIndex + estimatedVisible + LOG_OVERSCAN);
  });
  const virtualLogs = $derived.by(() => (useVirtualLogs ? logs.slice(logStartIndex, logEndIndex) : logs));
  const logTopSpacer = $derived.by(() => (useVirtualLogs ? logStartIndex * LOG_ROW_ESTIMATE_PX : 0));
  const logBottomSpacer = $derived.by(() =>
    useVirtualLogs ? Math.max(0, (logs.length - logEndIndex) * LOG_ROW_ESTIMATE_PX) : 0
  );

  function updateLogViewport(): void {
    if (!logsViewport) return;
    logViewportHeight = logsViewport.clientHeight;
  }

  function handleLogScroll(event: Event): void {
    const target = event.currentTarget as HTMLDivElement;
    logScrollTop = target.scrollTop;
  }

  onMount(() => {
    if (typeof ResizeObserver !== 'undefined') {
      logViewportObserver = new ResizeObserver(() => updateLogViewport());
      if (logsViewport) {
        logViewportObserver.observe(logsViewport);
      }
    }
    updateLogViewport();
  });

  onDestroy(() => {
    if (logViewportObserver && logsViewport) {
      logViewportObserver.unobserve(logsViewport);
    }
    logViewportObserver = null;
  });
</script>

{#if loading && logs.length === 0}
  <div class="flex flex-1 items-center justify-center px-4 py-6 text-sm text-surface-500">
    {loadingMessage}
  </div>
{:else if logs.length === 0}
  <div class="flex flex-1 items-center justify-center px-4 py-6 text-sm text-surface-500">
    {emptyMessage}
  </div>
{:else}
  <div class="flex-1 overflow-y-auto lg:max-h-[65vh] lg:max-h-[65svh] lg:max-h-[65dvh]" bind:this={logsViewport} onscroll={handleLogScroll}>
    {#if logTopSpacer > 0}
      <div style={`height: ${logTopSpacer}px;`}></div>
    {/if}
    {#each virtualLogs as log (log.id)}
      <article class="border-b border-surface-900/40 px-4 py-3 last:border-b-0">
        <div class="flex flex-wrap items-center gap-3 text-xs uppercase tracking-[0.3em] text-surface-500">
          <span
            class="rounded-full border px-2 py-0.5 font-semibold"
            style:color={entryLevelColor(log)}
            style:border-color={entryLevelColor(log)}
            style:background-color={badgeBackground(entryLevelColor(log))}
          >
            {log.level}
          </span>
          {#if log.threadId}
            <span class="rounded-full border border-surface-800/70 px-2 py-0.5 font-semibold text-surface-300">
              Thread {log.threadId}
            </span>
          {/if}
          <span>{log.inlineTimestamp ?? log.timestamp ?? '—'}</span>
          {#if log.fileId}
            <span class="text-surface-400">· {log.fileId}</span>
          {/if}
        </div>
        <p class="mt-2 break-words whitespace-pre-wrap font-mono text-sm leading-relaxed text-surface-100">
          {log.renderedMessage || log.message}
        </p>
      </article>
    {/each}
    {#if logBottomSpacer > 0}
      <div style={`height: ${logBottomSpacer}px;`}></div>
    {/if}
  </div>
{/if}
