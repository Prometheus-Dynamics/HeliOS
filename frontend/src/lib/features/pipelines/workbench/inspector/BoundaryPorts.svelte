<script lang="ts">
  import type {
    PipelineDataType,
    PipelineInputQueueConfig,
    PipelineNodeValue,
    PipelineOutputSinkConfig,
    PipelineTypeDescriptor
  } from '$lib/types/pipeline';
  import type { PipelineOutputEntry, PipelinePortEntry } from '$lib/components/pipelines/types';
  import {
    buildNodeValueFromInput,
    formatPipelineValue,
    getDataTypeVariants,
    isDataTypeSettable,
    resolveDataTypeKey
  } from '$lib/features/pipelines/valueFormatting';
  import InspectorSection from '$lib/components/pipelines/inspector/InspectorSection.svelte';
  import { buildTypeOptions, describePortType } from '$lib/features/pipelines/inspector/inspectorTypeUtils';
  import { channelPolicyOptions } from './channelPolicies';
  import BoundaryRules from './BoundaryRules.svelte';

  type PortConfigEvent =
    | { direction: 'input'; name: string; config: PipelineInputQueueConfig }
    | { direction: 'output'; name: string; config: PipelineOutputSinkConfig };

  type PortEditEvent = {
    direction: 'input' | 'output';
    nodeId: string;
    name: string;
    oldName?: string;
    dataTypeKey: string;
  };

  const {
    inputs = [],
    outputs = [],
    pipelineOutputConfigs = {},
    onRemovePort,
    onSetPortConfig,
    onSetPortValue,
    onEditPort,
    onAddPort,
    typePalette = {}
  }: {
    inputs?: PipelinePortEntry[];
    outputs?: PipelineOutputEntry[];
    pipelineOutputConfigs?: Record<string, PipelineOutputSinkConfig>;
    onRemovePort?: (payload: { direction: 'input' | 'output'; name: string }) => void;
    onSetPortConfig?: (payload: PortConfigEvent) => void;
    onSetPortValue?: (payload: { direction: 'input' | 'output'; name: string; value: PipelineNodeValue | null }) => void;
    onEditPort?: (payload: PortEditEvent) => void;
    onAddPort?: (payload: { direction: 'input' | 'output'; name: string; dataTypeKey: string }) => void;
    typePalette?: Record<string, PipelineTypeDescriptor>;
  } = $props();

  const boundaryTypeOptions = $derived(buildTypeOptions(typePalette));

  let newInputName = $state('');
  let newInputTypeKey = $state('generic');
  let newOutputName = $state('');
  let newOutputTypeKey = $state('generic');
  let pipelineInputDrafts = $state<Record<string, string>>({});
  let pipelineInputErrors = $state<Record<string, string | null>>({});
  let portEditDrafts = $state<Record<string, { name: string; dataTypeKey: string }>>({});
  let portEditOpen = $state<Record<string, boolean>>({});

  $effect(() => {
    const drafts: Record<string, string> = {};
    inputs.forEach((entry) => {
      if (entry.value) {
        drafts[entry.name] = formatPipelineValue(entry.value) ?? '';
      }
    });
    pipelineInputDrafts = drafts;
    pipelineInputErrors = {};
  });

  const portEditKey = (direction: 'input' | 'output', nodeId: string) => `${direction}:${nodeId}`;

  $effect(() => {
    const nextDrafts = { ...portEditDrafts };
    const nextOpen = { ...portEditOpen };
    const activeKeys = new Set<string>();
    inputs.forEach((entry) => {
      const key = portEditKey('input', entry.nodeId);
      activeKeys.add(key);
      if (!nextOpen[key]) {
        nextDrafts[key] = { name: entry.name, dataTypeKey: resolveDataTypeKey(entry.dataType) ?? 'generic' };
      } else if (!nextDrafts[key]) {
        nextDrafts[key] = { name: entry.name, dataTypeKey: resolveDataTypeKey(entry.dataType) ?? 'generic' };
      }
    });
    outputs.forEach((entry) => {
      const key = portEditKey('output', entry.nodeId);
      activeKeys.add(key);
      if (!nextOpen[key]) {
        nextDrafts[key] = { name: entry.name, dataTypeKey: resolveDataTypeKey(entry.dataType) ?? 'generic' };
      } else if (!nextDrafts[key]) {
        nextDrafts[key] = { name: entry.name, dataTypeKey: resolveDataTypeKey(entry.dataType) ?? 'generic' };
      }
    });
    Object.keys(nextDrafts).forEach((key) => {
      if (!activeKeys.has(key)) {
        delete nextDrafts[key];
        delete nextOpen[key];
      }
    });
    portEditDrafts = nextDrafts;
    portEditOpen = nextOpen;
  });

  function updatePortDraft(key: string, patch: Partial<{ name: string; dataTypeKey: string }>) {
    const existing = portEditDrafts[key] ?? { name: '', dataTypeKey: 'generic' };
    portEditDrafts = { ...portEditDrafts, [key]: { ...existing, ...patch } };
  }

  function togglePortEdit(key: string, open: boolean) {
    portEditOpen = { ...portEditOpen, [key]: open };
  }

  function cancelPortEdit(direction: 'input' | 'output', entry: PipelinePortEntry | PipelineOutputEntry) {
    const key = portEditKey(direction, entry.nodeId);
    portEditDrafts = {
      ...portEditDrafts,
      [key]: { name: entry.name, dataTypeKey: resolveDataTypeKey(entry.dataType) ?? 'generic' }
    };
    togglePortEdit(key, false);
  }

  function savePortEdit(direction: 'input' | 'output', entry: PipelinePortEntry | PipelineOutputEntry) {
    const key = portEditKey(direction, entry.nodeId);
    const draft = portEditDrafts[key] ?? { name: entry.name, dataTypeKey: resolveDataTypeKey(entry.dataType) ?? 'generic' };
    const name = draft.name.trim();
    const type = draft.dataTypeKey?.trim() || resolveDataTypeKey(entry.dataType) || 'generic';
    if (!name) return;
    onEditPort?.({
      direction,
      nodeId: entry.nodeId,
      name,
      oldName: entry.name,
      dataTypeKey: type
    });
    togglePortEdit(key, false);
  }

  function addPort(direction: 'input' | 'output') {
    const name = direction === 'input' ? newInputName.trim() : newOutputName.trim();
    if (!name) return;
    const dataTypeKey = direction === 'input' ? newInputTypeKey : newOutputTypeKey;
    onAddPort?.({ direction, name, dataTypeKey: dataTypeKey?.trim() || 'generic' });
    if (direction === 'input') {
      newInputName = '';
      newInputTypeKey = 'generic';
    } else {
      newOutputName = '';
      newOutputTypeKey = 'generic';
    }
  }

  function updateInputConfig(name: string, patch: Partial<PipelineInputQueueConfig>) {
    const current = inputs.find((entry) => entry.name === name)?.queueConfig ?? {
      policy: 'NewestWins',
      capacity: 3
    };
    onSetPortConfig?.({
      direction: 'input',
      name,
      config: { ...current, ...patch }
    });
  }

  function updateOutputCapacity(name: string, capacity: number) {
    const config: PipelineOutputSinkConfig = { capacity };
    onSetPortConfig?.({
      direction: 'output',
      name,
      config
    });
  }

  function updatePipelineInputDraft(name: string, value: string) {
    pipelineInputDrafts = { ...pipelineInputDrafts, [name]: value };
  }

  function applyPipelineInputValue(name: string, dataType: PipelineDataType) {
    if (!isDataTypeSettable(dataType)) {
      pipelineInputErrors = { ...pipelineInputErrors, [name]: 'Port is not settable' };
      return;
    }
    const variants = getDataTypeVariants(dataType);
    const draft = pipelineInputDrafts[name] ?? variants[0] ?? '';
    const typeKey = resolveDataTypeKey(dataType);
    if (!typeKey) {
      pipelineInputErrors = { ...pipelineInputErrors, [name]: 'Unknown data type' };
      return;
    }
    const result = buildNodeValueFromInput(draft, typeKey);
    if (!result.success) {
      pipelineInputErrors = { ...pipelineInputErrors, [name]: result.error };
      return;
    }
    pipelineInputErrors = { ...pipelineInputErrors, [name]: null };
    onSetPortValue?.({ direction: 'input', name, value: result.value });
  }

  function clearPipelineInputValue(name: string, dataType: PipelineDataType) {
    if (!isDataTypeSettable(dataType)) return;
    const nextDrafts = { ...pipelineInputDrafts };
    delete nextDrafts[name];
    pipelineInputDrafts = nextDrafts;
    pipelineInputErrors = { ...pipelineInputErrors, [name]: null };
    onSetPortValue?.({ direction: 'input', name, value: null });
  }
