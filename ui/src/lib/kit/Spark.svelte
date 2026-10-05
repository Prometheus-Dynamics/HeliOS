<script lang="ts">
  // A tiny inline line chart.
  let { values, color = "var(--accent)", width = 60, height = 16, max }: { values: number[]; color?: string; width?: number; height?: number; max?: number } = $props();
  const top = $derived(max ?? Math.max(...values, 1e-9));
  const d = $derived(values.length > 1 ? values.map((v, i) => `${i ? "L" : "M"}${((i / (values.length - 1)) * width).toFixed(1)},${(height - 1 - (Math.min(v, top) / top) * (height - 2)).toFixed(1)}`).join("") : "");
</script>

<svg {width} {height} viewBox="0 0 {width} {height}" aria-hidden="true" class="spark">
  {#if d}
    <path d="{d}L{width},{height}L0,{height}Z" fill={color} opacity="0.14" />
    <path {d} fill="none" stroke={color} stroke-width="1.3" stroke-linejoin="round" />
  {/if}
</svg>

<style>
  .spark {
    flex-shrink: 0;
    display: block;
  }
</style>
