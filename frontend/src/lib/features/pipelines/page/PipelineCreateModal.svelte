<script lang="ts">
  import type { Writable } from 'svelte/store';
  import type { PipelineOverviewPipeline, PipelineTemplateSummary } from '$lib/types/pipeline';

  type PipelineCreateModalProps = {
    open: boolean;
    createMode: Writable<'blank' | 'existing' | 'import' | 'template'>;
    createName: Writable<string>;
    createSourcePipelineId: Writable<string | null>;
    createSourceTemplateId: Writable<string | null>;
    createBusy?: boolean;
    createError?: string | null;
    pipelines: PipelineOverviewPipeline[];
    templates: PipelineTemplateSummary[];
    pipelineForSource: () => PipelineOverviewPipeline | null;
    templateForSource: () => PipelineTemplateSummary | null;
    onClose?: () => void;
    onCreate?: () => void;
    onTriggerImport?: () => void;
    onFileChange?: (event: Event) => void;
    importInput?: HTMLInputElement | null;
  };

  let {
    open,
    createMode,
    createName,
    createSourcePipelineId,
    createSourceTemplateId,
    createBusy = false,
    createError = null,
    pipelines,
    templates,
    pipelineForSource,
    templateForSource,
    onClose = () => {},
    onCreate = () => {},
    onTriggerImport = () => {},
    onFileChange = () => {},
    importInput = $bindable(null)
  }: PipelineCreateModalProps = $props();

  const sourceSelectValue = $derived.by(() => {
    if ($createMode === 'template') {
      const templateId = $createSourceTemplateId;
      return templateId ? `template:${templateId}` : 'blank';
    }
    if ($createMode === 'existing') {
      const pipelineId = $createSourcePipelineId;
      return pipelineId ? `existing:${pipelineId}` : 'blank';
    }
    return 'blank';
  });

  const handleSourceChange = (event: Event) => {
    const value = (event.currentTarget as HTMLSelectElement).value;
    if (value === 'blank') {
      createMode.set('blank');
      return;
    }
    if (value.startsWith('template:')) {
      const templateId = value.replace('template:', '').trim();
      if (!templateId) {
        createMode.set('blank');
        return;
      }
      createMode.set('template');
      createSourceTemplateId.set(templateId);
      return;
    }
    if (value.startsWith('existing:')) {
      const pipelineId = value.replace('existing:', '').trim();
      if (!pipelineId) {
        createMode.set('blank');
        return;
      }
      createMode.set('existing');
      createSourcePipelineId.set(pipelineId);
    }
  };

  $effect(() => {
    if ($createMode === 'template') {
      if (!templates.length) {
        createMode.set('blank');
        return;
      }
      const templateId = $createSourceTemplateId;
      if (!templateId || !templates.some((template) => template.templateId === templateId)) {
        createSourceTemplateId.set(templates[0]?.templateId ?? null);
      }
      return;
    }
    if ($createMode === 'existing') {
      if (!pipelines.length) {
        createMode.set('blank');
        return;
      }
      const pipelineId = $createSourcePipelineId;
      if (!pipelineId || !pipelines.some((pipeline) => pipeline.id === pipelineId)) {
        createSourcePipelineId.set(pipelines[0]?.id ?? null);
      }
    }
  });

  export type $$Props = PipelineCreateModalProps;
</script>

{#if open}
  <div class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"></div>
  <div class="fixed left-1/2 top-24 z-50 w-full max-w-xl -translate-x-1/2 rounded border border-surface-700 bg-surface-950/95 p-6 shadow-2xl">
    <div class="flex items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Create pipeline</p>
        <h2 class="text-lg font-semibold text-white">New pipeline</h2>
      </div>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose}>
        Close
      </button>
    </div>
    <div class="mt-4 space-y-4">
      <label class="flex flex-col gap-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Name</span>
        <input
          class="input h-9"
          placeholder="Optional (uses graph name)"
          value={$createName}
          oninput={(event) => createName.set((event.currentTarget as HTMLInputElement).value)}
        />
      </label>
      <div class="space-y-2">
        <label class="flex flex-col gap-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Start from</span>
          <select
            class="rounded border border-surface-800 bg-surface-900/60 px-3 py-2 text-sm text-surface-100"
            value={sourceSelectValue}
            onchange={handleSourceChange}
          >
            <optgroup label="Blank">
              <option value="blank">Blank graph</option>
            </optgroup>
            <optgroup label="Templates">
              {#if templates.length === 0}
                <option value="template:" disabled>No templates available</option>
              {:else}
                {#each templates as template (template.templateId)}
                  <option value={`template:${template.templateId}`}>{template.name}</option>
                {/each}
              {/if}
            </optgroup>
            <optgroup label="Existing">
              {#if pipelines.length === 0}
                <option value="existing:" disabled>No pipelines available</option>
              {:else}
                {#each pipelines as pipeline (pipeline.id)}
                  <option value={`existing:${pipeline.id}`}>{pipeline.name}</option>
                {/each}
              {/if}
            </optgroup>
          </select>
        </label>
        {#if $createMode === 'template'}
          {#if templateForSource()?.summary}
            <p class="text-xs text-surface-500">{templateForSource()?.summary}</p>
          {:else if templates.length === 0}
            <p class="text-xs text-surface-500">No pipeline templates are available on this device.</p>
          {/if}
        {:else if $createMode === 'existing'}
          {#if pipelineForSource()}
            <p class="text-xs text-surface-500">Cloning from {pipelineForSource()?.name}</p>
          {:else if pipelines.length === 0}
            <p class="text-xs text-surface-500">No pipelines to clone yet.</p>
          {/if}
        {:else}
          <p class="text-xs text-surface-500">Start with an empty graph and build from scratch.</p>
        {/if}
      </div>
      {#if createError}
        <p class="text-xs text-error-300">{createError}</p>
      {/if}
    </div>
    <div class="mt-6 flex items-center justify-between gap-3">
      <div class="flex items-center gap-2">
        <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onTriggerImport}>
          Import JSON
        </button>
        <input
          class="hidden"
          type="file"
          accept="application/json"
          bind:this={importInput}
          onchange={onFileChange}
        />
      </div>
      <div class="flex items-center gap-3">
        <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose} disabled={createBusy}>
          Cancel
        </button>
        <button
          class="btn btn-2xs preset-filled-primary-500 uppercase tracking-[0.3em]"
          type="button"
          onclick={onCreate}
          disabled={createBusy || ($createMode === 'existing' && pipelines.length === 0) || ($createMode === 'template' && templates.length === 0)}
        >
          {createBusy ? 'Creating…' : 'Create'}
        </button>
      </div>
    </div>
  </div>
{/if}
