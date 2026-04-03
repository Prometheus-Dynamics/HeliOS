<script lang="ts">
  import type {
    PipelineMissingDataPolicy,
    PipelineNodeSyncConfig,
    PipelineReadinessPolicy,
    PipelineSyncDropPolicy,
    PipelineSyncGroupConfig,
    PipelineWorkKey
  } from '$lib/types/pipeline';

  type SyncPolicyControlsProps = {
    syncDraft: PipelineNodeSyncConfig;
    tickSourceKind: 'ports' | 'timer';
    tickSourceInterval: number;
    tickModeValue: 'all' | 'any' | 'primary';
    primaryGroupSelection: string | null;
    defaultTimerIntervalMs: number;
    groupAccentColor: (index: number) => string;
    onTickSourceChange: (kind: 'ports' | 'timer') => void;
    onTickSourceIntervalChange: (value: number) => void;
    onTickModeChange: (event: Event) => void;
    onPrimaryGroupChange: (groupId: string) => void;
    onUpdateGroup: (index: number, updater: (group: PipelineSyncGroupConfig) => void) => void;
  };

  const {
    syncDraft,
    tickSourceKind,
    tickSourceInterval,
    tickModeValue,
    primaryGroupSelection,
    defaultTimerIntervalMs,
    groupAccentColor,
    onTickSourceChange,
    onTickSourceIntervalChange,
    onTickModeChange,
    onPrimaryGroupChange,
    onUpdateGroup
  }: SyncPolicyControlsProps = $props();

  export type $$Props = SyncPolicyControlsProps;

  function handleTickSourceChange(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    if (target.value !== 'ports' && target.value !== 'timer') return;
    onTickSourceChange(target.value);
  }

  function handleMatchKeyChange(index: number, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    if (target.value !== 'workId' && target.value !== 'timestamp' && target.value !== 'localId') return;
    onUpdateGroup(index, (candidate) => {
      candidate.matchKey = target.value as PipelineWorkKey;
    });
  }

  function handleReadinessChange(index: number, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    if (target.value !== 'allSameKey' && target.value !== 'any') return;
    onUpdateGroup(index, (candidate) => {
      candidate.readiness = target.value as PipelineReadinessPolicy;
    });
  }

  function handleDropChange(index: number, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    if (
      target.value !== 'dropOldest' &&
      target.value !== 'dropNewest' &&
      target.value !== 'keepLatest' &&
      target.value !== 'block'
    ) {
      return;
    }
    onUpdateGroup(index, (candidate) => {
      candidate.drop = target.value as PipelineSyncDropPolicy;
    });
  }

  function handleStalenessKindChange(index: number, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    const kind = target.value;
    if (kind !== 'allowAny' && kind !== 'requireExact' && kind !== 'maxLagCount' && kind !== 'maxLagDuration') {
      return;
    }
    onUpdateGroup(index, (candidate) => {
      if (kind === 'maxLagCount') {
        candidate.staleness = {
          kind,
          maxDistance:
            candidate.staleness && 'maxDistance' in candidate.staleness ? candidate.staleness.maxDistance : 3
        };
      } else if (kind === 'maxLagDuration') {
        candidate.staleness = {
          kind,
          maxLagMs: candidate.staleness && 'maxLagMs' in candidate.staleness ? candidate.staleness.maxLagMs : 33
        };
      } else {
        candidate.staleness = { kind };
      }
    });
  }

  function handleMissingKindChange(index: number, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    const kind = target.value;
    if (kind !== 'allowNone' && kind !== 'skipTick' && kind !== 'wait') return;
    onUpdateGroup(index, (candidate) => {
      if (kind === 'wait') {
        candidate.missing = {
          kind,
          timeoutMs: candidate.missing?.kind === 'wait' ? candidate.missing.timeoutMs ?? 10 : 10
        };
      } else {
        candidate.missing = { kind } as PipelineMissingDataPolicy;
      }
    });
  }
</script>

