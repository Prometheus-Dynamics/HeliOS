<script lang="ts">
  // The last few seconds of some of a device's channels, as lines on one scale (auto-fitted to
  // what is shown). Drawn from the rows directly: a row costs nothing to keep, and the path is
  // thinned to about one point a pixel column.
  import type { LiveDevice } from "./feed.ts";

  let {
    device,
    channels,
    colors,
    tick = 0,
    seconds = 10,
    width = 240,
    height = 46,
  }: { device: LiveDevice; channels: number[]; colors: string[]; tick?: number; seconds?: number; width?: number; height?: number } = $props();

  const paths = $derived.by(() => {
    void tick;
    if (!device.count) return [] as { d: string; color: string; last: number }[];
    const newest = device.newestUs();
    const from = newest - seconds * 1_000_000;
    const first = device.firstFrom(from);
    const span = Math.max(1, newest - from);
    const step = Math.max(1, Math.floor((device.count - first) / width));
    const rows: number[] = [];
    for (let i = first; i < device.count; i += step) rows.push(device.index(i));
    const last = device.index(device.count - 1);
    rows.push(last);
    const lows: number[] = [];
    const highs: number[] = [];
    for (const channel of channels) {
      let low = Infinity;
      let high = -Infinity;
      for (const at of rows) {
        const value = device.v[channel][at];
        if (Number.isFinite(value)) {
          low = Math.min(low, value);
          high = Math.max(high, value);
        }
      }
      lows.push(low);
      highs.push(high);
    }
    // One scale for all the channels, so their sizes compare.
    const low = Math.min(...lows.filter(Number.isFinite));
    const high = Math.max(...highs.filter(Number.isFinite));
    const range = high - low > 1e-9 ? high - low : 1;
    return channels.map((channel, c) => {
      let d = "";
      for (const at of rows) {
        const value = device.v[channel][at];
        if (!Number.isFinite(value)) continue;
        const x = ((device.t[at] - from) / span) * width;
        const y = height - 3 - ((value - low) / range) * (height - 6);
        d += `${d ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`;
      }
      const latest = device.v[channel][last];
      return { d, color: colors[c % colors.length], last: Number.isFinite(latest) ? latest : Number.NaN };
    });
  });
</script>

<svg class="trace" viewBox="0 0 {width} {height}" preserveAspectRatio="none" aria-hidden="true">
  <line x1="0" x2={width} y1={height - 0.5} y2={height - 0.5} class="base" />
  {#each paths as line, i (i)}
    {#if line.d}<path d={line.d} fill="none" style="stroke: {line.color}" stroke-width="1.3" vector-effect="non-scaling-stroke" stroke-linejoin="round" />{/if}
  {/each}
</svg>

<style>
  .trace {
    display: block;
    width: 100%;
    height: 46px;
    overflow: visible;
  }
  .base {
    stroke: var(--line);
    stroke-width: 1;
  }
</style>
