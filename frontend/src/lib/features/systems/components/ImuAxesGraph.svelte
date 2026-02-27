<script lang="ts">
  import type { ImuAxes } from '$lib/types/systems';

  let {
    title = 'Axes',
    unit = '',
    series = [] as ImuAxes[],
    timestampsMs = [] as number[],
    autoScale = true
  } = $props();

  const colors = {
    x: 'var(--axis-x)',
    y: 'var(--axis-y)',
    z: 'var(--axis-z)'
  };

  type Point = { x: number; y: number };
  type NormalizedAxes = { x: Point[]; y: Point[]; z: Point[]; maxAbs: number };

  let lockedMaxAbs = $state<number | null>(null);
  const normalized = $derived.by(() => normalizeSeries(series, autoScale ? undefined : lockedMaxAbs ?? undefined));
  const latest = $derived(series.at(-1) ?? { x: 0, y: 0, z: 0 });
  const hasSeries = $derived(series.length > 0);
  let hoverIndex = $state<number | null>(null);
  const gridLines = [-50, -25, 0, 25, 50];

  $effect(() => {
    if (autoScale) {
      lockedMaxAbs = null;
    } else if (lockedMaxAbs == null) {
      lockedMaxAbs = normalizeSeries(series).maxAbs;
    }
  });

  function normalizeSeries(values: ImuAxes[], fixedMaxAbs?: number): NormalizedAxes {
    const input = values.length ? values : [{ x: 0, y: 0, z: 0 }];
    const maxAbs =
      typeof fixedMaxAbs === 'number' && Number.isFinite(fixedMaxAbs) && fixedMaxAbs > 0
        ? fixedMaxAbs
        : Math.max(1, ...input.flatMap((axes) => [Math.abs(axes.x), Math.abs(axes.y), Math.abs(axes.z)]));
    const amplitude = 44;
    return {
      x: input.map((axes, idx) => point(idx, input.length, axes.x, maxAbs, amplitude)),
      y: input.map((axes, idx) => point(idx, input.length, axes.y, maxAbs, amplitude)),
      z: input.map((axes, idx) => point(idx, input.length, axes.z, maxAbs, amplitude)),
      maxAbs
    };
  }

  function point(idx: number, length: number, value: number, scale: number, amplitude: number) {
    const x = (idx / Math.max(length - 1, 1)) * 100;
    const y = clamp(50 - (value / scale) * amplitude, 4, 96);
    return { x, y };
  }

  function clamp(value: number, min: number, max: number): number {
    return Math.min(max, Math.max(min, value));
  }

  function formatValue(value: number): string {
    if (!Number.isFinite(value)) return '0';
    if (Math.abs(value) >= 100) return value.toFixed(0);
    if (Math.abs(value) >= 10) return value.toFixed(1);
    return value.toFixed(2);
  }

  const displayIndex = $derived.by(() => {
    if (hoverIndex == null) return null;
    return Math.min(series.length - 1, Math.max(0, hoverIndex));
  });

  const displayAxes = $derived.by(() => {
    const idx = displayIndex;
    return idx == null ? null : (series[idx] ?? null);
  });

  const displayTime = $derived.by(() => {
    const idx = displayIndex;
    if (idx == null) return null;
    const ms = timestampsMs?.[idx];
    if (typeof ms !== 'number' || !Number.isFinite(ms)) return null;
    const date = new Date(ms);
    if (Number.isNaN(date.getTime())) return null;
    return date.toLocaleTimeString();
  });

  function handleMove(event: MouseEvent) {
    const target = event.currentTarget as HTMLElement | null;
    if (!target || series.length < 2) return;
    const rect = target.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const t = rect.width <= 0 ? 0 : x / rect.width;
    const idx = Math.round(t * Math.max(series.length - 1, 1));
    hoverIndex = Math.min(series.length - 1, Math.max(0, idx));
  }

  function handleLeave() {
    hoverIndex = null;
  }
</script>

