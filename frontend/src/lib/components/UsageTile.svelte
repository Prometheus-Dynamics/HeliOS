<script lang="ts">
  import InlineSparkline from '$lib/components/InlineSparkline.svelte';

  const {
    label = $bindable('Usage'),
    value = $bindable(0 as unknown as number | string),
    trend = $bindable('stable'),
    valueClass = $bindable('text-primary-200'),
    sparkClass = $bindable('text-primary-500'),
    series = $bindable<number[]>([]),
    max = $bindable(100),
    unit = $bindable('%'),
    precision = $bindable(0),
    tooltip = $bindable<string | undefined>(undefined),
    warning = $bindable(false),
    warningLabel = $bindable('Thermal throttle')
  } = $props();

  const formattedValue = $derived(() => {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return value.toFixed(Math.max(0, precision));
    }
    return value;
  });
</script>

<div
  class={`relative overflow-hidden rounded border p-3 ${
    warning ? 'border-amber-400/70 bg-amber-500/5' : 'border-surface-800/80'
  }`}
  title={tooltip}
>
  <p class="relative z-10 text-xs uppercase tracking-[0.3em] text-surface-500">{label}</p>
  {#if warning}
    <span class="absolute right-3 top-3 text-micro-tight uppercase tracking-[0.35em] text-amber-300">
      {warningLabel}
    </span>
  {/if}
  <p class={`relative z-10 flex items-baseline gap-1 text-3xl font-semibold ${valueClass}`}>
    <span>{formattedValue()}</span>
    {#if unit}
      <span class="text-sm tracking-[0.3em] text-surface-400">{unit}</span>
    {/if}
  </p>
  <p class="relative z-10 text-xs text-surface-500">{trend}</p>
  <InlineSparkline {series} {max} colorClass={sparkClass} />
</div>
