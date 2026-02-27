<script lang="ts">
  import type { PipelineGraphPlan } from '$lib/types/pipeline';

  const props = $props<{
    plan: PipelineGraphPlan;
    onChange: (plan: PipelineGraphPlan) => void;
    open?: boolean;
    showHeader?: boolean;
  }>();

  const META = {
    gpuBackend: 'helios.daedalus.gpu_backend',
    plannerEnableGpu: 'helios.daedalus.planner.enable_gpu',
    plannerEnableLints: 'helios.daedalus.planner.enable_lints',
    runtimeMode: 'helios.daedalus.runtime.mode',
    runtimePoolSize: 'helios.daedalus.runtime.pool_size',
    runtimeDefaultPolicy: 'helios.daedalus.runtime.default_policy',
    runtimeBackpressure: 'helios.daedalus.runtime.backpressure',
    runtimeLockfreeQueues: 'helios.daedalus.runtime.lockfree_queues'
  } as const;

  let open = $state(Boolean(props.open));
  const showHeader = $derived(props.showHeader ?? true);

  $effect(() => {
    if (typeof props.open === 'boolean') {
      open = props.open;
    }
  });

  const metadata = $derived((props.plan.daedalus?.metadata ?? {}) as Record<string, string>);

  const read = (key: string, fallback = ''): string => {
    const value = metadata[key];
    return typeof value === 'string' ? value : fallback;
  };

  const readBool = (key: string, fallback: boolean): boolean => {
    const raw = read(key, fallback ? 'true' : 'false').trim().toLowerCase();
    return raw === '1' || raw === 'true' || raw === 'yes' || raw === 'on';
  };

  const updateMeta = (patch: Record<string, string | null | undefined>) => {
    const nextMeta: Record<string, string> = { ...(props.plan.daedalus?.metadata ?? {}) };
    for (const [key, value] of Object.entries(patch)) {
      if (value == null || value.trim().length === 0) {
        delete nextMeta[key];
      } else {
        nextMeta[key] = value;
      }
    }
    props.onChange({
      ...props.plan,
      format: 'daedalus',
      daedalus: { metadata: nextMeta }
    });
  };

  let gpuBackend = $state(read(META.gpuBackend, 'cpu'));
  let enableGpu = $state(readBool(META.plannerEnableGpu, false));
  let enableLints = $state(readBool(META.plannerEnableLints, true));
  let runtimeMode = $state(read(META.runtimeMode, 'serial'));
  let poolSize = $state(read(META.runtimePoolSize, ''));
  let backpressure = $state(read(META.runtimeBackpressure, 'none'));
  let defaultPolicy = $state(read(META.runtimeDefaultPolicy, 'fifo'));
  let lockfreeQueues = $state(readBool(META.runtimeLockfreeQueues, false));
  let boundedCap = $state('');

  $effect(() => {
    // Sync local draft state when plan changes.
    gpuBackend = read(META.gpuBackend, 'cpu');
    enableGpu = readBool(META.plannerEnableGpu, false);
    enableLints = readBool(META.plannerEnableLints, true);
    runtimeMode = read(META.runtimeMode, 'serial');
    poolSize = read(META.runtimePoolSize, '');
    backpressure = read(META.runtimeBackpressure, 'none');
    defaultPolicy = read(META.runtimeDefaultPolicy, 'fifo');
    lockfreeQueues = readBool(META.runtimeLockfreeQueues, false);

    const match = defaultPolicy.trim().toLowerCase().match(/^bounded:(\\d+)$/);
    boundedCap = match ? match[1] : '';
  });

  const apply = () => {
    const policy =
      defaultPolicy === 'bounded' && boundedCap.trim().length
        ? `bounded:${Math.max(1, Number(boundedCap) || 1)}`
        : defaultPolicy;
    updateMeta({
      [META.gpuBackend]: gpuBackend,
      [META.plannerEnableGpu]: String(enableGpu),
      [META.plannerEnableLints]: String(enableLints),
      [META.runtimeMode]: runtimeMode,
      [META.runtimePoolSize]: poolSize,
      [META.runtimeBackpressure]: backpressure,
      [META.runtimeDefaultPolicy]: policy,
      [META.runtimeLockfreeQueues]: String(lockfreeQueues),
      // Remove deprecated Daedalus config keys that no longer exist upstream.
      'helios.daedalus.bundles': null,
      'helios.daedalus.features.metrics': null,
      'helios.daedalus.features.snapshots': null,
      'helios.daedalus.features.payload_value': null,
      'helios.daedalus.features.lockfree_queues': null,
      'helios.daedalus.features.executor_pool': null
    });
  };
