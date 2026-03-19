<script lang="ts">
  import { toaster } from '$lib';
  import { cloneNodeSyncConfig } from '$lib/features/pipelines/model';
  import type { PipelineDetailContext } from '$lib';
  import type {
    PipelineGraphNode,
    PipelineNodeSyncConfig,
    PipelineSyncGroupConfig,
    PipelineTickMode
  } from '$lib/types/pipeline';
  import {
    canonicalPortName,
    defaultSyncConfigForNode,
    listUnassignedPorts,
    metadataAlignedPortHints,
    nodeSyncPorts,
    validateSyncConfig
  } from './syncPolicyUtils';
  import {
    groupAccentColor,
    updateTickMode,
    setPrimaryTickGroup,
    toggleRequiredGroup,
    removeGroup,
    updateGroup,
    renameGroup,
    movePortToGroup,
    syncAllInputs as syncAllInputsDraft
  } from './syncPolicyDraftHelpers';
  import SyncAssignBoard from './SyncAssignBoard.svelte';
  import SyncPolicyControls from './SyncPolicyControls.svelte';
  import SyncPolicyHeader from './SyncPolicyHeader.svelte';

  const DEFAULT_TIMER_INTERVAL_MS = 33;

  const {
    context,
    nodeId = null,
    onSetSyncConfig
  }: {
    context: PipelineDetailContext;
    nodeId?: string | null;
    onSetSyncConfig?: (payload: { nodeId: string; config: PipelineNodeSyncConfig | null }) => void;
  } = $props();

  const activeNodeId = $derived.by(() => nodeId ?? context.graphSelectionNodeId ?? null);

  const selectedNode = $derived.by<PipelineGraphNode | null>(() => {
    if (!context.pipeline || !activeNodeId) {
      return null;
    }
    return context.pipeline.graph.nodes?.[activeNodeId] ?? null;
  });

  const selectedNodePorts = $derived.by(() => nodeSyncPorts(selectedNode));
  const metadataSyncHints = $derived.by(() => metadataAlignedPortHints(selectedNode));
  const hasMetadataSyncHints = $derived.by(() => metadataSyncHints.length >= 2);
  const metadataSyncHintSummary = $derived.by(() => metadataSyncHints.join(', '));

  let syncDraft = $state<PipelineNodeSyncConfig | null>(null);
  let newGroupId = $state('');
  let draggedPort = $state<string | null>(null);
  let dragTarget = $state<string | null>(null);
  let viewTab = $state<'assign' | 'policies'>('assign');

  const UNASSIGNED_TARGET = '__unassigned__';
  $effect(() => {
    if (!selectedNode) {
      syncDraft = null;
      return;
    }
    syncDraft = cloneNodeSyncConfig(selectedNode.sync) ?? null;
  });

  const persistSyncConfig = (config: PipelineNodeSyncConfig | null) => {
    syncDraft = config ? cloneNodeSyncConfig(config) ?? null : null;
    if (!activeNodeId) return;
    onSetSyncConfig?.({ nodeId: activeNodeId, config });
  };

  const ensureSyncDraft = (): PipelineNodeSyncConfig | null => {
    if (!selectedNode) return null;
    if (syncDraft) {
      return cloneNodeSyncConfig(syncDraft) ?? null;
    }
    if (!selectedNode.sync) {
      return defaultSyncConfigForNode(selectedNode);
    }
    return cloneNodeSyncConfig(selectedNode.sync) ?? defaultSyncConfigForNode(selectedNode);
  };

  const modifySyncDraft = (mutator: (draft: PipelineNodeSyncConfig) => void) => {
    const base = ensureSyncDraft();
    if (!base) {
      persistSyncConfig(null);
      return;
    }
    mutator(base);
    persistSyncConfig(base);
  };

  function toggleSyncConfig(enabled: boolean) {
    if (!selectedNode) return;
    if (!enabled) {
      persistSyncConfig(null);
      return;
    }
    const config = ensureSyncDraft();
    if (config) {
      persistSyncConfig(config);
    }
  }

  function applyMetadataSyncHints() {
    if (!selectedNode) return;
    const ports = metadataSyncHints;
    if (ports.length < 2) {
      toaster.warning({
        title: 'No metadata hints',
        description: 'The selected node does not declare aligned inputs.'
      });
      return;
    }
    const groupId = 'aligned';
    const config: PipelineNodeSyncConfig = {
      groups: [
        {
          id: groupId,
          ports,
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
    persistSyncConfig(config);
    toaster.success({
      title: 'Sync hints applied',
      description: `Aligned inputs: ${ports.join(', ')}`
    });
  }

  function setTickSource(kind: 'ports' | 'timer') {
    modifySyncDraft((draft) => {
      if (kind === 'ports') {
        draft.tickSource = { kind: 'ports' };
      } else {
        const existing =
          draft.tickSource?.kind === 'timer' ? draft.tickSource.intervalMs : DEFAULT_TIMER_INTERVAL_MS;
        draft.tickSource = { kind: 'timer', intervalMs: existing };
      }
    });
  }

  function setTickSourceInterval(value: number) {
    modifySyncDraft((draft) => {
      if (!draft.tickSource || draft.tickSource.kind !== 'timer') {
        draft.tickSource = { kind: 'timer', intervalMs: Math.max(1, value || DEFAULT_TIMER_INTERVAL_MS) };
      } else {
        draft.tickSource.intervalMs = Math.max(1, value || DEFAULT_TIMER_INTERVAL_MS);
      }
    });
  }

  function addSyncGroup() {
    if (!selectedNode) return;
    const id = newGroupId.trim() || `group-${(syncDraft?.groups.length ?? 0) + 1}`;
    modifySyncDraft((draft) => {
      draft.groups.push({
        id,
        ports: [],
        matchKey: 'workId',
        readiness: 'allSameKey',
        staleness: { kind: 'requireExact' },
        drop: 'dropOldest',
        missing: { kind: 'wait', timeoutMs: 10 }
      });
      if (!draft.tickPolicy.requiredGroups.includes(id)) {
        draft.tickPolicy.requiredGroups.push(id);
      }
    });
    newGroupId = '';
  }

  function handleTickModeChange(event: Event) {
    const draft = syncDraft;
    if (!draft) return;
    const value = (event.currentTarget as HTMLSelectElement).value;
    if (value === 'all') {
      updateTickModeLocal('allGroups');
      return;
    }
    if (value === 'any') {
      updateTickModeLocal('anyGroup');
      return;
    }
    if (draft.groups.length === 0) return;
    const currentMode = draft.tickPolicy.mode;
    const primary =
      currentMode === 'allGroups' || currentMode === 'anyGroup'
        ? draft.groups[0]?.id ?? ''
        : currentMode.primaryGroup;
    updateTickModeLocal({ primaryGroup: primary });
  }

  function updateTickModeLocal(mode: PipelineTickMode) {
    modifySyncDraft((draft) => updateTickMode(draft, mode));
  }

  function setPrimaryTickGroupLocal(groupId: string) {
    modifySyncDraft((draft) => setPrimaryTickGroup(draft, groupId));
  }

  function toggleRequiredGroupLocal(groupId: string, enabled: boolean) {
    modifySyncDraft((draft) => toggleRequiredGroup(draft, groupId, enabled));
  }

  function removeGroupLocal(index: number) {
    modifySyncDraft((draft) => removeGroup(draft, index));
  }

  function updateGroupLocal(index: number, updater: (group: PipelineSyncGroupConfig) => void) {
    modifySyncDraft((draft) => updateGroup(draft, index, updater));
  }

  function renameGroupLocal(index: number, value: string) {
    const nextId = value.trim();
    if (!nextId) return;
    modifySyncDraft((draft) => renameGroup(draft, index, nextId));
  }

  const syncValidationErrors = $derived.by(() => validateSyncConfig(syncDraft, selectedNodePorts));
  const syncUnassignedPorts = $derived.by(() => listUnassignedPorts(syncDraft, selectedNodePorts));
  let quickAssignGroupId = $state<string | null>(null);

  $effect(() => {
    const groups = syncDraft?.groups ?? [];
    if (!groups.length) {
      quickAssignGroupId = null;
      return;
    }
    if (!quickAssignGroupId || !groups.some((group) => group.id === quickAssignGroupId)) {
      quickAssignGroupId = groups[0]?.id ?? null;
    }
  });

  const portAssignments = $derived.by<Record<string, string | null>>(() => {
    const index: Record<string, string | null> = {};
    selectedNodePorts.forEach((port) => {
      index[canonicalPortName(port)] = null;
    });
    (syncDraft?.groups ?? []).forEach((group) => {
      const id = group.id ?? null;
      (group.ports ?? []).forEach((port) => {
        const normalized = canonicalPortName(port);
        if (normalized in index) {
          index[normalized] = id;
        }
      });
    });
    return index;
  });

  const portsForGroup = (groupId: string | null) =>
    selectedNodePorts.filter((port) => (portAssignments[canonicalPortName(port)] ?? null) === groupId);

  function movePortToGroupLocal(port: string, targetGroupId: string | null) {
    modifySyncDraft((draft) => movePortToGroup(draft, port, targetGroupId));
  }

  const assignPortToGroup = (groupId: string | null, port: string) => {
    if (!groupId) return;
    movePortToGroupLocal(port, groupId);
  };

  function handlePortDragStart(port: string, event: DragEvent) {
    const canonical = canonicalPortName(port);
    draggedPort = canonical;
    event.dataTransfer?.setData('text/plain', canonical);
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move';
    }
  }

  function handleDragOver(event: DragEvent, targetId: string | null) {
    event.preventDefault();
    dragTarget = targetId ?? UNASSIGNED_TARGET;
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = 'move';
    }
  }

  function handleDragLeave() {
    dragTarget = null;
  }

  function handleDrop(event: DragEvent, targetId: string | null) {
    event.preventDefault();
    const payload = draggedPort || event.dataTransfer?.getData('text/plain');
    if (payload) {
      movePortToGroupLocal(payload, targetId);
    }
    draggedPort = null;
    dragTarget = null;
  }

  function handleDragEnd() {
    draggedPort = null;
    dragTarget = null;
  }

  function handleSyncAllInputs() {
    if (!selectedNodePorts.length) return;
    modifySyncDraft((draft) => syncAllInputsDraft(draft, selectedNodePorts));
  }

  const tickSourceKind = $derived.by(() =>
    syncDraft?.tickSource?.kind === 'timer' ? 'timer' : 'ports'
  );
  const tickSourceInterval = $derived.by(() =>
    syncDraft?.tickSource?.kind === 'timer'
      ? syncDraft.tickSource.intervalMs
      : DEFAULT_TIMER_INTERVAL_MS
  );
  const tickModeValue = $derived.by(() => {
    if (!syncDraft) return 'all';
    if (syncDraft.tickPolicy.mode === 'anyGroup') return 'any';
    if (syncDraft.tickPolicy.mode === 'allGroups') return 'all';
    return 'primary';
  });
  const primaryGroupSelection = $derived.by(() => {
    const mode = syncDraft?.tickPolicy.mode;
    if (!mode || mode === 'allGroups' || mode === 'anyGroup') return null;
    return mode.primaryGroup;
  });
</script>

{#if !context.pipeline || !selectedNode}
  <section class="border border-surface-800/80 bg-surface-950/80 p-4 text-center text-surface-400">
    Select a node to manage sync policy.
  </section>
{:else}
  <section class="sync-panel space-y-3">
    <SyncPolicyHeader
      selectedNode={selectedNode}
      syncDraft={syncDraft}
      hasMetadataSyncHints={hasMetadataSyncHints}
      metadataSyncHintSummary={metadataSyncHintSummary}
      hasSelectedPorts={selectedNodePorts.length > 0}
      onToggleSync={toggleSyncConfig}
      onApplyMetadataSyncHints={applyMetadataSyncHints}
      onSyncAllInputs={handleSyncAllInputs}
    />

    {#if syncValidationErrors.length > 0}
      <div class="space-y-1 border border-error-600/60 bg-error-500/10 p-3 text-[0.7rem] text-error-200 shadow-inner">
        <p class="text-micro uppercase tracking-[0.3em]">Sync issues</p>
        <ul class="list-disc pl-4">
          {#each syncValidationErrors as error (error)}
            <li>{error}</li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if syncDraft}
      <div class="sync-tabs" role="tablist" aria-label="Sync views">
        <button
          class={`sync-tab ${viewTab === 'assign' ? 'is-active' : ''}`}
          type="button"
          role="tab"
          aria-selected={viewTab === 'assign'}
          onclick={() => (viewTab = 'assign')}
        >
          Assign
        </button>
        <button
          class={`sync-tab ${viewTab === 'policies' ? 'is-active' : ''}`}
          type="button"
          role="tab"
          aria-selected={viewTab === 'policies'}
          onclick={() => (viewTab = 'policies')}
        >
          Policies
        </button>
      </div>

      <div class="sync-body">
        {#if viewTab === 'assign'}
          <SyncAssignBoard
            syncDraft={syncDraft}
            syncUnassignedPorts={syncUnassignedPorts}
            quickAssignGroupId={quickAssignGroupId}
            dragTarget={dragTarget}
            draggedPort={draggedPort}
            unassignedTarget={UNASSIGNED_TARGET}
            tickModeValue={tickModeValue}
            primaryGroupSelection={primaryGroupSelection}
            newGroupId={newGroupId}
            groupAccentColor={groupAccentColor}
            portsForGroup={portsForGroup}
            canonicalPortName={canonicalPortName}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
            onDragStart={handlePortDragStart}
            onDragEnd={handleDragEnd}
            onAssignPortToGroup={assignPortToGroup}
            onRenameGroup={renameGroupLocal}
            onToggleRequiredGroup={toggleRequiredGroupLocal}
            onSetPrimaryGroup={(groupId) => updateTickModeLocal({ primaryGroup: groupId })}
            onRemoveGroup={removeGroupLocal}
            onMovePortToGroup={movePortToGroupLocal}
            onAddGroup={addSyncGroup}
            onUpdateNewGroupId={(value) => (newGroupId = value)}
          />
        {:else}
          <SyncPolicyControls
            syncDraft={syncDraft}
            tickSourceKind={tickSourceKind}
            tickSourceInterval={tickSourceInterval}
            tickModeValue={tickModeValue}
            primaryGroupSelection={primaryGroupSelection}
            defaultTimerIntervalMs={DEFAULT_TIMER_INTERVAL_MS}
            groupAccentColor={groupAccentColor}
            onTickSourceChange={setTickSource}
            onTickSourceIntervalChange={setTickSourceInterval}
            onTickModeChange={handleTickModeChange}
            onPrimaryGroupChange={setPrimaryTickGroupLocal}
            onUpdateGroup={updateGroupLocal}
          />
        {/if}
      </div>
    {:else}
      <div class="border border-dashed border-surface-800/70 bg-surface-950/40 p-4 text-[0.75rem] text-surface-300">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Sync disabled</p>
        <p class="mt-1 text-surface-300">Enable sync or apply a preset above to open the visual grouping board.</p>
      </div>
    {/if}
  </section>
{/if}

<style>
  @import './NodeSyncPolicyEditor.css';
</style>
