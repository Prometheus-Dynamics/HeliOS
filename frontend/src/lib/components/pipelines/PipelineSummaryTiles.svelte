<script lang="ts">
  import type { PipelineSummaryCounts } from './types';

  const {
    summary = { total: 0, live: 0, degraded: 0, drafts: 0 },
    items
  }: {
    summary?: PipelineSummaryCounts;
    items?: Array<{ label: string; value: number; className?: string }>;
  } = $props();

  const tiles =
    items ??
    [
      { label: 'Total', value: summary.total, className: 'border-surface-700 bg-surface-900/70' },
      { label: 'Live', value: summary.live, className: 'border-emerald-700/50 bg-emerald-950/40 text-emerald-100' },
      { label: 'Degraded', value: summary.degraded, className: 'border-amber-700/50 bg-amber-950/40 text-amber-100' },
      { label: 'Drafts', value: summary.drafts, className: 'border-surface-700 bg-surface-900/70' }
    ];
</script>

<section class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
  {#each tiles as tile (tile.label)}
    <div class={`rounded border p-4 text-white ${tile.className ?? ''}`.trim()}>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">{tile.label}</p>
      <p class="mt-2 text-2xl font-semibold">{tile.value}</p>
    </div>
  {/each}
</section>