<div class="grid gap-2 md:grid-cols-2">
  <div class="control-tile">
    <p class="control-tile__eyebrow">Tick source</p>
    <div class="flex flex-wrap items-center gap-2">
      <select
        class="input h-8 text-xs"
        value={tickSourceKind}
        onchange={handleTickSourceChange}
      >
        <option value="ports">Input ports</option>
        <option value="timer">Timer</option>
      </select>
      {#if tickSourceKind === 'timer'}
        <input
          class="input h-8 text-xs"
          type="number"
          min="1"
          value={tickSourceInterval}
          oninput={(event) =>
            onTickSourceIntervalChange(Number(event.currentTarget.value) || defaultTimerIntervalMs)}
        />
        <span class="text-micro text-surface-400">ms</span>
      {:else}
        <span class="text-micro text-surface-400">Port ready</span>
      {/if}
    </div>
  </div>
  <div class="control-tile">
    <p class="control-tile__eyebrow">Tick policy</p>
    <div class="space-y-2">
      <select class="input h-8 text-xs" value={tickModeValue} onchange={onTickModeChange}>
        <option value="all">All groups ready</option>
        <option value="any">Any group ready</option>
        <option value="primary">Primary group</option>
      </select>
      {#if tickModeValue === 'primary'}
        <select
          class="input h-8 text-xs"
          value={primaryGroupSelection ?? ''}
          onchange={(event) => onPrimaryGroupChange(event.currentTarget.value)}
        >
          {#each syncDraft.groups as group (`primary-option-${group.id}`)}
            <option value={group.id}>{group.id}</option>
          {/each}
        </select>
      {/if}
    </div>
  </div>
</div>

<div class="policy-groups space-y-2">
  <p class="text-[0.62rem] text-surface-400">Required groups must deliver a packet before a tick fires; others are optional.</p>
  {#if syncDraft.groups.length === 0}
    <p class="text-[0.7rem] text-surface-500">Add groups in the Assign tab to configure policies.</p>
  {:else}
    {#each syncDraft.groups as group, index (`policy-${group.id}-${index}`)}
      <div class="policy-row">
        <div class="policy-row__meta">
          <span class="policy-row__dot" style={`--lane-accent:${groupAccentColor(index)};`}></span>
          <div>
            <p class="text-[0.7rem] font-semibold text-surface-50">{group.id}</p>
            <p class="text-micro-tight text-surface-400">{(group.ports?.length ?? 0)} ports</p>
          </div>
        </div>
        <div class="policy-row__controls">
          <label class="group-rule">
            <span>Match</span>
            <select
              class="input h-8 text-xs"
              value={group.matchKey}
              onchange={(event) => handleMatchKeyChange(index, event)}
            >
              <option value="workId">Work ID</option>
              <option value="timestamp">Timestamp</option>
              <option value="localId">Local ID</option>
            </select>
          </label>
          <label class="group-rule">
            <span>Ready</span>
            <select
              class="input h-8 text-xs"
              value={group.readiness}
              onchange={(event) => handleReadinessChange(index, event)}
            >
              <option value="allSameKey">All same key</option>
              <option value="any">Any</option>
            </select>
          </label>
          <label class="group-rule">
            <span>Drop</span>
            <select
              class="input h-8 text-xs"
              value={group.drop}
              onchange={(event) => handleDropChange(index, event)}
            >
              <option value="dropOldest">Drop oldest</option>
              <option value="dropNewest">Drop newest</option>
              <option value="keepLatest">Keep latest</option>
              <option value="block">Block</option>
            </select>
          </label>
          <label class="group-rule">
            <span>Lag</span>
            <select
              class="input h-8 text-xs"
              value={group.staleness?.kind ?? 'allowAny'}
              onchange={(event) => handleStalenessKindChange(index, event)}
            >
              <option value="allowAny">Allow any</option>
              <option value="requireExact">Require exact</option>
              <option value="maxLagCount">Max lag count</option>
              <option value="maxLagDuration">Max lag ms</option>
            </select>
            {#if group.staleness?.kind === 'maxLagCount'}
              <input
                class="input h-8 text-xs"
                type="number"
                min="1"
                value={group.staleness.maxDistance}
                oninput={(event) =>
                  onUpdateGroup(index, (candidate) => {
                    if (candidate.staleness?.kind === 'maxLagCount') {
                      candidate.staleness.maxDistance = Math.max(
                        1,
                        Number(event.currentTarget.value) || 1
                      );
                    }
                  })}
              />
            {:else if group.staleness?.kind === 'maxLagDuration'}
              <input
                class="input h-8 text-xs"
                type="number"
                min="1"
                value={group.staleness.maxLagMs}
                oninput={(event) =>
                  onUpdateGroup(index, (candidate) => {
                    if (candidate.staleness?.kind === 'maxLagDuration') {
                      candidate.staleness.maxLagMs = Math.max(
                        1,
                        Number(event.currentTarget.value) || 1
                      );
                    }
                  })}
              />
            {/if}
          </label>
          <label class="group-rule">
            <span>Missing</span>
            <select
              class="input h-8 text-xs"
              value={group.missing?.kind ?? 'allowNone'}
              onchange={(event) => handleMissingKindChange(index, event)}
            >
              <option value="allowNone">Allow none</option>
              <option value="skipTick">Skip tick</option>
              <option value="wait">Wait (timeout)</option>
            </select>
            {#if group.missing?.kind === 'wait'}
              <input
                class="input h-8 text-xs"
                type="number"
                min="0"
                value={group.missing.timeoutMs ?? 10}
                oninput={(event) =>
                  onUpdateGroup(index, (candidate) => {
                    if (candidate.missing?.kind === 'wait') {
                      candidate.missing.timeoutMs = Math.max(
                        0,
                        Number(event.currentTarget.value) || 0
                      );
                    }
                  })}
              />
            {/if}
          </label>
        </div>
      </div>
    {/each}
  {/if}
</div>
