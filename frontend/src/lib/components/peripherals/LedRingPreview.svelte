<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';

  type Props = {
    count: number;
    colors: string[];
    whites?: number[];
    brightness?: number;
    brightnesses?: number[];
    indexOffset?: number;
    selected?: number;
    selectedIndices?: number[];
    onSelect?: (index: number) => void;
    interactive?: boolean;
    hideHeader?: boolean;
    showSwatches?: boolean;
    svgClass?: string;
    swatchSizeClass?: string;
  };

  const {
    count,
    colors,
    whites = [],
    brightness = 255,
    brightnesses = [],
    indexOffset = 0,
    selected = -1,
    selectedIndices = [],
    onSelect,
    interactive = true,
    hideHeader = false,
    showSwatches = true,
    svgClass = 'h-56 w-full select-none',
    swatchSizeClass = 'h-6 w-6'
  }: Props = $props();

  type Dot = {
    index: number;
    cx: number;
    cy: number;
    fill: string;
    stroke: string;
    selected: boolean;
  };

  const selectedLookup = $derived(new SvelteSet(selectedIndices));
  const dots = $derived(buildDots());

  function clamp(value: number, min: number, max: number): number {
    if (!Number.isFinite(value)) return min;
    return Math.min(max, Math.max(min, value));
  }

  function hexToRgb(hex: string): { r: number; g: number; b: number } {
    const sanitized = hex?.startsWith('#') ? hex.slice(1) : hex;
    if (!sanitized || sanitized.length < 6) {
      return { r: 0, g: 0, b: 0 };
    }
    return {
      r: parseInt(sanitized.slice(0, 2), 16),
      g: parseInt(sanitized.slice(2, 4), 16),
      b: parseInt(sanitized.slice(4, 6), 16)
    };
  }

  function rgbToHex(rgb: { r: number; g: number; b: number }): string {
    const toHex = (v: number) => clamp(Math.round(v), 0, 255).toString(16).padStart(2, '0');
    return `#${toHex(rgb.r)}${toHex(rgb.g)}${toHex(rgb.b)}`;
  }

  function previewColor(hex: string, white: number, intensity: number): string {
    const base = hexToRgb(hex);
    const w = clamp(white, 0, 255);
    const scale = clamp(intensity, 0, 255) / 255;
    return rgbToHex({
      r: (base.r + w) * scale,
      g: (base.g + w) * scale,
      b: (base.b + w) * scale
    });
  }

  function buildDots(): Dot[] {
    const safeCount = Math.max(1, Math.min(128, Math.trunc(count)));
    const radius = 40;
    const center = 50;
    const startAngle = -Math.PI / 2;
    const step = (Math.PI * 2) / safeCount;
    const offset = ((Math.trunc(indexOffset) % safeCount) + safeCount) % safeCount;
    return Array.from({ length: safeCount }, (_, positionIndex) => {
      const index = (positionIndex + offset) % safeCount;
      const angle = startAngle + positionIndex * step;
      const cx = center + Math.cos(angle) * radius;
      const cy = center + Math.sin(angle) * radius;
      const intensity = clamp(brightnesses[index] ?? brightness, 0, 255);
      const fill = previewColor(colors[index] ?? '#000000', whites[index] ?? 0, intensity);
      const isSelected = selectedLookup.has(index) || index === selected;
      return {
        index,
        cx,
        cy,
        fill,
        stroke: isSelected ? 'var(--color-primary-300)' : 'rgba(148,163,184,0.35)',
        selected: isSelected
      };
    });
  }
</script>

<div class="space-y-2">
  {#if !hideHeader}
    <div class="flex items-center justify-between gap-3">
      <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Preview</p>
      <p class="text-xs text-surface-500">{Math.max(1, Math.trunc(count))} LEDs</p>
    </div>
  {/if}

  <div class="rounded border border-surface-800 bg-surface-950/30 p-3">
    <svg viewBox="0 0 100 100" class={svgClass}>
      <circle cx="50" cy="50" r="44" fill="none" stroke="rgba(148,163,184,0.15)" stroke-width="2" />
      {#each dots as dot (dot.index)}
        <g>
          <circle
            cx={dot.cx}
            cy={dot.cy}
            r={dot.selected ? 5.2 : 4.6}
            fill={dot.fill}
            stroke={dot.stroke}
            stroke-width="2"
            class={interactive ? 'cursor-pointer' : 'cursor-default'}
            role={interactive ? 'button' : undefined}
            aria-label={interactive ? `Select LED ${dot.index + 1}` : undefined}
            onpointerdown={(event) => {
              event.preventDefault();
              if (interactive) {
                onSelect?.(dot.index);
              }
            }}
          />
          <text x={dot.cx} y={dot.cy + 1.5} text-anchor="middle" font-size="3.2" fill="rgba(15,23,42,0.9)" class="pointer-events-none">
            {dot.index + 1}
          </text>
        </g>
      {/each}
    </svg>

    {#if showSwatches}
      <div class="mt-2 flex flex-wrap gap-1">
        {#each Array.from({ length: Math.max(1, Math.trunc(count)) }, (_, ledIndex) => ledIndex) as ledIndex (ledIndex)}
          <button
            type="button"
            class={`${swatchSizeClass} rounded border ${selectedLookup.has(ledIndex) || ledIndex === selected ? 'border-primary-400 ring-2 ring-primary-500/30' : 'border-surface-800'} `}
            style={`background:${previewColor(colors[ledIndex] ?? '#000000', whites[ledIndex] ?? 0, brightnesses[ledIndex] ?? brightness)};`}
            aria-label={`Select LED ${ledIndex + 1}`}
            disabled={!interactive}
            onclick={() => onSelect?.(ledIndex)}
          ></button>
        {/each}
      </div>
    {/if}
  </div>
</div>
