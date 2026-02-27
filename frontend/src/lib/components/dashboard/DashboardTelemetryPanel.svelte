<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Panel, UsageTile, UsageTrendChart } from '$lib';
  import type { UsageTileConfig } from '../types';

type TelemetryPanelProps = {
  tiles?: UsageTileConfig[];
  cpuSeries?: number[];
  memorySeries?: number[];
  title?: string;
  eyebrow?: string;
  className?: string;
  primaryLabel?: string;
  secondaryLabel?: string;
  primaryUnit?: string;
  secondaryUnit?: string;
  primaryMax?: number;
  secondaryMax?: number;
  primaryTone?: 'primary' | 'secondary' | 'tertiary';
  secondaryTone?: 'primary' | 'secondary' | 'tertiary';
  actions?: Snippet;
  children?: Snippet;
};

const {
  tiles = [],
  cpuSeries = [],
  memorySeries = [],
  title = 'Processing load',
  eyebrow = 'Telemetry snapshot',
  className = '',
  primaryLabel = 'CPU',
  secondaryLabel = 'Memory',
  primaryUnit = '%',
  secondaryUnit = '%',
  primaryMax = 100,
  secondaryMax = 100,
  primaryTone = 'primary',
  secondaryTone = 'tertiary',
  actions,
  children
}: TelemetryPanelProps = $props();

export type $$Props = TelemetryPanelProps;
export interface $$Slots {
  default?: Record<string, never>;
  actions?: Record<string, never>;
}
</script>

<Panel className={className || 'xl:col-span-2'} tone="default" {eyebrow} {title} {actions}>
  {#if children}
    {@render children()}
  {:else}
    <div class="space-y-4">
      <div class="grid gap-3 md:grid-cols-3 xl:grid-cols-4">
        {#each tiles as tile (tile.label)}
          <UsageTile {...tile} />
        {/each}
      </div>

      <div class="rounded border border-surface-800 bg-surface-950/50 p-3">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Resource trends</p>
        <div class="mt-3">
          <UsageTrendChart
            labelA={primaryLabel}
            labelB={secondaryLabel}
            unitA={primaryUnit}
            unitB={secondaryUnit}
            maxA={primaryMax}
            maxB={secondaryMax}
            seriesA={cpuSeries}
            seriesB={memorySeries}
            toneA={primaryTone}
            toneB={secondaryTone}
          />
        </div>
      </div>
    </div>
  {/if}
</Panel>