</script>

<div class="rounded border border-surface-800/80 bg-surface-950/60 p-3">
  {#if showHeader}
    <button
      type="button"
      class="flex w-full items-center justify-between gap-3 text-left"
      onclick={() => {
        if (typeof props.open === 'boolean') return;
        open = !open;
      }}
    >
      <div>
        <p class="text-micro-tight uppercase tracking-[0.28em] text-surface-500">Daedalus</p>
        <p class="text-sm font-semibold text-white">Engine config</p>
      </div>
      <span class="text-xs text-surface-400">{open ? 'Hide' : 'Show'}</span>
    </button>
  {/if}

  {#if open}
    <div class="mt-3 grid gap-3 md:grid-cols-2">
      <label class="flex flex-col gap-1 text-[0.56rem] uppercase tracking-[0.26em] text-surface-500">
        <span>GPU backend</span>
        <select class="select h-9" bind:value={gpuBackend} onchange={apply}>
          <option value="cpu">cpu</option>
          <option value="mock">mock</option>
          <option value="device">device</option>
        </select>
      </label>

      <label class="flex flex-col gap-1 text-[0.56rem] uppercase tracking-[0.26em] text-surface-500">
        <span>Runtime mode</span>
        <select class="select h-9" bind:value={runtimeMode} onchange={apply}>
          <option value="serial">serial</option>
          <option value="parallel">parallel</option>
        </select>
      </label>

      <label class="flex flex-col gap-1 text-[0.56rem] uppercase tracking-[0.26em] text-surface-500">
        <span>Pool size (optional)</span>
        <input class="input h-9" type="number" min="1" placeholder="unset" bind:value={poolSize} onblur={apply} />
      </label>

      <label class="flex flex-col gap-1 text-[0.56rem] uppercase tracking-[0.26em] text-surface-500">
        <span>Backpressure</span>
        <select class="select h-9" bind:value={backpressure} onchange={apply}>
          <option value="none">none</option>
          <option value="bounded">bounded</option>
          <option value="error_on_overflow">error_on_overflow</option>
        </select>
      </label>

      <label class="flex flex-col gap-1 text-[0.56rem] uppercase tracking-[0.26em] text-surface-500">
        <span>Default edge policy</span>
        <select class="select h-9" bind:value={defaultPolicy} onchange={apply}>
          <option value="fifo">fifo</option>
          <option value="newest_wins">newest_wins</option>
          <option value="broadcast">broadcast</option>
          <option value="bounded">bounded</option>
        </select>
      </label>

      {#if defaultPolicy === 'bounded'}
        <label class="flex flex-col gap-1 text-[0.56rem] uppercase tracking-[0.26em] text-surface-500">
          <span>Bounded cap</span>
          <input class="input h-9" type="number" min="1" placeholder="64" bind:value={boundedCap} onblur={apply} />
        </label>
      {/if}

      <div class="md:col-span-2 grid gap-2">
        <label class="flex items-center justify-between gap-3 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
          <span class="text-xs uppercase tracking-[0.22em] text-surface-400">Planner GPU pass</span>
          <input class="toggle" type="checkbox" bind:checked={enableGpu} onchange={apply} />
        </label>
        <label class="flex items-center justify-between gap-3 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
          <span class="text-xs uppercase tracking-[0.22em] text-surface-400">Planner lints</span>
          <input class="toggle" type="checkbox" bind:checked={enableLints} onchange={apply} />
        </label>
        <label class="flex items-center justify-between gap-3 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
          <span class="text-xs uppercase tracking-[0.22em] text-surface-400">Lock-free queues</span>
          <input class="toggle" type="checkbox" bind:checked={lockfreeQueues} onchange={apply} />
        </label>
      </div>

    </div>
  {/if}
</div>
