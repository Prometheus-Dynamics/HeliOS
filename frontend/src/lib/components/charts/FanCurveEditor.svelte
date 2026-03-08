<script lang="ts">
  export type FanCurvePoint = { temp_c: number; percent: number };

  type Props = {
    curve: FanCurvePoint[];
    tone?: string;
    tempMin?: number;
    tempMax?: number;
    currentTemp?: number | null;
    selectedIndex?: number;
    onSelect?: (index: number) => void;
    onCurveChange?: (curve: FanCurvePoint[]) => void;
    invertPercent?: boolean;
  };

  const {
    curve,
    tone = 'primary',
    tempMin = 30,
    tempMax = 95,
    currentTemp = null,
    selectedIndex = -1,
    onSelect,
    onCurveChange,
    invertPercent = false
  }: Props = $props();

  const WIDTH = 140;
  const HEIGHT = 100;
  const PADDING_LEFT = 20;
  const PADDING_RIGHT = 8;
  const PADDING_TOP = 10;
  const PADDING_BOTTOM = 18;
  const PLOT_W = WIDTH - PADDING_LEFT - PADDING_RIGHT;
  const PLOT_H = HEIGHT - PADDING_TOP - PADDING_BOTTOM;

  let svgEl = $state<SVGSVGElement | null>(null);
  let draggingIndex = $state<number | null>(null);

  const normalizedMin = 0;
  const normalizedMax = 100;

  const points = $derived(
    curve.map((point) => ({
      temp_c: clamp(Number(point.temp_c), tempMin, tempMax),
      percent: clamp(Number(point.percent), 0, 100)
    }))
  );

  const polylinePoints = $derived(points.map((p) => `${xForTemp(p.temp_c)},${yForPercent(toDisplayPercent(p.percent))}`).join(' '));
  const currentX = $derived(currentTemp == null ? null : xForTemp(clamp(currentTemp, tempMin, tempMax)));
  const currentInterpolated = $derived(currentTemp == null ? null : interpolate(points, currentTemp, normalizedMin, normalizedMax));
  const currentY = $derived(currentInterpolated == null ? null : yForPercent(toDisplayPercent(currentInterpolated)));

  function clamp(value: number, min: number, max: number): number {
    if (!Number.isFinite(value)) return min;
    return Math.min(max, Math.max(min, value));
  }

  function colorVar(t: string, shade: number): string {
    const sanitized = typeof t === 'string' && t.trim().length ? t.trim() : 'primary';
    return `var(--color-${sanitized}-${shade})`;
  }

  function xForTemp(temp: number): number {
    const ratio = (temp - tempMin) / Math.max(1e-9, tempMax - tempMin);
    return PADDING_LEFT + clamp(ratio, 0, 1) * PLOT_W;
  }

  function yForPercent(percent: number): number {
    const ratio = clamp(percent, 0, 100) / 100;
    return PADDING_TOP + (1 - ratio) * PLOT_H;
  }

  function tempForX(x: number): number {
    const ratio = (x - PADDING_LEFT) / Math.max(1e-9, PLOT_W);
    return tempMin + clamp(ratio, 0, 1) * (tempMax - tempMin);
  }

  function percentForY(y: number): number {
    const ratio = (y - PADDING_TOP) / Math.max(1e-9, PLOT_H);
    return clamp((1 - clamp(ratio, 0, 1)) * 100, 0, 100);
  }

  function toDisplayPercent(value: number): number {
    return invertPercent ? clamp(100 - value, 0, 100) : value;
  }

  function fromDisplayPercent(value: number): number {
    return invertPercent ? clamp(100 - value, 0, 100) : value;
  }

  function interpolate(points: FanCurvePoint[], temperature: number, min: number, max: number): number {
    if (!points.length || !Number.isFinite(temperature)) return min;
    const sorted = [...points].sort((a, b) => a.temp_c - b.temp_c);
    const temp = temperature;
    if (temp <= sorted[0]?.temp_c) return clamp(sorted[0]?.percent ?? min, min, max);
    if (temp >= sorted.at(-1)?.temp_c) return clamp(sorted.at(-1)?.percent ?? max, min, max);
    for (let i = 0; i < sorted.length - 1; i += 1) {
      const a = sorted[i];
      const b = sorted[i + 1];
      if (!a || !b) continue;
      if (temp < a.temp_c || temp > b.temp_c) continue;
      if (Math.abs(b.temp_c - a.temp_c) < 1e-6) continue;
      const ratio = clamp((temp - a.temp_c) / (b.temp_c - a.temp_c), 0, 1);
      const interpolated = a.percent + ratio * (b.percent - a.percent);
      return clamp(Math.round(interpolated), min, max);
    }
    return clamp(sorted.at(-1)?.percent ?? max, min, max);
  }

  function round2(value: number): number {
    if (!Number.isFinite(value)) return 0;
    return Math.round(value * 100) / 100;
  }

  function updatePoint(index: number, nextTemp: number, nextPercent: number): void {
    const safeIndex = clamp(index, 0, Math.max(0, points.length - 1));
    const prevTemp = points[safeIndex - 1]?.temp_c ?? tempMin;
    const nextTempLimit = points[safeIndex + 1]?.temp_c ?? tempMax;
    const isFirst = safeIndex === 0;
    const isLast = safeIndex === Math.max(0, points.length - 1);
    const temp = isFirst ? tempMin : isLast ? tempMax : clamp(nextTemp, prevTemp + 0.1, nextTempLimit - 0.1);
    const percent = clamp(Math.round(fromDisplayPercent(nextPercent)), 0, 100);
    const next = points.map((p, idx) => (idx === safeIndex ? { temp_c: temp, percent } : p));
    onCurveChange?.(next.map((p) => ({ temp_c: round2(p.temp_c), percent: clamp(Math.round(p.percent), 0, 100) })));
  }

  function pointerToPlot(event: PointerEvent): { x: number; y: number } | null {
    if (!svgEl) return null;
    const ctm = svgEl.getScreenCTM();
    if (!ctm) return null;
    const pt = new DOMPoint(event.clientX, event.clientY);
    const local = pt.matrixTransform(ctm.inverse());
    return { x: clamp(local.x, 0, WIDTH), y: clamp(local.y, 0, HEIGHT) };
  }

  function handlePointerMove(event: PointerEvent): void {
    if (draggingIndex == null) return;
    const plot = pointerToPlot(event);
    if (!plot) return;
    const nextTemp = tempForX(plot.x);
    const nextPercent = percentForY(plot.y);
    updatePoint(draggingIndex, nextTemp, nextPercent);
  }

  function stopDrag(): void {
    draggingIndex = null;
  }
