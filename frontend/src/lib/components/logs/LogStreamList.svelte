<script lang="ts">
  import type { LogFilter, LogStreamSummary } from '$lib/features/logs/types';

  let {
    logStreams = [],
    selectedLogStream = $bindable<string | null>(null),
    logFilter = $bindable<LogFilter>('all'),
    onStreamChange = () => {},
    onFilterChange = () => {}
  }: {
    logStreams?: LogStreamSummary[];
    selectedLogStream?: string | null;
    logFilter?: LogFilter;
    onStreamChange?: (value: string) => void;
    onFilterChange?: (value: LogFilter) => void;
  } = $props();
</script>

<div class="flex flex-wrap items-center gap-3">
  <select
    class="select select-sm w-full sm:w-40 text-micro-tight uppercase tracking-[0.3em] text-surface-500"
    aria-label="Log stream"
    bind:value={selectedLogStream}
    onchange={(event) => onStreamChange((event.target as HTMLSelectElement).value)}
  >
    {#each logStreams as stream (stream.id)}
      <option value={stream.id}>{stream.label}</option>
    {/each}
  </select>
  <select
    class="select select-sm w-full sm:w-32 text-micro-tight uppercase tracking-[0.3em] text-surface-500"
    aria-label="Log level filter"
    bind:value={logFilter}
    onchange={(event) => onFilterChange((event.target as HTMLSelectElement).value as LogFilter)}
  >
    <option value="all">All</option>
    <option value="info">Info</option>
    <option value="warn">Warn</option>
    <option value="error">Error</option>
    <option value="debug">Debug</option>
  </select>
</div>
