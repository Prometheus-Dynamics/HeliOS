<script lang="ts">
  import type { Snippet } from 'svelte';

  type PipelineTab = 'pipeline' | 'tune';

  type PipelineInspectorPanelProps = {
    activeTab: PipelineTab;
    onSelectTab?: (tab: PipelineTab) => void;
    toolbar?: Snippet;
    pipeline?: Snippet;
    tune?: Snippet;
  };

  let {
    activeTab,
    onSelectTab = () => {},
    toolbar,
    pipeline,
    tune
  }: PipelineInspectorPanelProps = $props();

  const tabBaseClasses =
    'px-2.5 py-1.5 text-[0.68rem] font-semibold uppercase tracking-[0.13em] border-b-2 border-transparent transition rounded-none';
  const tabActiveClasses = 'text-primary-100 border-primary-400';
  const tabInactiveClasses = 'text-surface-300 hover:text-primary-100 hover:border-primary-300';

  export type $$Props = PipelineInspectorPanelProps;
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3">
  <div class="flex flex-wrap items-center gap-1.5 border-b border-surface-800/70 pb-1.5">
    <div class="flex flex-wrap items-center gap-1.5">
      <button
        class={`${tabBaseClasses} ${activeTab === 'tune' ? tabActiveClasses : tabInactiveClasses}`}
        type="button"
        onclick={() => onSelectTab('tune')}
      >
        Tune
      </button>
      <button
        class={`${tabBaseClasses} ${activeTab === 'pipeline' ? tabActiveClasses : tabInactiveClasses}`}
        type="button"
        onclick={() => onSelectTab('pipeline')}
      >
        Graph
      </button>
    </div>
    <div class="ml-auto flex items-center gap-1.5">
      {@render toolbar?.()}
    </div>
  </div>

  {#if activeTab === 'pipeline'}
    {@render pipeline?.()}
  {:else}
    {@render tune?.()}
  {/if}
</div>
