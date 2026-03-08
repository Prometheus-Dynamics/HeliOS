<script lang="ts">
  import type { PipelineGraphNode } from '$lib/types/pipeline';
  import { SvelteSet } from 'svelte/reactivity';

  type DaedalusSyncPolicy = 'AllReady' | 'Latest' | 'ZipByTag';
  type DaedalusBackpressureStrategy = 'None' | 'BoundedQueues' | 'ErrorOnOverflow';

  export type DaedalusSyncGroup = {
    name: string;
    policy: DaedalusSyncPolicy;
    ports: string[];
    capacity?: number | null;
    backpressure?: DaedalusBackpressureStrategy | null;
  };

  const {
    node,
    nodeId,
    onChange,
    showCompute = true,
    showHeader = true,
    showSyncGroups = true
  }: {
    node: PipelineGraphNode;
    nodeId: string;
    onChange?: (payload: { nodeId: string; syncGroups: DaedalusSyncGroup[] }) => void;
    showCompute?: boolean;
    showHeader?: boolean;
    showSyncGroups?: boolean;
  } = $props();

  const readSyncGroups = (): DaedalusSyncGroup[] => {
    const groups = (node.source as { sync_groups?: unknown } | null | undefined)?.sync_groups;
    if (!Array.isArray(groups)) return [];
    return groups
      .map((group) => {
        if (!group || typeof group !== 'object') return null;
        const record = group as Record<string, unknown>;
        const name = typeof record.name === 'string' ? record.name.trim() : '';
        const policyRaw = record.policy;
        const policy: DaedalusSyncPolicy =
          policyRaw === 'Latest' || policyRaw === 'ZipByTag' || policyRaw === 'AllReady' ? policyRaw : 'AllReady';
        const ports = Array.isArray(record.ports)
          ? record.ports.filter((p): p is string => typeof p === 'string' && p.trim().length > 0)
          : [];
        const capacityRaw = record.capacity;
        const capacity = typeof capacityRaw === 'number' && Number.isFinite(capacityRaw) ? Math.max(1, Math.floor(capacityRaw)) : null;
        const backpressureRaw = record.backpressure;
        const backpressure: DaedalusBackpressureStrategy | null =
          backpressureRaw === 'BoundedQueues' || backpressureRaw === 'ErrorOnOverflow' || backpressureRaw === 'None'
            ? backpressureRaw
            : null;
        if (!name) return null;
        const entry: DaedalusSyncGroup = {
          name,
          policy,
          ports,
          ...(capacity != null ? { capacity } : {}),
          ...(backpressure != null ? { backpressure } : {})
        };
        return entry;
      })
      .filter((entry): entry is DaedalusSyncGroup => entry != null);
  };

  const portOptions = $derived(Object.keys(node.inputs ?? {}).sort((a, b) => a.localeCompare(b)));

  let groups = $derived.by<DaedalusSyncGroup[]>(() => readSyncGroups());

  const persist = () => {
    onChange?.({
      nodeId,
      syncGroups: groups.map((group) => ({
        ...group,
        ports: group.ports.slice()
      }))
    });
  };

  const addGroup = () => {
    const baseName = 'sync';
    const nextIndex = groups.length + 1;
    groups = [
      ...groups,
      {
        name: `${baseName}-${nextIndex}`,
        policy: 'AllReady',
        ports: [],
        capacity: null,
        backpressure: null
      }
    ];
    persist();
  };

  const removeGroup = (index: number) => {
    groups = groups.filter((_, i) => i !== index);
    persist();
  };

  const updateGroup = (index: number, patch: Partial<DaedalusSyncGroup>) => {
    groups = groups.map((group, i) => (i === index ? { ...group, ...patch } : group));
  };

  const togglePort = (index: number, port: string) => {
    const current = groups[index];
    if (!current) return;
    const normalized = port.trim();
    if (!normalized) return;
    const set = new SvelteSet(current.ports);
    if (set.has(normalized)) {
      set.delete(normalized);
    } else {
      set.add(normalized);
    }
    updateGroup(index, { ports: Array.from(set).sort((a, b) => a.localeCompare(b)) });
  };
</script>

