<script lang="ts">
  let {
    controls,
    controlsQuery = $bindable(),
    showReadOnlyControls = $bindable(false),
    controlState = $bindable(),
    controlAppliedState = $bindable(),
    controlBusy = $bindable(),
    filteredControls,
    menuOptions,
    applyControl,
    scheduleControlApply,
    displayValue,
    extractValue,
    controlMin,
    controlMax,
    controlStep,
    accessLabel,
    accessBadgeClass,
    compact = false
  } = $props();

  type AccessFilter = 'all' | 'writable' | 'readonly';

  let accessFilter = $state<AccessFilter>('all');

  const queryControls = $derived.by(() =>
    typeof filteredControls === 'function' ? filteredControls() : []
  );

  const controlCounts = $derived.by(() => {
    const total = Array.isArray(controls) ? controls.length : 0;
    const readonly = Array.isArray(controls)
      ? controls.filter((ctrl) => ctrl?.access === 'ReadOnly').length
      : 0;
    return {
      total,
      readonly,
      writable: Math.max(0, total - readonly)
    };
  });

  const visibleControls = $derived.by(() => {
    const filtered = Array.isArray(queryControls) ? queryControls : [];
    const accessFiltered =
      accessFilter === 'writable'
        ? filtered.filter((ctrl) => ctrl?.access !== 'ReadOnly')
        : accessFilter === 'readonly'
          ? filtered.filter((ctrl) => ctrl?.access === 'ReadOnly')
          : filtered;

    return [...accessFiltered].sort((a, b) => {
      const aReadOnly = a?.access === 'ReadOnly' ? 1 : 0;
      const bReadOnly = b?.access === 'ReadOnly' ? 1 : 0;
      if (aReadOnly !== bReadOnly) return aReadOnly - bReadOnly;
      return String(a?.name ?? '').localeCompare(String(b?.name ?? ''));
    });
  });

  const hasSearchQuery = $derived.by(() => String(controlsQuery ?? '').trim().length > 0);

  function setAccessFilter(next: AccessFilter): void {
    accessFilter = next;
    if (next === 'readonly') {
      showReadOnlyControls = true;
    }
  }

  function clearSearch(): void {
    controlsQuery = '';
  }

  const filterButtonClass = (active: boolean): string => {
    if (compact) {
      return `rounded border px-2 py-1 text-micro-tight uppercase tracking-[0.14em] transition ${
        active
          ? 'border-primary-500/60 bg-primary-500/20 text-primary-100'
          : 'border-surface-700 bg-surface-900/60 text-surface-300 hover:border-primary-400/60 hover:text-primary-200'
      }`;
    }
    return `btn btn-3xs uppercase tracking-[0.22em] ${active ? 'preset-filled-primary-500' : 'preset-tonal'}`;
  };
</script>

