<script lang="ts">
  import { buildTypeOptions, describePortType } from '$lib/features/pipelines/inspector/inspectorTypeUtils';
  import type { PipelineDataType, PipelineOverviewPipeline, PipelineTypeDescriptor } from '$lib/types/pipeline';

  type IoNodeContext = {
    nodeId: string;
    kind: 'input' | 'output';
    ports: Array<{ name: string; dataType: PipelineDataType }>;
  };

  type ChildLinkState = {
    status: 'embedded' | 'linked' | 'mismatch' | 'unresolved';
    alias: string | null;
    targetId: string | null;
    hasExternal: boolean;
    hasEmbedded: boolean;
  };

  const {
    ioNodeContext = null,
    childLinkState = null,
    pipelines = [],
    pipelineId = null,
    graphSelectionNodeId = null,
    typePalette = {},
    onAddIoPort,
    onRemoveIoPort,
    onRelinkExternal,
    onOpenExternal
  }: {
    ioNodeContext?: IoNodeContext | null;
    childLinkState?: ChildLinkState | null;
    pipelines?: PipelineOverviewPipeline[];
    pipelineId?: string | null;
    graphSelectionNodeId?: string | null;
    typePalette?: Record<string, PipelineTypeDescriptor>;
    onAddIoPort?: (payload: { nodeId: string; name: string; dataTypeKey: string }) => void;
    onRemoveIoPort?: (payload: { nodeId: string; name: string }) => void;
    onRelinkExternal?: (payload: { nodeId: string; pipelineId: string }) => Promise<void> | void;
    onOpenExternal?: (payload: { pipelineId: string }) => Promise<void> | void;
  } = $props();

  const ioTypeOptions = $derived(buildTypeOptions(typePalette));

  let ioPortNameDraft = $state('');
  let ioPortTypeKey = $state('generic');

  $effect(() => {
    if (!ioNodeContext) return;
    ioPortNameDraft = '';
    ioPortTypeKey = 'generic';
  });

  function addIoPort() {
    if (!ioNodeContext) return;
    const name = ioPortNameDraft.trim();
    if (!name) return;
    const dataTypeKey = ioPortTypeKey?.trim() || 'generic';
    onAddIoPort?.({ nodeId: ioNodeContext.nodeId, name, dataTypeKey });
    ioPortNameDraft = '';
    ioPortTypeKey = 'generic';
  }

  function removeIoPort(name: string) {
    if (!ioNodeContext) return;
    const trimmed = name.trim();
    if (!trimmed) return;
    onRemoveIoPort?.({ nodeId: ioNodeContext.nodeId, name: trimmed });
  }

  const relinkOptions = $derived.by<Array<{ id: string; label: string }>>(() =>
    pipelines
      .filter((pipeline) => pipeline.id !== pipelineId)
      .map((pipeline) => ({
        id: pipeline.id,
        label: pipeline.alias ?? pipeline.name ?? pipeline.id
      }))
  );

  const canOpenExternal = $derived.by(() => {
    const targetId = childLinkState?.targetId;
    if (!targetId) return false;
    return pipelines.some((pipeline) => pipeline.id === targetId);
  });

  let relinkTarget = $state<string | null>(null);
  let relinkBusy = $state(false);
  let relinkError = $state<string | null>(null);

  $effect(() => {
    void graphSelectionNodeId;
    void childLinkState?.status;
    relinkTarget = null;
    relinkError = null;
  });

  async function relinkExternal() {
    if (!graphSelectionNodeId || !childLinkState || !relinkTarget || relinkBusy) return;
    relinkBusy = true;
    relinkError = null;
    try {
      await onRelinkExternal?.({ nodeId: graphSelectionNodeId, pipelineId: relinkTarget });
      relinkTarget = null;
    } catch (error) {
      relinkError = (error as Error)?.message ?? 'Relink failed';
    } finally {
      relinkBusy = false;
    }
  }

  function openExternalPipeline() {
    if (!childLinkState?.targetId) return;
    if (!canOpenExternal) {
      relinkError = 'Referenced pipeline is not available locally.';
      return;
    }
    relinkError = null;
    void onOpenExternal?.({ pipelineId: childLinkState.targetId });
  }
</script>

