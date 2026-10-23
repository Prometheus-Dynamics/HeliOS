<script lang="ts">
  // The status ring as it is: its LEDs in a circle, each in the colour the board shows now
  // (lemnosd's frames, as streamed), logical LED 0 at the top running clockwise.
  import type { LiveDevice } from "./feed.ts";
  import { ringFrame, type RingFrame } from "./ring.ts";

  let { light, size = 72, tick = 0 }: { light: LiveDevice; size?: number; tick?: number } = $props();

  let cache: RingFrame | null = null;
  const frame = $derived.by(() => {
    void tick;
    cache = ringFrame(light, cache);
    return cache;
  });
  const colors = $derived(frame?.colors ?? []);
  const n = $derived(colors.length);
  const lit = $derived(colors.filter((c) => c).length);
  const css = (rgb: number) => (rgb ? `#${rgb.toString(16).padStart(6, "0")}` : "#20262e");
</script>

<figure class="ring">
  <svg viewBox="-50 -50 100 100" width={size} height={size} role="img" aria-label="Status ring, {lit} of {n} LEDs lit">
    <circle r="38" fill="none" stroke="var(--inset)" stroke-width="14" />
    {#each colors as rgb, i (i)}
      {@const a = (i / n) * 2 * Math.PI - Math.PI / 2}
      <circle cx={Math.cos(a) * 38} cy={Math.sin(a) * 38} r={Math.max(2.5, Math.min(6, 110 / n))} fill={css(rgb)} stroke="#0f1115" stroke-width="1" />
    {/each}
    {#if !frame}<text y="4" text-anchor="middle" font-size="11" fill="var(--fg-3)">no frames</text>{/if}
  </svg>
  <figcaption>
    <b>Status ring</b>
    <span class="mono">{lit}/{n} lit{frame?.brightness != null ? ` · ${Math.round(frame.brightness * 100)}%` : ""}</span>
  </figcaption>
</figure>

<style>
  .ring {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    margin: 0;
  }
  figcaption {
    display: flex;
    flex-direction: column;
    align-items: center;
    font-size: 11px;
    color: var(--fg-3);
  }
  figcaption b {
    font-weight: 600;
    color: var(--fg-2);
  }
  .mono {
    font-family: var(--font-code);
  }
</style>
