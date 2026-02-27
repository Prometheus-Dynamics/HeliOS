<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import ConsoleSurface from '$lib/components/ui/ConsoleSurface.svelte';
  import SectionHeader from '$lib/components/ui/SectionHeader.svelte';
  import LogToolbar from '$lib/components/logs/LogToolbar.svelte';
  import LogViewport from '$lib/components/logs/LogViewport.svelte';
  import { createLogsStore } from '$lib/features/logs/logsStore';
  import type { LogEntry, LogFilter } from '$lib/features/logs/types';

  const logsStore = createLogsStore();
  const logsState = logsStore.state;

  const logLevelFallbackColor: Record<Exclude<LogFilter, 'all'>, string> = {
    info: '#22c55e',
    warn: '#facc15',
    error: '#f87171',
    debug: '#94a3b8'
  };
  const badgeBackground = (color?: string): string | undefined =>
    color ? colorWithAlpha(color, 0.15) ?? undefined : undefined;
  const entryLevelColor = (entry: LogEntry): string => entry.levelColor ?? logLevelFallbackColor[entry.level];

  onMount(() => {
    logsStore.start();
  });

  onDestroy(() => {
    logsStore.destroy();
  });

  function handleStreamChange(value: string): void {
    logsStore.setSelectedStream(value);
  }

  function handleFilterChange(value: LogFilter): void {
    logsStore.setFilter(value);
  }

  function handleWindowChange(value: string): void {
    logsStore.setDownloadWindow(value);
  }

  function handleDownload(): void {
    void logsStore.downloadSelectedLog();
  }

  function colorWithAlpha(color: string, alpha: number): string | null {
    const parsed = parseCssColor(color);
    if (!parsed) return null;
    const [r, g, b] = parsed;
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }

  function parseCssColor(input: string): [number, number, number] | null {
    const value = input.trim().toLowerCase();
    if (!value) return null;
    if (value.startsWith('#') && (value.length === 7 || value.length === 4)) {
      if (value.length === 4) {
        const r = value[1] ?? '0';
        const g = value[2] ?? '0';
        const b = value[3] ?? '0';
        return [parseInt(r + r, 16), parseInt(g + g, 16), parseInt(b + b, 16)];
      }
      const r = parseInt(value.slice(1, 3), 16);
      const g = parseInt(value.slice(3, 5), 16);
      const b = parseInt(value.slice(5, 7), 16);
      return [r, g, b];
    }
    return null;
  }
</script>

<ConsoleSurface
  className="min-h-[clamp(16rem,45vh,32rem)] min-h-[clamp(16rem,45svh,32rem)] min-h-[clamp(16rem,45dvh,32rem)]"
  tone="contrast"
  bodyClassName="px-0 pb-0"
>
  {#snippet header()}
    <SectionHeader
      title="Log entries"
      subtitle={$logsState.logsLoading ? 'Fetching latest log lines…' : $logsState.tailing ? 'Live stream ready.' : 'Tail paused.'}
    />
  {/snippet}

  {#snippet toolbar()}
    <LogToolbar
      logStreams={$logsState.logStreams}
      selectedLogStream={$logsState.selectedLogStream}
      logFilter={$logsState.logFilter}
      logDownloadWindow={$logsState.logDownloadWindow}
      logDownloadStatus={$logsState.logDownloadStatus}
      logDownloadError={$logsState.logDownloadError}
      logDownloadBusy={$logsState.logDownloadBusy}
      onStreamChange={handleStreamChange}
      onFilterChange={handleFilterChange}
      onWindowChange={handleWindowChange}
      onDownload={handleDownload}
    />
  {/snippet}

  {#if $logsState.logStreamsError}
    <div class="border-b border-error-500/30 bg-error-500/5 px-4 py-3 text-sm text-error-300">
      {$logsState.logStreamsError}
    </div>
  {/if}
  {#if $logsState.logsError}
    <div class="border-b border-error-500/30 bg-error-500/5 px-4 py-3 text-sm text-error-300">
      {$logsState.logsError}
    </div>
  {/if}
  <LogViewport
    logs={$logsState.filteredLogs}
    loading={$logsState.logsLoading}
    entryLevelColor={entryLevelColor}
    badgeBackground={badgeBackground}
  />
</ConsoleSurface>