</script>

<div class="space-y-2 rounded border border-surface-800 bg-surface-950/30 p-3">
  <div class="flex items-center justify-between gap-3">
    <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Fan curve</p>
    <p class="text-xs text-surface-500">{tempMin}°C–{tempMax}°C</p>
  </div>

  <div class="relative w-full overflow-hidden rounded border border-surface-800/80 bg-surface-950/60" style="aspect-ratio: 16 / 9;">
    <svg
      bind:this={svgEl}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      preserveAspectRatio="xMidYMid meet"
      class="h-full w-full"
      role="application"
      aria-label="Fan curve editor"
      onpointermove={handlePointerMove}
      onpointerup={stopDrag}
      onpointercancel={stopDrag}
      onpointerleave={stopDrag}
    >
      <text x="6" y={PADDING_TOP + 4} font-size="4.2" fill="rgba(148,163,184,0.75)">%</text>
      <text x={WIDTH - 10} y={HEIGHT - 6} font-size="4.2" fill="rgba(148,163,184,0.75)">°C</text>

      {#each [0, 25, 50, 75, 100] as pct (pct)}
        <line
          x1={PADDING_LEFT}
          y1={yForPercent(pct)}
          x2={WIDTH - PADDING_RIGHT}
          y2={yForPercent(pct)}
          stroke="rgba(148,163,184,0.10)"
          stroke-width="1"
        />
        <text x={PADDING_LEFT - 4} y={yForPercent(pct) + 1.6} text-anchor="end" font-size="4" fill="rgba(148,163,184,0.75)">
          {pct}
        </text>
      {/each}
      {#each [tempMin, (tempMin * 2 + tempMax) / 3, (tempMin + tempMax * 2) / 3, tempMax] as temp (temp)}
        <line
          x1={xForTemp(temp)}
          y1={PADDING_TOP}
          x2={xForTemp(temp)}
          y2={HEIGHT - PADDING_BOTTOM}
          stroke="rgba(148,163,184,0.10)"
          stroke-width="1"
        />
        <text x={xForTemp(temp)} y={HEIGHT - 6} text-anchor="middle" font-size="4" fill="rgba(148,163,184,0.75)">
          {Math.round(temp)}
        </text>
      {/each}

      <rect
        x={PADDING_LEFT}
        y={PADDING_TOP}
        width={PLOT_W}
        height={PLOT_H}
        fill="none"
        stroke="rgba(148,163,184,0.18)"
        stroke-width="1.2"
      />

      {#if currentX != null}
        <line
          x1={currentX}
          y1={PADDING_TOP}
          x2={currentX}
          y2={HEIGHT - PADDING_BOTTOM}
          stroke={colorVar('primary', 300)}
          stroke-width="1.2"
          opacity="0.35"
        />
      {/if}
      {#if currentX != null && currentY != null}
        <circle cx={currentX} cy={currentY} r="2.4" fill={colorVar('primary', 200)} opacity="0.95" />
      {/if}

      <polyline fill="none" stroke={colorVar(tone, 300)} stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" points={polylinePoints} />

      {#each points as point, idx (idx)}
        <circle
          cx={xForTemp(point.temp_c)}
          cy={yForPercent(toDisplayPercent(point.percent))}
          r={idx === selectedIndex ? 3.6 : 3.1}
          fill={idx === selectedIndex ? colorVar(tone, 200) : 'rgba(226,232,240,0.9)'}
          stroke={idx === selectedIndex ? colorVar(tone, 400) : 'rgba(30,41,59,0.8)'}
          stroke-width="1.5"
          class="cursor-grab active:cursor-grabbing"
          role="button"
          tabindex="0"
          onpointerdown={(event) => {
            event.preventDefault();
            (event.currentTarget as SVGCircleElement).setPointerCapture(event.pointerId);
            draggingIndex = idx;
            onSelect?.(idx);
          }}
          onclick={() => onSelect?.(idx)}
          onkeydown={(event) => {
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault();
              onSelect?.(idx);
            }
          }}
        />
      {/each}
    </svg>
  </div>

  {#if currentTemp != null && currentInterpolated != null}
    <p class="text-xs text-surface-500">At {currentTemp.toFixed(1)}°C → {currentInterpolated}%</p>
  {:else}
    <p class="text-xs text-surface-600">Select a point to edit, drag to adjust.</p>
  {/if}
</div>