<div class="space-y-2 rounded border border-surface-800 bg-surface-950/30 p-3">
  <div class="flex items-center justify-between gap-3">
    <div>
      <p class="text-micro uppercase tracking-[0.35em] text-surface-500">IMU</p>
      <p class="text-sm font-semibold text-surface-100">{title}</p>
    </div>
    <div class="flex items-center gap-3 text-xs font-semibold text-surface-300">
      <div class="flex items-center gap-1">
        <span class="h-2 w-2 rounded-full" style={`background:${colors.x};`}></span>
        <span class="text-surface-500">X</span>
        <span style={`color:${colors.x};`}>{formatValue(latest.x)}{unit}</span>
      </div>
      <div class="flex items-center gap-1">
        <span class="h-2 w-2 rounded-full" style={`background:${colors.y};`}></span>
        <span class="text-surface-500">Y</span>
        <span style={`color:${colors.y};`}>{formatValue(latest.y)}{unit}</span>
      </div>
      <div class="flex items-center gap-1">
        <span class="h-2 w-2 rounded-full" style={`background:${colors.z};`}></span>
        <span class="text-surface-500">Z</span>
        <span style={`color:${colors.z};`}>{formatValue(latest.z)}{unit}</span>
      </div>
    </div>
  </div>

  <div
    class="relative h-40 w-full overflow-hidden rounded border border-surface-800/80 bg-surface-950/60"
    role="img"
    aria-label="IMU axes graph"
    onmousemove={handleMove}
    onmouseleave={handleLeave}
  >
    <svg viewBox="0 0 100 100" preserveAspectRatio="none" class="h-full w-full">
      {#each gridLines as grid (grid)}
        <line
          x1="0"
          x2="100"
          y1={50 - grid}
          y2={50 - grid}
          stroke="rgba(255,255,255,0.06)"
          stroke-width="0.4"
        />
      {/each}

      <polyline
        fill="none"
        stroke={colors.x}
        stroke-width="1.6"
        stroke-linecap="round"
        points={normalized.x.map((p) => `${p.x},${p.y}`).join(' ')}
      />
      <polyline
        fill="none"
        stroke={colors.y}
        stroke-width="1.6"
        stroke-linecap="round"
        points={normalized.y.map((p) => `${p.x},${p.y}`).join(' ')}
      />
      <polyline
        fill="none"
        stroke={colors.z}
        stroke-width="1.6"
        stroke-linecap="round"
        points={normalized.z.map((p) => `${p.x},${p.y}`).join(' ')}
      />

      {#if hasSeries}
        <circle cx={normalized.x.at(-1)?.x} cy={normalized.x.at(-1)?.y} r="2" fill={colors.x} />
        <circle cx={normalized.y.at(-1)?.x} cy={normalized.y.at(-1)?.y} r="2" fill={colors.y} />
        <circle cx={normalized.z.at(-1)?.x} cy={normalized.z.at(-1)?.y} r="2" fill={colors.z} />
      {/if}
    </svg>

    {#if displayIndex != null && displayAxes}
      <div
        class="pointer-events-none absolute inset-y-0 w-px bg-white/10"
        style={`left:${(displayIndex / Math.max(series.length - 1, 1)) * 100}%;`}
      ></div>
      <div class="pointer-events-none absolute left-2 top-2 rounded bg-black/55 px-2 py-1 text-micro text-surface-100">
        <span class="mr-2 text-surface-300">X</span><span class="font-semibold">{formatValue(displayAxes.x)}{unit}</span>
        <span class="mx-2 text-surface-300">Y</span><span class="font-semibold">{formatValue(displayAxes.y)}{unit}</span>
        <span class="mx-2 text-surface-300">Z</span><span class="font-semibold">{formatValue(displayAxes.z)}{unit}</span>
        {#if displayTime}
          <span class="ml-2 text-surface-300">{displayTime}</span>
        {/if}
      </div>
    {/if}

    {#if !hasSeries}
      <div class="pointer-events-none absolute inset-0 flex items-center justify-center bg-surface-950/70 text-micro uppercase tracking-[0.3em] text-surface-500">
        Awaiting samples
      </div>
    {/if}

    <div class="pointer-events-none absolute bottom-2 left-2 rounded bg-black/40 px-2 py-1 text-micro-tight uppercase tracking-[0.2em] text-surface-500">
      Range ±{formatValue(normalized.maxAbs)}{unit || ''}
    </div>
  </div>
</div>
