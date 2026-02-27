<script lang="ts">
  interface Segment {
    label: string;
    value: number;
    color?: string;
    detail?: string;
  }

  const {
    segments = $bindable<Segment[]>([]),
    total = $bindable(0)
  } = $props();

  const formatted = $derived(
    segments.map((segment) => ({
      ...segment,
      percentage: total ? (segment.value / total) * 100 : 0
    }))
  );
  const hasSegments = $derived(formatted.some((segment) => segment.percentage > 0));
</script>

<div class="space-y-3 text-xs">
  {#if hasSegments}
    <div class="flex h-10 w-full overflow-hidden border border-surface-700/60 bg-surface-900">
      {#each formatted as segment (segment.label)}
        <div
          class="group relative flex items-center justify-center text-micro-tight uppercase tracking-[0.3em] text-white/80"
          style={`width:${segment.percentage}%;background:${segment.color ?? 'var(--color-primary-500)'}`}
        >
          {segment.percentage.toFixed(0)}%
          <div class="pointer-events-none absolute bottom-full mb-2 hidden rounded bg-black/80 px-2 py-1 text-micro-tight tracking-widest group-hover:block">
            {segment.label}: {segment.percentage.toFixed(1)}%
            {#if segment.detail}
              <span class="block text-micro-tight text-surface-300">{segment.detail}</span>
            {/if}
          </div>
        </div>
      {/each}
    </div>

    <ul class="grid gap-2 sm:grid-cols-2">
      {#each formatted as segment (segment.label)}
        <li class="flex items-center gap-2 text-surface-400">
          <span
            class="inline-block h-2 w-8 rounded"
            style={`background:${segment.color ?? 'var(--color-primary-500)'}`}
          ></span>
          <div class="flex flex-col">
            <span class="font-semibold text-surface-100">{segment.label}</span>
            <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">
              {segment.percentage.toFixed(1)}%
              {#if segment.detail}
                · {segment.detail}
              {/if}
            </span>
          </div>
        </li>
      {/each}
    </ul>
  {:else}
    <div class="flex h-24 items-center justify-center rounded border border-dashed border-surface-700/60 bg-surface-900/30 text-micro uppercase tracking-[0.3em] text-surface-500">
      No segment data yet
    </div>
  {/if}
</div>
