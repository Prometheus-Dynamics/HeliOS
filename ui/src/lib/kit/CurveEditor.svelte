<script lang="ts">
  // A small piecewise-linear curve editor (fan curve): drag points, double-
  // click empty space to add one, double-click a point to remove it.
  let {
    points,
    xmin,
    xmax,
    marker,
    onchange,
    xunit = "",
    disabled = false,
  }: { points: [number, number][]; xmin: number; xmax: number; marker?: { x: number; y: number }; onchange: (p: [number, number][]) => void; xunit?: string; disabled?: boolean } = $props();

  const W = 240;
  const H = 90;
  const px = (x: number) => ((x - xmin) / (xmax - xmin)) * W;
  const py = (y: number) => H - y * H;
  let dragging = $state<number | null>(null);
  let svg = $state<SVGSVGElement>();

  function at(event: PointerEvent | MouseEvent): [number, number] {
    const r = svg!.getBoundingClientRect();
    const x = xmin + ((event.clientX - r.left) / r.width) * (xmax - xmin);
    const y = 1 - (event.clientY - r.top) / r.height;
    return [Math.round(Math.min(xmax, Math.max(xmin, x))), Math.round(Math.min(1, Math.max(0, y)) * 100) / 100];
  }
  function move(event: PointerEvent) {
    if (dragging === null) return;
    const [x, y] = at(event);
    const next = points.map((p) => [...p] as [number, number]);
    const lo = dragging > 0 ? next[dragging - 1][0] + 1 : xmin;
    const hi = dragging < next.length - 1 ? next[dragging + 1][0] - 1 : xmax;
    next[dragging] = [Math.min(hi, Math.max(lo, x)), y];
    onchange(next);
  }
  const d = $derived(`M0,${py(points[0]?.[1] ?? 0)} ${points.map((p) => `L${px(p[0])},${py(p[1])}`).join(" ")} L${W},${py(points.at(-1)?.[1] ?? 0)}`);
</script>

<svg
  bind:this={svg}
  viewBox="-6 -6 {W + 12} {H + 12}"
  class="curve"
  class:disabled
  role="presentation"
  onpointermove={move}
  onpointerup={() => (dragging = null)}
  ondblclick={(e) => {
    if (disabled || (e.target as Element).tagName === "circle") return;
    const p = at(e);
    onchange([...points, p].sort((a, b) => a[0] - b[0]));
  }}
>
  {#each [0.25, 0.5, 0.75] as g (g)}
    <line x1="0" x2={W} y1={py(g)} y2={py(g)} class="grid" />
  {/each}
  <path d="{d} L{W},{H} L0,{H}Z" class="area" />
  <path {d} class="line" />
  {#if marker}
    <line x1={px(marker.x)} x2={px(marker.x)} y1="0" y2={H} class="mark" />
    <circle cx={px(marker.x)} cy={py(marker.y)} r="3.5" class="mark-dot" />
  {/if}
  {#each points as p, i (i)}
    <circle
      cx={px(p[0])}
      cy={py(p[1])}
      r="5"
      class="pt"
      role="slider"
      tabindex="-1"
      aria-valuenow={p[1]}
      onpointerdown={(e) => {
        if (disabled) return;
        dragging = i;
        svg?.setPointerCapture(e.pointerId);
      }}
      ondblclick={() => !disabled && points.length > 2 && onchange(points.filter((_, k) => k !== i))}
    ><title>{p[0]}{xunit} → {Math.round(p[1] * 100)}%</title></circle>
  {/each}
  <text x="0" y={H + 5} class="ax">{xmin}{xunit}</text>
  <text x={W} y={H + 5} class="ax end">{xmax}{xunit}</text>
</svg>

<style>
  .curve {
    width: 100%;
    height: auto;
    max-height: 120px;
    overflow: visible;
    user-select: none;
  }
  .disabled {
    opacity: 0.55;
  }
  .grid {
    stroke: var(--line);
  }
  .area {
    fill: color-mix(in oklab, var(--accent) 12%, transparent);
  }
  .line {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.8;
  }
  .pt {
    fill: var(--s1);
    stroke: var(--accent);
    stroke-width: 2;
    cursor: grab;
  }
  .pt:hover {
    fill: var(--accent);
  }
  .mark {
    stroke: var(--warn);
    stroke-dasharray: 3 3;
  }
  .mark-dot {
    fill: var(--warn);
  }
  .ax {
    font-size: 8px;
    fill: var(--fg-3);
    dominant-baseline: hanging;
    font-family: var(--font-code);
  }
  .ax.end {
    text-anchor: end;
  }
</style>
