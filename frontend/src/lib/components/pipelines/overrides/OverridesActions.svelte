<script lang="ts">
  import type { PipelineUiTab } from '$lib/features/pipelines/pipelineUiTypes';

  type BindingWarning = {
    id: string;
    label: string;
    reason: string;
  };

  type Props = {
    streamError: string | null;
    editMode: boolean;
    bindingWarnings: BindingWarning[];
    tabs: ReadonlyArray<PipelineUiTab> | null;
    isSearching: boolean;
    activeTabId: string | null;
    onTabSelect: (id: string) => void;
  };

  const {
    streamError,
    editMode,
    bindingWarnings,
    tabs,
    isSearching,
    activeTabId,
    onTabSelect
  }: Props = $props();
</script>

<div class="space-y-3">
  {#if streamError}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
      {streamError}
    </div>
  {/if}

  {#if editMode && bindingWarnings.length > 0}
    <div class="rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-100">
      <p class="text-micro-tight uppercase tracking-[0.3em] text-amber-200">Missing bindings</p>
      <div class="mt-2 max-h-28 space-y-1 overflow-auto pr-1">
        {#each bindingWarnings as warning (warning.id)}
          <div class="flex items-center justify-between gap-2 text-[0.7rem] text-amber-100">
            <span class="truncate">{warning.label}</span>
            <span class="shrink-0 text-micro-tight uppercase tracking-[0.2em] text-amber-200">{warning.reason}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if tabs && !isSearching}
    <div class="flex flex-wrap gap-2">
      {#each tabs as tab (tab.id)}
        <button
          class={`inline-flex h-7 items-center justify-center rounded border px-3 text-micro-tight uppercase tracking-[0.3em] transition ${
            activeTabId === tab.id
              ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
              : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
          }`}
          type="button"
          onclick={() => onTabSelect(tab.id)}
        >
          {tab.title}
        </button>
      {/each}
    </div>
  {/if}
</div>
