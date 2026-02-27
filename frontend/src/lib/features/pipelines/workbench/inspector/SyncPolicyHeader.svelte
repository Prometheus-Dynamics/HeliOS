<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faArrowsRotate, faWandMagicSparkles } from '@fortawesome/free-solid-svg-icons';
  import type { PipelineGraphNode, PipelineNodeSyncConfig } from '$lib/types/pipeline';

  type SyncPolicyHeaderProps = {
    selectedNode: PipelineGraphNode;
    syncDraft: PipelineNodeSyncConfig | null;
    hasMetadataSyncHints: boolean;
    metadataSyncHintSummary: string;
    hasSelectedPorts: boolean;
    onToggleSync: (enabled: boolean) => void;
    onApplyMetadataSyncHints: () => void;
    onSyncAllInputs: () => void;
  };

  const {
    selectedNode,
    syncDraft,
    hasMetadataSyncHints,
    metadataSyncHintSummary,
    hasSelectedPorts,
    onToggleSync,
    onApplyMetadataSyncHints,
    onSyncAllInputs
  }: SyncPolicyHeaderProps = $props();

  export type $$Props = SyncPolicyHeaderProps;
</script>

<div class="border border-surface-800 bg-surface-900/70 p-3 shadow-md">
  <div class="flex flex-wrap items-start justify-between gap-3">
    <div class="space-y-1">
      <p class="text-[0.58rem] uppercase tracking-[0.32em] text-surface-500">Visual sync composer</p>
      <div class="flex flex-wrap items-center gap-2">
        <p class="text-base font-semibold text-surface-50">
          {selectedNode.metadata?.name ?? selectedNode.backendId}
        </p>
        <span class="inline-flex items-center gap-2 border border-surface-700/70 bg-surface-900/80 px-2.5 py-1 text-micro-tight uppercase tracking-[0.18em] text-surface-100">
          <span
            class={`h-2.5 w-2.5 ${syncDraft ? 'bg-primary-400 ring-[0.3rem] ring-primary-400/25' : 'bg-surface-600 ring-[0.3rem] ring-surface-700/40'}`}
          ></span>
          <span>{syncDraft ? 'Sync enforced' : 'Sync disabled'}</span>
        </span>
      </div>
      <p class="max-w-3xl text-micro-tight text-surface-400">Drag inputs into sync groups or drop to unassign.</p>
    </div>
    <label class="inline-flex items-center gap-2 border border-surface-700/60 bg-surface-900/70 px-3 py-2 text-[0.7rem] text-surface-100 shadow-sm">
      <input
        type="checkbox"
        class="form-checkbox h-4 w-4 border-surface-700 bg-surface-800"
        checked={!!syncDraft}
        onchange={(event) => onToggleSync((event.currentTarget as HTMLInputElement).checked)}
      />
      <span class="uppercase tracking-[0.22em]">Enable sync</span>
    </label>
  </div>
  <div class="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-3">
    <button
      class="sync-quick"
      type="button"
      onclick={onApplyMetadataSyncHints}
      disabled={!hasMetadataSyncHints}
      title={hasMetadataSyncHints
        ? `Aligned ports: ${metadataSyncHintSummary}`
        : 'Selected node metadata does not declare aligned inputs'}
    >
      <FaIcon icon={faWandMagicSparkles} class="h-4 w-4" />
      <span class="flex flex-col leading-tight">
        <span class="text-[0.62rem] uppercase tracking-[0.22em]">Metadata hints</span>
        <span class="text-micro-tight text-surface-300">Auto-pair aligned ports</span>
      </span>
    </button>
    <button class="sync-quick" type="button" onclick={onSyncAllInputs} disabled={!hasSelectedPorts}>
      <FaIcon icon={faArrowsRotate} class="h-4 w-4" />
      <span class="flex flex-col leading-tight">
        <span class="text-[0.62rem] uppercase tracking-[0.22em]">Sync all inputs</span>
        <span class="text-micro-tight text-surface-300">One balanced bundle</span>
      </span>
    </button>
  </div>
</div>
