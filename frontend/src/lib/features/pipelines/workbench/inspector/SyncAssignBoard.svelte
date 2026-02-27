<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faTrash } from '@fortawesome/free-solid-svg-icons';
  import type { PipelineNodeSyncConfig } from '$lib/types/pipeline';

  type SyncAssignBoardProps = {
    syncDraft: PipelineNodeSyncConfig;
    syncUnassignedPorts: string[];
    quickAssignGroupId: string | null;
    dragTarget: string | null;
    draggedPort: string | null;
    unassignedTarget: string;
    tickModeValue: 'all' | 'any' | 'primary';
    primaryGroupSelection: string | null;
    newGroupId: string;
    groupAccentColor: (index: number) => string;
    portsForGroup: (groupId: string | null) => string[];
    canonicalPortName: (port: string) => string;
    onDragOver: (event: DragEvent, targetId: string | null) => void;
    onDragLeave: () => void;
    onDrop: (event: DragEvent, targetId: string | null) => void;
    onDragStart: (port: string, event: DragEvent) => void;
    onDragEnd: () => void;
    onAssignPortToGroup: (groupId: string | null, port: string) => void;
    onRenameGroup: (index: number, value: string) => void;
    onToggleRequiredGroup: (groupId: string, enabled: boolean) => void;
    onSetPrimaryGroup: (groupId: string) => void;
    onRemoveGroup: (index: number) => void;
    onMovePortToGroup: (port: string, groupId: string | null) => void;
    onAddGroup: () => void;
    onUpdateNewGroupId: (value: string) => void;
  };

  const {
    syncDraft,
    syncUnassignedPorts,
    quickAssignGroupId,
    dragTarget,
    draggedPort,
    unassignedTarget,
    tickModeValue,
    primaryGroupSelection,
    newGroupId,
    groupAccentColor,
    portsForGroup,
    canonicalPortName,
    onDragOver,
    onDragLeave,
    onDrop,
    onDragStart,
    onDragEnd,
    onAssignPortToGroup,
    onRenameGroup,
    onToggleRequiredGroup,
    onSetPrimaryGroup,
    onRemoveGroup,
    onMovePortToGroup,
    onAddGroup,
    onUpdateNewGroupId
  }: SyncAssignBoardProps = $props();

  export type $$Props = SyncAssignBoardProps;
</script>

<div class="sync-board border border-surface-800/70 bg-surface-950/60 p-2.5 shadow-inner">
  <div
    class={`sync-board__lane ${dragTarget === unassignedTarget ? 'is-drop-target' : ''}`}
    role="group"
    aria-label="Unassigned ports"
    ondragover={(event) => onDragOver(event, null)}
    ondragleave={onDragLeave}
    ondrop={(event) => onDrop(event, null)}
  >
    <header class="flex items-start justify-between gap-2">
      <div>
        <p class="text-[0.62rem] uppercase tracking-[0.28em] text-surface-500">Unassigned</p>
        <p class="text-[0.78rem] font-semibold text-surface-100">Ports waiting</p>
      </div>
    </header>
    <div class={`port-dropzone ${dragTarget === unassignedTarget ? 'is-drop-target' : ''}`}>
      {#if syncUnassignedPorts.length === 0}
        <p class="text-micro text-surface-400">Everything is grouped. Drag a chip here to unassign.</p>
      {:else}
        <div class="flex flex-wrap gap-2">
          {#each syncUnassignedPorts as port (port)}
            <button
              class="port-chip"
              type="button"
              draggable="true"
              ondragstart={(event) => onDragStart(port, event)}
              ondragend={onDragEnd}
              onclick={() => onAssignPortToGroup(quickAssignGroupId ?? syncDraft.groups[0]?.id ?? null, port)}
            >
              <span>{port}</span>
              {#if syncDraft.groups.length > 0}
                <span class="port-chip__hint">Tap to send</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>

  {#each syncDraft.groups as group, index (`${group.id ?? ''}-${index}`)}
    {@const assignedPorts = portsForGroup(group.id)}
    <div
      class={`sync-board__lane ${dragTarget === group.id ? 'is-drop-target' : ''}`}
      style={`--lane-accent:${groupAccentColor(index)};`}
      role="group"
      aria-label={`Sync group ${group.id || index + 1}`}
      ondragover={(event) => onDragOver(event, group.id)}
      ondragleave={onDragLeave}
      ondrop={(event) => onDrop(event, group.id)}
    >
      <div class="group-head">
        <div class="flex flex-wrap items-center gap-2 pr-9">
          <span class="lane-dot" aria-hidden="true"></span>
          <input
            class="input h-8 text-xs"
            value={group.id}
            oninput={(event) => onRenameGroup(index, (event.currentTarget as HTMLInputElement).value)}
          />
        </div>
        <div class="flex flex-wrap gap-2 text-[0.58rem] uppercase tracking-[0.2em]">
          <button
            class={`pill ${syncDraft.tickPolicy.requiredGroups.includes(group.id) ? 'pill--active' : ''}`}
            type="button"
            onclick={() => onToggleRequiredGroup(group.id, !syncDraft.tickPolicy.requiredGroups.includes(group.id))}
          >
            Required
          </button>
          <button
            class={`pill ${tickModeValue === 'primary' && primaryGroupSelection === group.id ? 'pill--active' : ''}`}
            type="button"
            onclick={() => onSetPrimaryGroup(group.id)}
          >
            Primary
          </button>
        </div>
        <button
          class="pill pill--danger group-remove"
          type="button"
          title="Remove group"
          aria-label={`Remove group ${group.id}`}
          onclick={() => onRemoveGroup(index)}
        >
          <FaIcon icon={faTrash} class="h-3.5 w-3.5" />
        </button>
      </div>
      <div class={`port-dropzone ${dragTarget === group.id ? 'is-drop-target' : ''}`}>
        {#if assignedPorts.length}
          <div class="flex flex-wrap gap-2">
            {#each assignedPorts as port (port)}
              <div
                class={`port-chip ${draggedPort === canonicalPortName(port) ? 'is-dragging' : ''}`}
                draggable="true"
                role="group"
                ondragstart={(event) => onDragStart(port, event)}
                ondragend={onDragEnd}
              >
                <span>{port}</span>
                <button
                  class="port-chip__remove"
                  type="button"
                  title="Remove from this group"
                  onclick={(event) => {
                    event.stopPropagation();
                    onMovePortToGroup(port, null);
                  }}
                >
                  ×
                </button>
              </div>
            {/each}
          </div>
        {:else}
          <p class="text-micro text-surface-400">Drop ports here or tap a chip from the unassigned lane.</p>
        {/if}
      </div>
    </div>
  {/each}

  <div class="sync-board__lane sync-board__lane--add">
    <p class="text-[0.62rem] uppercase tracking-[0.24em] text-surface-500">Add sync group</p>
    <input
      class="input h-8 text-xs"
      placeholder="Group ID"
      value={newGroupId}
      oninput={(event) => onUpdateNewGroupId((event.currentTarget as HTMLInputElement).value)}
    />
    <p class="text-[0.62rem] text-surface-400">New groups get their own accent stripe and become drop targets instantly.</p>
    <button class="btn btn-2xs preset-tonal uppercase tracking-[0.2em]" type="button" onclick={onAddGroup}>
      Create group
    </button>
  </div>
</div>
