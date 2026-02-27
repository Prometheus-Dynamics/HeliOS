<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import {
    defaultSyncConfigForNode,
    metadataAlignedPortHints
  } from '$lib/features/pipelines/workbench/inspector/syncPolicyUtils';
  import NodeSyncPolicyEditor from './inspector/NodeSyncPolicyEditor.svelte';
  import DaedalusNodeRuntimeEditor from './inspector/DaedalusNodeRuntimeEditor.svelte';
  import { faSliders, faWandMagicSparkles, faEraser } from '@fortawesome/free-solid-svg-icons';
  import type { PipelineDetailContext } from '$lib';
  import type { PipelineNodeSyncConfig } from '$lib/types/pipeline';

  const {
    context,
    onSetSyncConfig,
    onSetDaedalusNodeRuntime
  }: {
    context: PipelineDetailContext;
    onSetSyncConfig?: (payload: { nodeId: string; config: PipelineNodeSyncConfig | null }) => void;
    onSetDaedalusNodeRuntime?: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
  } = $props();

  const selectedNodeId = $derived.by(() => context.graphSelectionNodeId ?? null);

  const selectedNode = $derived.by(() => {
    if (!context.pipeline || !selectedNodeId) return null;
    return context.pipeline.graph.nodes?.[selectedNodeId] ?? null;
  });

  const isDaedalusGraph = $derived.by(() => {
    const graph = context.pipeline?.graph;
    return Boolean(graph?.format === 'daedalus' || graph?.daedalus);
  });

  const metadataHints = $derived.by(() => metadataAlignedPortHints(selectedNode));
  const canApplyMetadata = $derived.by(() => metadataHints.length >= 2);

  function applyDefaultSync() {
    if (!selectedNode || !selectedNodeId) return;
    const config = defaultSyncConfigForNode(selectedNode);
    onSetSyncConfig?.({ nodeId: selectedNodeId, config });
  }

  function applyMetadataSync() {
    if (!selectedNode || !selectedNodeId || metadataHints.length < 2) return;
    const groupId = 'aligned';
    const config: PipelineNodeSyncConfig = {
      groups: [
        {
          id: groupId,
          ports: metadataHints,
          matchKey: 'workId',
          readiness: 'allSameKey',
          staleness: { kind: 'requireExact' },
          drop: 'dropOldest',
          missing: { kind: 'wait', timeoutMs: 5 }
        }
      ],
      tickPolicy: { requiredGroups: [groupId], mode: 'allGroups' },
      tickSource: { kind: 'ports' }
    };
    onSetSyncConfig?.({ nodeId: selectedNodeId, config });
  }

  function clearSync() {
    if (!selectedNodeId) return;
    onSetSyncConfig?.({ nodeId: selectedNodeId, config: null });
  }

  const daedalusSyncGroups = $derived.by(() => {
    const groups = (selectedNode?.source as { sync_groups?: unknown } | null | undefined)?.sync_groups;
    return Array.isArray(groups) ? groups : [];
  });
  const syncEnabled = $derived.by(() => {
    if (isDaedalusGraph) return daedalusSyncGroups.length > 0;
    return Boolean(selectedNode?.sync);
  });
</script>

