<script lang="ts">
  // A thin horizontal bar for a 0..1 value, coloured by thresholds.
  let { value, warn = 0.75, crit = 0.9, color, height = 4 }: { value: number; warn?: number; crit?: number; color?: string; height?: number } = $props();
  const fill = $derived(color ?? (value >= crit ? "var(--err)" : value >= warn ? "var(--warn)" : "var(--ok)"));
</script>

<span class="meter" style:height="{height}px"><span style:width="{Math.min(1, Math.max(0, value)) * 100}%" style:background={fill}></span></span>

<style>
  .meter {
    display: block;
    width: 100%;
    background: var(--line);
    border-radius: var(--r-1);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    border-radius: var(--r-1);
  }
</style>