<section class="space-y-3 rounded border border-surface-800/70 bg-surface-950/50 p-4">
  {#if showHeader}
    <header class="flex items-start justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.35em] text-surface-500">Daedalus</p>
        <p class="text-sm font-semibold text-white">Runtime controls</p>
        <p class="text-xs text-surface-500">Saved into the graph JSON and applied on next load.</p>
      </div>
    </header>
  {/if}

  {#if showCompute}
    <div class="flex flex-col gap-1 text-micro uppercase tracking-[0.3em] text-surface-500">
      <span>Compute affinity</span>
      <span class="rounded border border-surface-800/70 bg-surface-900/40 px-2 py-2 text-micro-tight normal-case tracking-normal text-surface-400">
        Defined by the node descriptor.
      </span>
    </div>
  {/if}

  {#if showSyncGroups}
    <div class="flex items-center justify-between gap-3 pt-2">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Sync groups</p>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={addGroup}>
        Add group
      </button>
    </div>

    {#if groups.length === 0}
      <p class="text-xs text-surface-500">No sync groups configured for this node.</p>
    {:else}
      <div class="space-y-3">
        {#each groups as group, index (`sync-${index}-${group.name}`)}
        <div class="rounded border border-surface-800/70 bg-surface-900/30 p-3 space-y-3">
          <div class="flex flex-wrap items-start justify-between gap-2">
            <div class="flex flex-col gap-1">
              <p class="text-xs font-semibold text-white">{group.name}</p>
              <p class="text-[0.7rem] text-surface-500">{group.ports.length} port{group.ports.length === 1 ? '' : 's'}</p>
            </div>
            <button
              class="btn btn-3xs preset-tonal-error uppercase tracking-[0.3em]"
              type="button"
              onclick={() => removeGroup(index)}
              aria-label="Remove sync group"
            >
              Remove
            </button>
          </div>

          <div class="grid gap-2 md:grid-cols-2">
            <label class="flex flex-col gap-1 text-micro-tight uppercase tracking-[0.28em] text-surface-500">
              <span>Name</span>
              <input
                class="input h-9"
                type="text"
                value={group.name}
                oninput={(event) => updateGroup(index, { name: (event.currentTarget as HTMLInputElement).value })}
                onblur={persist}
              />
            </label>
            <label class="flex flex-col gap-1 text-micro-tight uppercase tracking-[0.28em] text-surface-500">
              <span>Policy</span>
              <select
                class="select h-9"
                value={group.policy}
                onchange={(event) =>
                  updateGroup(index, { policy: (event.currentTarget as HTMLSelectElement).value as DaedalusSyncPolicy })}
                onblur={persist}
              >
                <option value="AllReady">AllReady</option>
                <option value="Latest">Latest</option>
                <option value="ZipByTag">ZipByTag</option>
              </select>
            </label>
            <label class="flex flex-col gap-1 text-micro-tight uppercase tracking-[0.28em] text-surface-500">
              <span>Capacity (optional)</span>
              <input
                class="input h-9"
                type="number"
                min="1"
                placeholder="inherit"
                value={group.capacity ?? ''}
                oninput={(event) => {
                  const raw = (event.currentTarget as HTMLInputElement).value;
                  const parsed = raw.trim() ? Number(raw) : null;
                  updateGroup(index, { capacity: parsed && Number.isFinite(parsed) ? Math.max(1, Math.floor(parsed)) : null });
                }}
                onblur={persist}
              />
            </label>
            <label class="flex flex-col gap-1 text-micro-tight uppercase tracking-[0.28em] text-surface-500">
              <span>Backpressure (optional)</span>
              <select
                class="select h-9"
                value={group.backpressure ?? ''}
                onchange={(event) => {
                  const value = (event.currentTarget as HTMLSelectElement).value as DaedalusBackpressureStrategy | '';
                  updateGroup(index, { backpressure: value ? value : null });
                  persist();
                }}
              >
                <option value="">inherit</option>
                <option value="None">None</option>
                <option value="BoundedQueues">BoundedQueues</option>
                <option value="ErrorOnOverflow">ErrorOnOverflow</option>
              </select>
            </label>
          </div>

          <div class="space-y-2">
            <p class="text-micro-tight uppercase tracking-[0.28em] text-surface-500">Ports</p>
            {#if portOptions.length === 0}
              <p class="text-xs text-surface-600">No input ports found on this node.</p>
            {:else}
              <div class="grid gap-2 sm:grid-cols-2">
                {#each portOptions as port (port)}
                  <label class="flex items-center justify-between gap-2 rounded border border-surface-800/60 bg-surface-950/40 px-3 py-2">
                    <span class="text-xs text-surface-300">{port}</span>
                    <input
                      class="checkbox"
                      type="checkbox"
                      checked={group.ports.includes(port)}
                      onchange={() => togglePort(index, port)}
                      onblur={persist}
                      aria-label={`Toggle port ${port}`}
                    />
                  </label>
                {/each}
              </div>
            {/if}
          </div>
        </div>
        {/each}
      </div>
    {/if}
  {/if}
</section>
