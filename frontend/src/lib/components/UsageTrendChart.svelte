<script lang="ts">
  const {
    labelA = $bindable('Throughput'),
    labelB = $bindable('Latency'),
    unitA = $bindable('%'),
    unitB = $bindable('ms'),
    seriesA = $bindable<number[]>([]),
    seriesB = $bindable<number[]>([]),
    maxA = $bindable(100),
    maxB = $bindable(40),
    toneA = $bindable('primary'),
    toneB = $bindable('secondary')
  } = $props();

  function colorVar(tone: string, shade: number): string {
    const sanitized = typeof tone === 'string' && tone.trim().length ? tone.trim() : 'primary';
    return `var(--color-${sanitized}-${shade})`;
  }

  const normalize = (series: number[], max: number) =>
    series.map((value, idx) => ({
      x: (idx / Math.max(series.length - 1, 1)) * 100,
      y: 100 - (Math.min(Math.max(value, 0), max) / max) * 100,
      value
    }));

  const aPoints = $derived(normalize(seriesA, maxA));
  const bPoints = $derived(normalize(seriesB, maxB));
  const hasSeries = $derived(seriesA.length > 0 || seriesB.length > 0);
  const gridLines = [20, 40, 60, 80];

  let hoverIndex = $state<number | null>(null);
  let container: HTMLDivElement | null = $state(null);

  const maxSeriesLength = $derived(() => Math.max(seriesA.length, seriesB.length));

  const activeIndex = $derived(() => {
    const fallback = maxSeriesLength() - 1;
    if (fallback < 0) return 0;
    const idx = hoverIndex ?? fallback;
    return Math.min(fallback, Math.max(0, idx));
  });

  const activeA = $derived(() => seriesA[activeIndex()] ?? seriesA.at(-1) ?? 0);
  const activeB = $derived(() => seriesB[activeIndex()] ?? seriesB.at(-1) ?? 0);
  type GuidePoint = { x: number; y: number };
  type HoverDetails = { x: number; throughput: number | null; latency: number | null };
  let hoverGuideValue = $state<GuidePoint | null>(null);
  let hoverDetailsValue = $state<HoverDetails | null>(null);
  let gradientId = $state(`trend-${Math.random().toString(36).slice(2, 8)}`);

  function syncHoverMeta(idx: number | null) {
    if (idx === null) {
      hoverGuideValue = null;
      hoverDetailsValue = null;
      return;
    }
    const guidePoint = aPoints[idx] ?? bPoints[idx] ?? null;
    if (!guidePoint) {
      hoverGuideValue = null;
      hoverDetailsValue = null;
      return;
    }
    hoverGuideValue = guidePoint;
    hoverDetailsValue = {
      x: guidePoint.x,
      throughput: seriesA[idx] ?? null,
      latency: seriesB[idx] ?? null
    };
  }

  function handlePointerMove(event: PointerEvent) {
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const ratio = (event.clientX - rect.left) / rect.width;
    const length = Math.max(1, maxSeriesLength());
    const idx = Math.round(ratio * (length - 1));
    hoverIndex = Number.isFinite(idx) ? Math.max(0, Math.min(idx, length - 1)) : null;
    syncHoverMeta(hoverIndex);
  }

  function clearHover() {
    hoverIndex = null;
    syncHoverMeta(null);
  }
</script>

<div
  class="relative h-56 w-full overflow-hidden border border-surface-700/60 bg-surface-950"
  bind:this={container}
  role="img"
  aria-label="Usage trend chart"
  onpointermove={handlePointerMove}
  onpointerleave={clearHover}
  onpointercancel={clearHover}
