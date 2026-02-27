<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Panel } from '$lib';
  import type { TimelineItem } from './types';

type TimelinePanelProps = {
  items?: TimelineItem[];
  title?: string;
  eyebrow?: string;
  emptyMessage?: string;
  actions?: Snippet;
  children?: Snippet;
};

const {
  items = [],
  title = 'Timeline',
  eyebrow = 'Event feed',
  emptyMessage = 'No events yet.',
  actions,
  children
}: TimelinePanelProps = $props();

export type $$Props = TimelinePanelProps;
export interface $$Slots {
  default?: Record<string, never>;
  actions?: Record<string, never>;
}
</script>

<Panel tone="contrast" {eyebrow} {title} {actions}>
  {#if children}
    {@render children()}
  {:else if items.length}
    <ol class="mt-3 space-y-3">
      {#each items as item (item.id ?? `${item.title}-${item.time}`)}
        <li class="rounded border border-surface-800 bg-surface-950/50 p-3 shadow-inner shadow-black/20">
          <div class="flex items-center justify-between gap-2">
            <span class="text-[0.7rem] uppercase tracking-[0.3em] text-surface-500">{item.time}</span>
            <span class="rounded-full border border-primary-500/40 bg-primary-500/10 px-2 py-1 text-micro uppercase tracking-[0.25em] text-primary-100">
              Event
            </span>
          </div>
          <p class="mt-1 text-base font-semibold text-surface-50">{item.title}</p>
          {#if item.detail}
            <p class="text-sm text-surface-400">{item.detail}</p>
          {/if}
        </li>
      {/each}
    </ol>
  {:else}
    <div class="mt-3 rounded border border-dashed border-surface-800/70 bg-surface-950/40 px-3 py-4 text-xs uppercase tracking-[0.3em] text-surface-500">
      {emptyMessage}
    </div>
  {/if}
</Panel>