</script>

<InspectorSection title="Inputs">
  {#snippet children()}
    {#if inputs.length === 0}
      <p class="mt-2 text-xs text-surface-500">No inputs defined.</p>
    {:else}
      <ul class="mt-2 space-y-2">
        {#each inputs as entry (entry.name)}
          {@const variants = getDataTypeVariants(entry.dataType)}
          {@const entrySettable = isDataTypeSettable(entry.dataType)}
          {@const editKey = portEditKey('input', entry.nodeId)}
          {@const editOpen = Boolean(portEditOpen[editKey])}
          {@const editDraft = portEditDrafts[editKey] ?? { name: entry.name, dataTypeKey: resolveDataTypeKey(entry.dataType) ?? 'generic' }}
          <li class="rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-xs">
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="text-sm text-white">{entry.name}</p>
                <p class="text-surface-400">{describePortType(entry.dataType, typePalette)}</p>
                {#if entry.value}
                  <p class="text-surface-500">Value: {formatPipelineValue(entry.value)}</p>
                {/if}
                {#if entry.queueConfig}
                  <p class="text-surface-500">
                    Queue: {entry.queueConfig.policy} · {entry.queueConfig.capacity}
                  </p>
                {/if}
              </div>
              <div class="flex flex-wrap items-center gap-2">
                <button
                  class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
                  type="button"
                  onclick={() => togglePortEdit(editKey, !editOpen)}
                >
                  {editOpen ? 'Close' : 'Edit'}
                </button>
                <button
                  class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
                  type="button"
                  onclick={() => onRemovePort?.({ direction: 'input', name: entry.name })}
                >
                  Remove
                </button>
              </div>
            </div>
            {#if editOpen}
              <div class="mt-3 grid gap-2 md:grid-cols-2 text-micro text-surface-400">
                <label class="flex flex-col gap-1">
                  <span>Port name</span>
                  <input
                    class="input h-8 text-xs"
                    value={editDraft.name}
                    oninput={(event) => updatePortDraft(editKey, { name: (event.currentTarget as HTMLInputElement).value })}
                  />
                </label>
                <label class="flex flex-col gap-1">
                  <span>Type</span>
                  <select
                    class="input h-8 text-xs"
                    value={editDraft.dataTypeKey}
                    onchange={(event) =>
                      updatePortDraft(editKey, { dataTypeKey: (event.currentTarget as HTMLSelectElement).value })
                    }
                  >
                    {#each boundaryTypeOptions as option (option.key)}
                      <option value={option.key}>{option.label}</option>
                    {/each}
                  </select>
                </label>
                <div class="flex flex-wrap gap-2 md:col-span-2">
                  <button
                    class="btn btn-3xs preset-filled uppercase tracking-[0.3em]"
                    type="button"
                    onclick={() => savePortEdit('input', entry)}
                    disabled={!editDraft.name.trim()}
                  >
                    Save
                  </button>
                  <button
                    class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
                    type="button"
                    onclick={() => cancelPortEdit('input', entry)}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            {/if}
            <BoundaryRules
              direction="input"
              name={entry.name}
              variants={variants}
              settable={entrySettable}
              draftValue={pipelineInputDrafts[entry.name] ?? variants[0] ?? ''}
              error={pipelineInputErrors[entry.name]}
              policy={entry.queueConfig?.policy ?? 'NewestWins'}
              capacity={entry.queueConfig?.capacity ?? 3}
              channelPolicyOptions={channelPolicyOptions}
              onPolicyChange={(value) =>
                updateInputConfig(entry.name, { policy: value as PipelineInputQueueConfig['policy'] })
              }
              onCapacityChange={(value) => updateInputConfig(entry.name, { capacity: value })}
              onDraftChange={(value) => updatePipelineInputDraft(entry.name, value)}
              onApply={() => applyPipelineInputValue(entry.name, entry.dataType)}
              onClear={() => clearPipelineInputValue(entry.name, entry.dataType)}
            />
          </li>
        {/each}
      </ul>
    {/if}
  {/snippet}
</InspectorSection>

<section class="grid gap-3 rounded border border-surface-800/70 bg-surface-900/30 p-4 md:grid-cols-[1fr_auto]">
  <div>
    <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Add input</p>
    <div class="mt-1 grid gap-2 md:grid-cols-2">
      <input
        class="w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2 text-sm"
        placeholder="name"
        value={newInputName}
        oninput={(e) => (newInputName = (e.currentTarget as HTMLInputElement).value)}
      />
      <select
        class="w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2 text-sm"
        value={newInputTypeKey}
        onchange={(e) => (newInputTypeKey = (e.currentTarget as HTMLSelectElement).value)}
      >
        {#each boundaryTypeOptions as option (option.key)}
          <option value={option.key}>{option.label}</option>
        {/each}
      </select>
    </div>
  </div>
  <div class="flex items-end">
    <button
      class="btn btn-3xs preset-filled uppercase tracking-[0.3em]"
      type="button"
      onclick={() => addPort('input')}
      disabled={!newInputName.trim()}
    >
      Add
    </button>
  </div>
</section>

<InspectorSection title="Outputs">
  {#snippet children()}
    {#if outputs.length === 0}
      <p class="mt-2 text-xs text-surface-500">No outputs defined.</p>
    {:else}
      <ul class="mt-2 space-y-2">
        {#each outputs as entry (entry.name)}
          {@const editKey = portEditKey('output', entry.nodeId)}
          {@const editOpen = Boolean(portEditOpen[editKey])}
          {@const editDraft = portEditDrafts[editKey] ?? { name: entry.name, dataTypeKey: resolveDataTypeKey(entry.dataType) ?? 'generic' }}
          <li class="rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-xs">
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="text-sm text-white">{entry.name}</p>
                <p class="text-surface-400">{describePortType(entry.dataType, typePalette)}</p>
              </div>
              <div class="flex flex-wrap items-center gap-2">
                <button
                  class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
                  type="button"
                  onclick={() => togglePortEdit(editKey, !editOpen)}
                >
                  {editOpen ? 'Close' : 'Edit'}
                </button>
                <button
                  class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
                  type="button"
                  onclick={() => onRemovePort?.({ direction: 'output', name: entry.name })}
                >
                  Remove
                </button>
              </div>
            </div>
            {#if editOpen}
              <div class="mt-3 grid gap-2 md:grid-cols-2 text-micro text-surface-400">
                <label class="flex flex-col gap-1">
                  <span>Port name</span>
                  <input
                    class="input h-8 text-xs"
                    value={editDraft.name}
                    oninput={(event) => updatePortDraft(editKey, { name: (event.currentTarget as HTMLInputElement).value })}
                  />
                </label>
                <label class="flex flex-col gap-1">
                  <span>Type</span>
                  <select
                    class="input h-8 text-xs"
                    value={editDraft.dataTypeKey}
                    onchange={(event) =>
                      updatePortDraft(editKey, { dataTypeKey: (event.currentTarget as HTMLSelectElement).value })
                    }
                  >
                    {#each boundaryTypeOptions as option (option.key)}
                      <option value={option.key}>{option.label}</option>
                    {/each}
                  </select>
                </label>
                <div class="flex flex-wrap gap-2 md:col-span-2">
                  <button
                    class="btn btn-3xs preset-filled uppercase tracking-[0.3em]"
                    type="button"
                    onclick={() => savePortEdit('output', entry)}
                    disabled={!editDraft.name.trim()}
                  >
                    Save
                  </button>
                  <button
                    class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
                    type="button"
                    onclick={() => cancelPortEdit('output', entry)}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if outputs.length > 0}
      <div class="rounded border border-surface-800/60 bg-surface-950/30 p-3 text-micro space-y-2">
        <p class="uppercase tracking-[0.3em] text-surface-500">Capacity</p>
        {#each outputs as entry (entry.name)}
          <BoundaryRules
            direction="output"
            name={entry.name}
            capacity={pipelineOutputConfigs?.[entry.name]?.capacity ?? 4}
            onCapacityChange={(value) => updateOutputCapacity(entry.name, value)}
          />
        {/each}
      </div>
    {/if}
    <div class="grid gap-3 rounded border border-surface-800/70 bg-surface-900/30 p-4 md:grid-cols-[1fr_auto]">
      <div>
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Add output</p>
        <div class="mt-1 grid gap-2 md:grid-cols-2">
          <input
            class="w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2 text-sm"
            placeholder="name"
            value={newOutputName}
            oninput={(e) => (newOutputName = (e.currentTarget as HTMLInputElement).value)}
          />
          <select
            class="w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2 text-sm"
            value={newOutputTypeKey}
            onchange={(e) => (newOutputTypeKey = (e.currentTarget as HTMLSelectElement).value)}
          >
            {#each boundaryTypeOptions as option (option.key)}
              <option value={option.key}>{option.label}</option>
            {/each}
          </select>
        </div>
      </div>
      <div class="flex items-end">
        <button
          class="btn btn-3xs preset-filled uppercase tracking-[0.3em]"
          type="button"
          onclick={() => addPort('output')}
          disabled={!newOutputName.trim()}
        >
          Add
        </button>
      </div>
    </div>
  {/snippet}
</InspectorSection>
