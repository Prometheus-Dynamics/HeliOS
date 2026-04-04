<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faPlus } from '@fortawesome/free-solid-svg-icons';

  let { state }: { state: any } = $props();
</script>

{#if state.showRemoveControls && state.pipelineRemoveModalOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 px-4 py-6" role="dialog" aria-modal="true" aria-label="Remove pipeline">
    <div class="w-full max-w-md rounded border border-surface-800 bg-surface-950 p-5 shadow-2xl">
      <div class="flex items-start justify-between gap-3">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Remove pipeline</p>
          <p class="mt-2 text-sm text-surface-200">{state.resolvePipelineLabel(state.pipelineRemoveCandidateId)}</p>
          <p class="mt-2 text-xs text-surface-500">It will be detached and removed from any grid slots.</p>
        </div>
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={state.closePipelineRemoveModal}>
          Close
        </button>
      </div>
      <div class="mt-5 flex justify-end gap-2">
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={state.closePipelineRemoveModal}>
          Cancel
        </button>
        <button class="btn btn-3xs preset-filled-error-500 uppercase tracking-[0.3em]" type="button" onclick={state.confirmPipelineRemove}>
          Remove
        </button>
      </div>
    </div>
  </div>
{/if}

{#if state.showAssignControls && state.pipelineAssignModalOpen}
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/70 px-4 py-6" role="dialog" aria-modal="true" aria-label="Assign pipelines">
    <div class="w-full max-w-4xl rounded border border-surface-800 bg-surface-950 p-5 shadow-2xl">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Assign pipelines</p>
          <p class="text-xs text-surface-500">Drag assigned pipelines into the grid on the Pipelines tab.</p>
        </div>
        <div class="flex gap-2">
          <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={state.closePipelineAssignModal}>
            Close
          </button>
          <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={state.savePipelineAssignModal}>
            Save
          </button>
        </div>
      </div>

      <div class="mt-4 grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
        <input
          class="rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100"
          type="search"
          placeholder="Search pipelines…"
          bind:value={state.pipelineAssignQuery}
        />
        <button
          class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
          type="button"
          onclick={state.refreshPipelineGraphs}
        >
          Refresh
        </button>
      </div>

      <div class="mt-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div class="min-h-[16rem] rounded border border-surface-800/70 bg-surface-900/40 p-3">
          <div class="space-y-2">
            {#if state.pipelineAssignFilteredGraphs.length === 0}
              <p class="text-xs text-surface-500">No matching pipelines.</p>
            {:else}
              {#each state.pipelineAssignFilteredGraphs as graph (graph.id ?? JSON.stringify(graph))}
                {@const graphId = state.normalizeId(graph?.id)}
                {@const checked = state.pipelineAssignDraft.includes(graphId)}
                <label class="flex items-start gap-3 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2 text-sm text-surface-200">
                  <input
                    class="mt-1"
                    type="checkbox"
                    checked={checked}
                    onchange={(event) => state.toggleAssignDraft(graphId, event.currentTarget.checked)}
                  />
                  <span class="min-w-0">
                    <span class="block truncate">{graph?.name ?? graphId}</span>
                    <span class="block text-2xs uppercase tracking-[0.25em] text-surface-500">{graphId}</span>
                  </span>
                </label>
              {/each}
            {/if}
          </div>
        </div>

        <div class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Add from template</p>
          <div class="mt-2 grid gap-2 md:grid-cols-[minmax(0,1fr)_auto]">
            <select
              class="rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100"
              bind:value={state.pipelineTemplateSelectedId}
              disabled={state.pipelineTemplateLoading || state.pipelineTemplateOptions.length === 0}
            >
              {#if state.pipelineTemplateOptions.length === 0}
                <option value="">
                  {state.pipelineTemplateLoading ? 'Loading templates…' : 'No templates available'}
                </option>
              {:else}
                {#each state.pipelineTemplateOptions as template (template.templateId)}
                  <option value={template.templateId}>{template.name}</option>
                {/each}
              {/if}
            </select>
            <button
              class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]"
              type="button"
              onclick={() => void state.handleTemplateAssign()}
              disabled={state.pipelineTemplateBusy || !state.pipelineTemplateSelectedId}
            >
              <FaIcon icon={faPlus} class="mr-1.5 h-3.5 w-3.5" />
              {state.pipelineTemplateBusy ? 'Adding…' : 'Add'}
            </button>
          </div>
          {#if state.pipelineTemplateError}
            <p class="mt-2 text-xs text-error-300">{state.pipelineTemplateError}</p>
          {/if}
        </div>
      </div>

      <div class="mt-4 flex items-center justify-between text-xs text-surface-500">
        <p>{state.pipelineAssignDraft.length} assigned</p>
      </div>
    </div>
  </div>
{/if}