{#if context.pipeline}
  <div class="pointer-events-none absolute right-4 top-4 z-30 flex flex-col items-end gap-2">
    <section class="sync-overlay pointer-events-auto w-full max-w-4xl border border-surface-800 bg-surface-950/90 px-4 py-3 text-xs text-surface-200 shadow-xl backdrop-blur md:max-w-5xl">
      <header class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <p class="text-[0.58rem] uppercase tracking-[0.35em] text-surface-500">
            {isDaedalusGraph ? 'Sync groups' : 'Sync workspace'}
          </p>
          <p class="text-[0.82rem] font-semibold text-surface-50">
            {selectedNode ? selectedNode.metadata?.name ?? selectedNode.backendId : 'Select a node'}
          </p>
          {#if isDaedalusGraph}
            <p class="text-[0.62rem] text-surface-400">
              Daedalus sync groups are configured on the node runtime panel.
            </p>
          {:else}
            <p class="text-[0.62rem] text-surface-400">Summaries update as you drag ports in the visual board below.</p>
          {/if}
        </div>
        <div class="inline-flex items-center gap-2 border border-surface-500/60 bg-surface-900/70 px-3 py-1 text-[0.58rem] uppercase tracking-[0.18em] text-surface-100">
          <span
            class={`h-2.5 w-2.5 transition ${
              syncEnabled ? 'bg-success-400 ring-[0.35rem] ring-success-400/25' : 'bg-surface-600 ring-[0.35rem] ring-surface-600/30'
            }`}
          ></span>
          <span>{syncEnabled ? 'Enforced' : 'Disabled'}</span>
        </div>
      </header>
      {#if isDaedalusGraph}
        <div class="mt-3 border border-surface-800/60 bg-surface-950/75 p-3">
          {#if !selectedNodeId || !selectedNode}
            <p class="text-micro text-surface-400">Select a node to configure sync groups.</p>
          {:else}
            <DaedalusNodeRuntimeEditor
              node={selectedNode}
              nodeId={selectedNodeId}
              showHeader={false}
              showCompute={false}
              onChange={(payload) =>
                onSetDaedalusNodeRuntime?.({
                  nodeId: payload.nodeId,
                  syncGroups: payload.syncGroups
                })}
            />
          {/if}
        </div>
      {:else}
        <div class="mt-3 flex flex-wrap gap-2" role="toolbar" aria-label="Sync presets">
          <button
            class="inline-flex items-center gap-1.5 border border-surface-600/60 bg-surface-900/90 px-3 py-2 text-micro-tight font-semibold uppercase tracking-[0.25em] text-surface-50 transition hover:border-primary-500/70 hover:bg-surface-800/90 disabled:cursor-not-allowed disabled:opacity-45"
            type="button"
            onclick={applyDefaultSync}
            disabled={!selectedNode}
            title="Apply balanced defaults"
          >
            <FaIcon icon={faSliders} class="h-3 w-3" />
            <span>Default</span>
          </button>
          <button
            class="sync-quick-chip"
            type="button"
            onclick={applyMetadataSync}
            disabled={!selectedNode || !canApplyMetadata}
            title={
              !selectedNode
                ? 'Select a node to apply metadata hints'
                : canApplyMetadata
                  ? `Aligned ports: ${metadataHints.join(', ')}`
                  : 'Selected node does not advertise aligned inputs'
            }
          >
            <FaIcon icon={faWandMagicSparkles} class="h-3 w-3" />
            <span>Metadata</span>
          </button>
          <button
            class="inline-flex items-center gap-1.5 border border-surface-600/60 bg-surface-900/90 px-3 py-2 text-micro-tight font-semibold uppercase tracking-[0.25em] text-surface-50 transition hover:border-primary-500/70 hover:bg-surface-800/90 disabled:cursor-not-allowed disabled:opacity-45"
            type="button"
            onclick={clearSync}
            disabled={!selectedNode}
            title="Remove sync configuration"
          >
            <FaIcon icon={faEraser} class="h-3 w-3" />
            <span>Clear</span>
          </button>
        </div>
        <div class="mt-2 max-h-[60vh] max-h-[60svh] max-h-[60dvh] overflow-y-auto border border-surface-800/60 bg-surface-950/75 p-3">
          {#if !selectedNodeId}
            <p class="text-micro text-surface-400">Select a node to configure sync policy.</p>
          {:else}
            <NodeSyncPolicyEditor context={context} nodeId={selectedNodeId} onSetSyncConfig={onSetSyncConfig} />
          {/if}
        </div>
      {/if}
    </section>
  </div>
{/if}

<style>
  .sync-overlay :global(.btn),
  .sync-overlay :global(.input),
  .sync-overlay :global(select),
  .sync-overlay :global(button),
  .sync-overlay :global(input),
  .sync-overlay :global(textarea) {
    border-radius: 0;
  }

  .sync-overlay :global(.btn) {
    min-height: 1.9rem;
  }
</style>
