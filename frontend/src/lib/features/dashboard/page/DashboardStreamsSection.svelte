<script lang="ts">
  import { DashboardStreamsPanel } from '$lib';
  import type { StreamPreviewItem } from '$lib/components/dashboard/DashboardStreamsPanel.svelte';

  type StreamSummary = { live: number; degraded: number; offline: number; total: number };

  type Props = {
    streams: StreamPreviewItem[];
    summary: StreamSummary;
  };

  const { streams, summary }: Props = $props();
</script>

<div class="min-h-[50vh] min-h-[50svh] min-h-[50dvh]">
  <DashboardStreamsPanel streams={streams} title="Streams" eyebrow="Capture readiness">
    {#snippet actions()}
      <span class="text-xs uppercase tracking-[0.3em] text-surface-500">
        {summary.live} live · {summary.degraded} degraded · {summary.offline} offline · {summary.total} total
      </span>
    {/snippet}
  </DashboardStreamsPanel>
</div>