{#if ioNodeContext}
  <section class="space-y-2 rounded border border-surface-700/70 bg-surface-900/70 p-3">
    <div class="flex items-center justify-between gap-2">
      <p class="text-[0.68rem] uppercase tracking-[0.26em] text-surface-400">Pipeline IO ports</p>
      <span class="text-micro text-surface-500">{ioNodeContext.kind === 'input' ? 'Inputs' : 'Outputs'}</span>
    </div>
    <p class="text-xs text-surface-400">
      {ioNodeContext.kind === 'input'
        ? 'These ports enter the pipeline from the host.'
        : 'These ports exit the pipeline to the host.'}
    </p>
    <div class="space-y-2">
      {#if ioNodeContext.ports.length === 0}
        <p class="text-xs text-surface-500">No ports yet. Add one below.</p>
      {:else}
        {#each ioNodeContext.ports as port (port.name)}
          <div class="flex items-center gap-2 rounded border border-surface-800/80 bg-surface-950/60 px-2 py-1.5">
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm font-semibold text-white">{port.name}</p>
              <p class="text-[0.7rem] text-surface-500">{describePortType(port.dataType, typePalette)}</p>
            </div>
            <button
              class="h-7 w-7 rounded border border-surface-700/70 bg-surface-900/70 text-xs font-semibold text-surface-300 transition hover:border-error-400/70 hover:bg-error-500/15 hover:text-error-100"
              type="button"
              onclick={() => removeIoPort(port.name)}
              aria-label={`Remove ${port.name}`}
            >
              –
            </button>
          </div>
        {/each}
      {/if}
    </div>
    <div class="mt-3 flex flex-wrap items-center gap-2">
      <input
        class="input h-9 flex-1 text-sm"
        placeholder="New port name"
        bind:value={ioPortNameDraft}
        onkeydown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            addIoPort();
          }
        }}
      />
      <select class="input h-9 text-sm" bind:value={ioPortTypeKey}>
        {#each ioTypeOptions as option (option.key)}
          <option value={option.key}>{option.label}</option>
        {/each}
      </select>
      <button
        class="h-9 rounded border border-primary-500/50 bg-primary-500/15 px-3 text-[0.75rem] font-semibold uppercase tracking-[0.22em] text-primary-50 transition hover:bg-primary-500/25 disabled:opacity-40"
        type="button"
        onclick={addIoPort}
        disabled={!ioPortNameDraft.trim()}
      >
        + Add
      </button>
    </div>
  </section>
{/if}

{#if childLinkState}
  <section class="space-y-2 rounded border border-surface-700/60 bg-surface-900/70 p-3">
    <div class="flex items-center justify-between gap-2">
      <div>
        <p class="text-micro uppercase tracking-[0.26em] text-surface-400">Pipeline link</p>
        <p class="text-base font-semibold text-white">
          {#if childLinkState.alias}
            {childLinkState.alias}
          {:else if childLinkState.hasExternal}
            External pipeline
          {:else}
            Embedded child
          {/if}
        </p>
        <p class="text-xs text-surface-400">
          {#if childLinkState.status === 'linked'}
            Reference resolved.
          {:else if childLinkState.status === 'embedded'}
            Inline embedded pipeline.
          {:else if childLinkState.status === 'mismatch'}
            Signature or revision mismatch with referenced pipeline.
          {:else}
            Referenced pipeline missing or unresolved.
          {/if}
        </p>
      </div>
      {#if childLinkState.status === 'linked'}
        <span class="rounded-full border border-primary-500/60 bg-primary-500/10 px-3 py-[3px] text-[0.7rem] font-semibold uppercase tracking-[0.24em] text-primary-100">
          Linked
        </span>
      {:else if childLinkState.status === 'embedded'}
        <span class="rounded-full border border-surface-600/70 bg-surface-800/60 px-3 py-[3px] text-[0.7rem] font-semibold uppercase tracking-[0.24em] text-surface-200">
          Inline
        </span>
      {:else}
        <span class="rounded-full border border-error-500/70 bg-error-500/10 px-3 py-[3px] text-[0.7rem] font-semibold uppercase tracking-[0.24em] text-error-100">
          Needs relink
        </span>
      {/if}
    </div>
    {#if childLinkState.targetId}
      <div class="flex flex-wrap gap-2">
        <button
          class="btn btn-3xs preset-outline uppercase tracking-[0.26em]"
          type="button"
          onclick={openExternalPipeline}
          disabled={!canOpenExternal}
        >
          {canOpenExternal ? 'Open pipeline' : 'Missing target'}
        </button>
      </div>
    {/if}

    {#if (childLinkState.status === 'unresolved' || childLinkState.status === 'mismatch') && relinkOptions.length > 0}
      <div class="space-y-2 rounded border border-surface-700/70 bg-surface-900/80 p-3">
        <p class="text-micro uppercase tracking-[0.26em] text-surface-400">Relink to pipeline</p>
        <select
          class="input w-full text-sm"
          value={relinkTarget ?? ''}
          onchange={(event) => (relinkTarget = (event.currentTarget as HTMLSelectElement).value || null)}
        >
          <option value="">Select pipeline…</option>
          {#each relinkOptions as option (option.id)}
            <option value={option.id}>{option.label}</option>
          {/each}
        </select>
        <div class="flex items-center gap-2">
          <button
            class="btn btn-3xs preset-outline uppercase tracking-[0.26em]"
            type="button"
            onclick={relinkExternal}
            disabled={!relinkTarget || relinkBusy}
          >
            {relinkBusy ? 'Relinking…' : 'Relink'}
          </button>
          {#if relinkError}
            <span class="text-[0.7rem] text-error-200">{relinkError}</span>
          {/if}
        </div>
      </div>
    {/if}
  </section>
{/if}