>
  <svg viewBox="0 0 100 100" preserveAspectRatio="none" class="h-full w-full">
    <defs>
      <linearGradient id={`${gradientId}-a`} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color={colorVar(toneA, 500)} stop-opacity="0.35" />
        <stop offset="100%" stop-color={colorVar(toneA, 500)} stop-opacity="0" />
      </linearGradient>
      <linearGradient id={`${gradientId}-b`} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color={colorVar(toneB, 500)} stop-opacity="0.35" />
        <stop offset="100%" stop-color={colorVar(toneB, 500)} stop-opacity="0" />
      </linearGradient>
    </defs>

    {#each gridLines as line (line)}
      <line
        x1="0"
        x2="100"
        y1={line}
        y2={line}
        stroke="rgba(255,255,255,0.06)"
        stroke-width="0.4"
      />
    {/each}

    <polyline
      fill={`url(#${gradientId}-b)`}
      stroke="none"
      points={`0,100 ${bPoints.map((p) => `${p.x},${p.y}`).join(' ')} 100,100`}
    />
    <polyline
      fill={`url(#${gradientId}-a)`}
      stroke="none"
      points={`0,100 ${aPoints.map((p) => `${p.x},${p.y}`).join(' ')} 100,100`}
    />
    <polyline
      fill="none"
      stroke={colorVar(toneB, 300)}
      stroke-width="1.4"
      stroke-linecap="round"
      points={bPoints.map((p) => `${p.x},${p.y}`).join(' ')}
    />
    <polyline
      fill="none"
      stroke={colorVar(toneA, 300)}
      stroke-width="1.6"
      stroke-linecap="round"
      points={aPoints.map((p) => `${p.x},${p.y}`).join(' ')}
    />

    {#if aPoints.length}
      <circle
        cx={aPoints[aPoints.length - 1].x}
        cy={aPoints[aPoints.length - 1].y}
        r="2"
        fill={colorVar(toneA, 200)}
      />
    {/if}
    {#if bPoints.length}
      <circle
        cx={bPoints[bPoints.length - 1].x}
        cy={bPoints[bPoints.length - 1].y}
        r="2"
        fill={colorVar(toneB, 200)}
      />
    {/if}
  </svg>

  <div class="pointer-events-none absolute inset-0 flex flex-col justify-between p-3 text-micro font-semibold tracking-[0.3em] text-surface-500">
    <div class="flex gap-4">
      <div class="flex items-center gap-2">
        <span class="inline-block h-1 w-6 rounded" style={`background:${colorVar(toneA, 300)};`}></span>
        <span>{labelA}</span>
        <span style={`color:${colorVar(toneA, 200)};`}>{activeA().toFixed(0)}{unitA}</span>
      </div>
      <div class="flex items-center gap-2">
        <span class="inline-block h-1 w-6 rounded" style={`background:${colorVar(toneB, 300)};`}></span>
        <span>{labelB}</span>
        <span style={`color:${colorVar(toneB, 200)};`}>{activeB().toFixed(0)}{unitB}</span>
      </div>
    </div>
    <div class="flex justify-between text-micro-tight uppercase text-surface-600">
      <span>History</span>
      <span>Now</span>
    </div>
  </div>

  {#if hoverGuideValue}
    <div
      class="pointer-events-none absolute inset-y-0"
      style={`left:${hoverGuideValue.x}%;`}
    >
      <div class="h-full border-l" style={`border-color:${colorVar(toneA, 500)};opacity:0.3;`}></div>
    </div>
  {/if}

  {#if hoverDetailsValue}
    <div
      class="pointer-events-none absolute top-2 rounded border border-surface-700/80 bg-black/70 px-2 py-1 text-micro-tight uppercase tracking-[0.3em]"
      style={`left:${hoverDetailsValue.x}%;transform:translateX(-50%);`}
    >
      <div>{hoverDetailsValue.throughput?.toFixed(0) ?? '--'}{unitA}</div>
      <div style={`color:${colorVar(toneB, 200)};`}>{hoverDetailsValue.latency?.toFixed(0) ?? '--'}{unitB}</div>
    </div>
  {/if}

  {#if !hasSeries}
    <div class="pointer-events-none absolute inset-0 flex items-center justify-center bg-surface-950/60 text-micro uppercase tracking-[0.3em] text-surface-500">
      Awaiting telemetry
    </div>
  {/if}
</div>
