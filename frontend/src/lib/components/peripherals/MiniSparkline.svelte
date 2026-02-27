<script lang="ts">
  type Props = {
    label: string;
    series: number[];
    timestampsMs?: number[];
    autoScale?: boolean;
    domainMin?: number;
    domainMax?: number;
    unit?: string;
    colorClass?: string;
    color?: string;
    precision?: number;
    heightClass?: string;
  };

  const {
    label,
    series,
    timestampsMs = [],
    autoScale = true,
    domainMin,
    domainMax,
    unit = '',
    colorClass = 'text-primary-400',
    color = '',
    precision = 3,
    heightClass = 'h-24'
  }: Props = $props();

  let container: HTMLDivElement;
  let hoverIndex = $state<number | null>(null);
  let lockedDomain = $state<{ min: number; max: number } | null>(null);

  $effect(() => {
    if (autoScale) {
      lockedDomain = null;
      return;
    }
    if (lockedDomain) return;
    if (!series.length) return;
    const min = Math.min(...series);
    const max = Math.max(...series);
    lockedDomain = { min, max };
  });

  const points = $derived.by(() => {
    if (!series.length) return '';
    const minCandidate = autoScale ? Math.min(...series) : (lockedDomain?.min ?? Math.min(...series));
    const maxCandidate = autoScale ? Math.max(...series) : (lockedDomain?.max ?? Math.max(...series));
    const min = typeof domainMin === 'number' && Number.isFinite(domainMin) ? domainMin : minCandidate;
    const max = typeof domainMax === 'number' && Number.isFinite(domainMax) ? domainMax : maxCandidate;
    const span = max - min;
    const scale = span === 0 ? 1 : span;
    return series
      .map((value, idx) => {
        const x = (idx / Math.max(series.length - 1, 1)) * 100;
        const y = 100 - ((value - min) / scale) * 100;
        return `${x.toFixed(2)},${y.toFixed(2)}`;
      })
      .join(' ');
  });

  const displayIndex = $derived.by(() => {
    if (hoverIndex == null) return null;
    return Math.min(series.length - 1, Math.max(0, hoverIndex));
  });

  const displayValue = $derived.by(() => {
    const idx = displayIndex;
    const v = idx == null ? series.at(-1) : series.at(idx);
    if (typeof v !== 'number' || !Number.isFinite(v)) return '—';
    return v.toFixed(Math.max(0, precision));
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
    if (!container || series.length < 2) return;
    const rect = container.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const t = rect.width <= 0 ? 0 : x / rect.width;
    const idx = Math.round(t * Math.max(series.length - 1, 1));
    hoverIndex = Math.min(series.length - 1, Math.max(0, idx));
  }

  function handleLeave() {
    hoverIndex = null;
  }
</script>

<div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
  <div class="flex items-center justify-between gap-3">
    <p class="text-micro uppercase tracking-[0.3em] text-surface-500">{label}</p>
    <p class={`text-sm font-semibold ${color ? '' : colorClass}`} style={color ? `color:${color};` : undefined}>
      {displayValue}{unit ? ` ${unit}` : ''}
    </p>
  </div>
  <div
    class={`relative mt-2 w-full ${heightClass}`}
    bind:this={container}
    role="img"
    aria-label="Sparkline"
    onmousemove={handleMove}
    onmouseleave={handleLeave}
  >
    {#if series.length < 2}
      <div class="h-full w-full rounded bg-surface-900/40"></div>
    {:else}
      <svg
        viewBox="0 0 100 100"
        preserveAspectRatio="none"
        class={`h-full w-full ${color ? '' : colorClass}`}
        style={color ? `color:${color};` : undefined}
      >
        <polyline fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" points={points} />
      </svg>

      {#if displayIndex != null}
        <div
          class="pointer-events-none absolute inset-y-0 w-px bg-white/15"
          style={`left:${(displayIndex / Math.max(series.length - 1, 1)) * 100}%;`}
        ></div>
        <div class="pointer-events-none absolute left-2 top-2 rounded bg-black/55 px-2 py-1 text-micro text-surface-100">
          <span class="font-semibold">
            {displayValue}{unit ? ` ${unit}` : ''}
          </span>
          {#if displayTime}
            <span class="ml-2 text-surface-300">{displayTime}</span>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>
