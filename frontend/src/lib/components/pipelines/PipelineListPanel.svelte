<script lang="ts">
  import type { Snippet } from 'svelte';
  import { createEventDispatcher } from 'svelte';
  import { Panel } from '$lib';
  import type { PipelineListItem } from './types';

type PipelineListPanelProps = {
  pipelines?: PipelineListItem[];
  selectedPipelineId?: string | null;
  searchTerm?: string;
  emptyMessage?: string;
  newLabel?: string;
  actions?: Snippet;
  children?: Snippet;
};

let {
  pipelines = [],
  selectedPipelineId = null,
  searchTerm = $bindable(''),
  emptyMessage = 'No pipelines available.',
  newLabel = 'New',
  actions,
  children
}: PipelineListPanelProps = $props();

export type $$Props = PipelineListPanelProps;
export interface $$Slots {
  default?: Record<string, never>;
  actions?: Record<string, never>;
}

  const dispatch = createEventDispatcher<{
    select: { id: string };
    create: void;
    search: { term: string };
  }>();

  function formatRevision(revision: string | null | undefined): string | null {
    if (!revision) return null;
    return revision.slice(0, 8);
  }
</script>

{#if actions}
  <Panel title="Pipelines" tone="contrast" {actions}>
    {#if children}
      {@render children()}
    {:else}
      <div class="mt-4 space-y-2">
        {#if pipelines.length === 0}
          <p class="rounded border border-dashed border-surface-700/80 bg-surface-900/50 p-4 text-sm text-surface-400">
            {emptyMessage}
          </p>
        {:else}
          {#each pipelines as pipeline (pipeline.id)}
            <button
              type="button"
              class={`w-full rounded border px-4 py-3 text-left transition ${
                pipeline.id === selectedPipelineId
                  ? 'border-primary-500 bg-primary-500/10 text-white'
                  : 'border-surface-700 bg-surface-900/60 text-surface-200 hover:border-primary-500/40'
              }`}
              onclick={() => dispatch('select', { id: pipeline.id })}
            >
              <div class="flex items-center justify-between gap-3">
                <div>
                  <p class="text-sm font-semibold">{pipeline.name}</p>
                </div>
              </div>
              {#if pipeline.revision}
                <div class="mt-2 text-xs text-surface-500">Rev {formatRevision(pipeline.revision)}</div>
              {/if}
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </Panel>
{:else}
  <Panel title="Pipelines" tone="contrast">
    {#snippet actions()}
      <div class="flex items-center gap-2">
        <input
          class="input flex-1 text-sm"
          placeholder="Search pipelines"
          value={searchTerm}
          oninput={(event) => {
            searchTerm = event.currentTarget.value;
            dispatch('search', { term: searchTerm });
          }}
        />
        <button
          class="btn btn-xs preset-filled-primary-500 uppercase tracking-[0.3em]"
          type="button"
          onclick={() => dispatch('create')}
        >
          {newLabel}
        </button>
      </div>
    {/snippet}

    {#if children}
      {@render children()}
    {:else}
      <div class="mt-4 space-y-2">
        {#if pipelines.length === 0}
          <p class="rounded border border-dashed border-surface-700/80 bg-surface-900/50 p-4 text-sm text-surface-400">
            {emptyMessage}
          </p>
        {:else}
          {#each pipelines as pipeline (pipeline.id)}
            <button
              type="button"
              class={`w-full rounded border px-4 py-3 text-left transition ${
                pipeline.id === selectedPipelineId
                  ? 'border-primary-500 bg-primary-500/10 text-white'
                  : 'border-surface-700 bg-surface-900/60 text-surface-200 hover:border-primary-500/40'
              }`}
              onclick={() => dispatch('select', { id: pipeline.id })}
            >
              <div class="flex items-center justify-between gap-3">
                <div>
                  <p class="text-sm font-semibold">{pipeline.name}</p>
                </div>
              </div>
              {#if pipeline.revision}
                <div class="mt-2 text-xs text-surface-500">Rev {formatRevision(pipeline.revision)}</div>
              {/if}
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </Panel>
{/if}
