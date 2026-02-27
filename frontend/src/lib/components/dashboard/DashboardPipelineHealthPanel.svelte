<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Panel } from '$lib';

  export type PipelineHealthRow = {
    id: string;
    name: string;
    state: string;
    fps: number;
    latency: string;
    stateClass?: string;
    barClass?: string;
  };

type PipelineHealthPanelProps = {
  pipelines?: PipelineHealthRow[];
  title?: string;
  eyebrow?: string;
  emptyMessage?: string;
  actions?: Snippet;
  children?: Snippet;
};

const {
  pipelines = [],
  title = 'Live graph health',
  eyebrow = 'Pipeline watch',
  emptyMessage = 'No active pipelines',
  actions,
  children
}: PipelineHealthPanelProps = $props();

export type $$Props = PipelineHealthPanelProps;
export interface $$Slots {
  default?: Record<string, never>;
  actions?: Record<string, never>;
}
</script>

<Panel tone="subtle" {eyebrow} {title} {actions}>
  {#if children}
    {@render children()}
  {:else}
    <div class="mt-4 space-y-3 text-sm">
      {#if pipelines.length === 0}
        <div class="rounded border border-dashed border-surface-700/70 bg-surface-950/40 px-3 py-4 text-xs uppercase tracking-[0.3em] text-surface-500">
          {emptyMessage}
        </div>
      {:else}
        <div class="grid gap-3">
          {#each pipelines as pipeline (pipeline.id)}
            <article class="rounded border border-surface-800 bg-surface-950/50 p-3 shadow shadow-black/20">
              <div class="flex items-start justify-between gap-3">
                <div>
                  <p class="text-base font-semibold text-surface-50">{pipeline.name}</p>
                  <p class="text-xs text-surface-500">Latency {pipeline.latency}</p>
                </div>
                <div class="flex flex-col items-end gap-1 text-right">
                  <span class={`rounded-full border px-2 py-1 text-[0.7rem] uppercase tracking-[0.3em] ${pipeline.stateClass ?? ''}`}>
                    {pipeline.state}
                  </span>
                  <span class="text-xs text-surface-500">{pipeline.fps ?? 0} fps</span>
                </div>
              </div>
              <div class="mt-3 h-1.5 rounded bg-surface-800">
                <div
                  class={`h-full rounded ${pipeline.barClass ?? ''}`}
                  style={`width:${Math.min(100, Math.max(0, pipeline.fps ?? 0))}%`}
                ></div>
              </div>
            </article>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</Panel>
