<script lang="ts">
  import { SummaryTiles } from '$lib';
  import type { DashboardPayload } from '$lib/types/dashboard';

  type Props = {
    isInitialLoading: boolean;
    summaryStats: DashboardPayload['summaryStats'];
  };

  const { isInitialLoading, summaryStats }: Props = $props();
</script>

{#if isInitialLoading}
  <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
    {#each Array.from({ length: 4 }).keys() as idx (idx)}
      <article class="border border-surface-800/70 bg-surface-900/40 p-4 animate-pulse">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-600">Loading</p>
        <p class="mt-1 h-8 w-16 rounded bg-surface-700/60"></p>
        <p class="mt-2 h-4 w-24 rounded bg-surface-800/60"></p>
      </article>
    {/each}
  </div>
{:else if summaryStats?.length}
  <SummaryTiles items={summaryStats} columns="grid-cols-2 lg:grid-cols-4" />
{/if}