<div class="space-y-3">
  <div class="rounded-xl border border-surface-800/70 bg-surface-950/60 p-3">
    <label class="block min-w-0">
      <div class="flex items-center gap-2 rounded border border-surface-700 bg-surface-900/70 px-2 py-1.5">
        <input
          class={`min-w-0 flex-1 bg-transparent text-surface-100 outline-none ${compact ? 'text-xs' : 'text-sm'}`}
          type="search"
          value={controlsQuery}
          placeholder="Exposure, white balance, gain…"
          oninput={(e) => (controlsQuery = e.currentTarget.value)}
        />
        {#if hasSearchQuery}
          <button class="btn btn-3xs preset-tonal uppercase tracking-[0.22em]" type="button" onclick={clearSearch}>
            Clear
          </button>
        {/if}
      </div>
    </label>

    <div class="mt-2 flex flex-wrap gap-1.5">
      <button
        type="button"
        class={filterButtonClass(accessFilter === 'all')}
        onclick={() => setAccessFilter('all')}
      >
        All ({controlCounts.total})
      </button>
      <button
        type="button"
        class={filterButtonClass(accessFilter === 'writable')}
        onclick={() => setAccessFilter('writable')}
      >
        Writable ({controlCounts.writable})
      </button>
      <button
        type="button"
        class={filterButtonClass(accessFilter === 'readonly')}
        onclick={() => setAccessFilter('readonly')}
      >
        Read-only ({controlCounts.readonly})
      </button>
    </div>
  </div>

  {#if !controls.length}
    <div class={`rounded border border-dashed border-surface-700/60 bg-surface-900/60 px-3 py-2 text-surface-400 ${compact ? 'text-xs' : 'text-sm'}`}>
      No controls surfaced by the engine for this stream.
    </div>
  {:else if !visibleControls.length}
    <div class={`rounded border border-dashed border-surface-700/60 bg-surface-900/60 px-3 py-2 text-surface-400 ${compact ? 'text-xs' : 'text-sm'}`}>
      No controls match your current search/filter.
    </div>
  {:else}
    <div class="grid gap-2.5">
      {#each visibleControls as ctrl (ctrl.id)}
        {@const min = controlMin(ctrl)}
        {@const max = controlMax(ctrl)}
        {@const step = controlStep(ctrl)}
        {@const value = controlState[ctrl.id] ?? null}
        {@const defaultValue = extractValue(ctrl.default)}
        <div class="rounded-xl border border-surface-800/70 bg-surface-950/70 p-3 transition hover:border-primary-500/35">
          <div class="min-w-0">
            <p class={`truncate font-semibold text-surface-50 ${compact ? 'text-xs leading-4' : 'text-sm leading-5'}`}>{ctrl.name}</p>
            <div class="mt-1 flex flex-wrap items-center gap-1.5 text-[0.62rem] uppercase tracking-[0.22em] text-surface-500">
              <span class="rounded border border-surface-800/80 bg-surface-900/40 px-2 py-0.5">{ctrl.kind}</span>
              <span class={`rounded border px-2 py-0.5 ${accessBadgeClass(ctrl.access)}`}>
                {accessLabel(ctrl.access)}
              </span>
              {#if controlBusy[ctrl.id]}
                <span class="rounded border border-primary-500/40 bg-primary-500/10 px-2 py-0.5 text-primary-100">
                  Applying…
                </span>
              {/if}
            </div>
          </div>

          <div class="mt-3 grid gap-2 sm:grid-cols-3">
            <div class="rounded border border-surface-800/60 bg-surface-900/40 px-2.5 py-2 text-micro-tight text-surface-400">
              <div class="uppercase tracking-[0.22em] text-surface-500">Min</div>
              <div class="mt-0.5 truncate text-surface-200">{min == null ? '—' : displayValue(ctrl, min)}</div>
            </div>

            {#if ctrl.access === 'ReadOnly'}
              <div class="rounded border border-surface-800/60 bg-surface-900/40 px-2.5 py-2 text-micro-tight text-surface-400">
                <div class="uppercase tracking-[0.22em] text-surface-500">Default</div>
                <div class="mt-0.5 truncate text-surface-200">{displayValue(ctrl, defaultValue)}</div>
              </div>
            {:else if defaultValue != null}
              <button
                type="button"
                class="w-full rounded border border-surface-800/60 bg-surface-900/40 px-2.5 py-2 text-left text-micro-tight text-surface-300 transition hover:border-primary-500/50 hover:text-primary-100"
                onclick={() => {
                  controlState = { ...controlState, [ctrl.id]: defaultValue };
                  void applyControl(ctrl, defaultValue);
                }}
                title="Apply default value"
              >
                <div class="uppercase tracking-[0.22em] text-surface-500">Default</div>
                <div class="mt-0.5 truncate text-surface-100">{displayValue(ctrl, defaultValue)}</div>
              </button>
            {:else}
              <div class="rounded border border-surface-800/60 bg-surface-900/40 px-2.5 py-2 text-micro-tight text-surface-400">
                <div class="uppercase tracking-[0.22em] text-surface-500">Default</div>
                <div class="mt-0.5 truncate text-surface-200">—</div>
              </div>
            {/if}

            <div class="rounded border border-surface-800/60 bg-surface-900/40 px-2.5 py-2 text-micro-tight text-surface-400">
              <div class="uppercase tracking-[0.22em] text-surface-500">Max</div>
              <div class="mt-0.5 truncate text-surface-200">{max == null ? '—' : displayValue(ctrl, max)}</div>
            </div>
          </div>

          <div class="mt-3">
            {#if ctrl.kind === 'Bool'}
              <div class="grid grid-cols-2 gap-2">
                <button
                  class={`rounded border px-3 py-2 uppercase tracking-[0.22em] transition ${compact ? 'text-micro-tight' : 'text-xs'} ${
                    value === false
                      ? 'border-primary-500/60 bg-primary-500/15 text-primary-100'
                      : 'border-surface-700 bg-surface-900/40 text-surface-300 hover:border-primary-500/50'
                  }`}
                  type="button"
                  disabled={ctrl.access === 'ReadOnly'}
                  onclick={() => applyControl(ctrl, false)}
                >
                  Off
                </button>
                <button
                  class={`rounded border px-3 py-2 uppercase tracking-[0.22em] transition ${compact ? 'text-micro-tight' : 'text-xs'} ${
                    value === true
                      ? 'border-primary-500/60 bg-primary-500/15 text-primary-100'
                      : 'border-surface-700 bg-surface-900/40 text-surface-300 hover:border-primary-500/50'
                  }`}
                  type="button"
                  disabled={ctrl.access === 'ReadOnly'}
                  onclick={() => applyControl(ctrl, true)}
                >
                  On
                </button>
              </div>
            {:else if ctrl.kind === 'Menu' || ctrl.kind === 'IntMenu'}
              <label class="block">
                <span class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">Select</span>
                <select
                  class={`mt-1 w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2 ${compact ? 'text-xs' : 'text-sm'}`}
                  value={Number(value ?? 0)}
                  disabled={ctrl.access === 'ReadOnly'}
                  onchange={(e) => {
                    const next = Number(e.currentTarget.value);
                    controlState = { ...controlState, [ctrl.id]: next };
                    void applyControl(ctrl, next);
                  }}
                >
                  {#each menuOptions(ctrl) as opt (opt.value)}
                    {@const optLabel = String(opt.label ?? '').trim()}
                    {@const optNumeric = String(opt.value)}
                    <option value={opt.value}>
                      {!optLabel.length || optLabel === optNumeric ? `[${optNumeric}]` : `[${optNumeric}] - ${optLabel}`}
                    </option>
                  {/each}
                </select>
              </label>
            {:else}
              <div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_126px] sm:items-end">
                <label>
                  <span class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">Adjust</span>
                  <input
                    class="mt-1 w-full accent-primary-500"
                    type="range"
                    min={min ?? 0}
                    max={max ?? 100}
                    step={step ?? (ctrl.kind === 'Float' ? 0.01 : 1)}
                    value={Number(value ?? min ?? 0)}
                    disabled={ctrl.access === 'ReadOnly' || min == null || max == null}
                    oninput={(e) => {
                      const next = Number(e.currentTarget.value);
                      controlState = { ...controlState, [ctrl.id]: next };
                      scheduleControlApply?.(ctrl, next);
                    }}
                    onchange={(e) => {
                      const next = Number(e.currentTarget.value);
                      controlState = { ...controlState, [ctrl.id]: next };
                      void applyControl(ctrl, next, { silent: true });
                    }}
                  />
                </label>

                <label>
                  <span class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">Value</span>
                  <input
                    class={`mt-1 w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2 ${compact ? 'text-xs' : 'text-sm'}`}
                    type="number"
                    min={min ?? undefined}
                    max={max ?? undefined}
                    step={step ?? (ctrl.kind === 'Float' ? 0.01 : 1)}
                    value={value ?? ''}
                    disabled={ctrl.access === 'ReadOnly'}
                    oninput={(e) => {
                      const raw = e.currentTarget.value;
                      const next = raw.trim().length ? Number(raw) : null;
                      controlState = { ...controlState, [ctrl.id]: next };
                      scheduleControlApply?.(ctrl, next);
                    }}
                    onchange={(e) => {
                      const raw = e.currentTarget.value;
                      const next = raw.trim().length ? Number(raw) : null;
                      controlState = { ...controlState, [ctrl.id]: next };
                      void applyControl(ctrl, next, { silent: true });
                    }}
                  />
                </label>
              </div>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
