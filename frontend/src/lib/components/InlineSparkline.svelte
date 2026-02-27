<script lang="ts">
  const {
    series = $bindable<number[]>([]),
    max = $bindable(100),
    colorClass = $bindable('text-primary-500')
  } = $props();
  const normalized = $derived(
    series.map((value, idx) => ({
      x: (idx / Math.max(series.length - 1, 1)) * 100,
      y: 100 - (Math.min(Math.max(value, 0), max) / max) * 100
    }))
  );
  let gradientId = $state(`inline-spark-${Math.random().toString(36).slice(2, 9)}`);
</script>

<svg
  viewBox="0 0 100 100"
  preserveAspectRatio="none"
  class={`pointer-events-none absolute inset-0 h-full w-full opacity-25 ${colorClass}`}
>
  <defs>
    <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="currentColor" stop-opacity="0.25" />
      <stop offset="100%" stop-color="currentColor" stop-opacity="0" />
    </linearGradient>
  </defs>

  <polyline
    fill={`url(#${gradientId})`}
    stroke="none"
    points={`0,100 ${normalized.map((p) => `${p.x},${p.y}`).join(' ')} 100,100`}
  />
  <polyline
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    points={normalized.map((p) => `${p.x},${p.y}`).join(' ')}
  />
</svg>
