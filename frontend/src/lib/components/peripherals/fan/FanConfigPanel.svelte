<script lang="ts">
  import type { FanConfig } from '../../../../routes/settings/types';

  type Props = {
    form: FanConfig;
    selectedPointIndex: number;
    settingsBusy: boolean;
    busy: boolean;
    dirty: boolean;
    settingsError: string | null;
    status: string | null;
    error: string | null;
    onSelectPoint: (index: number) => void;
    onAddPoint: () => void;
    onUpdatePoint: (index: number, key: 'temp_c' | 'percent', value: number) => void;
    onRemovePoint: (index: number) => void;
    onSave: () => void;
    onOpenAdvanced: () => void;
    onRetryLoad: () => void;
  };

  const {
    form,
    selectedPointIndex,
    settingsBusy,
    busy,
    dirty,
    settingsError,
    status,
    error,
    onSelectPoint,
    onAddPoint,
    onUpdatePoint,
    onRemovePoint,
    onSave,
    onOpenAdvanced,
    onRetryLoad
  }: Props = $props();
</script>

<div class="space-y-4">
  <header class="flex items-start justify-between gap-2">
    <div>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Persisted configuration</p>
      <p class="text-sm text-surface-400">Saved to the device.</p>
    </div>
    <div class="flex items-center gap-2 text-micro uppercase tracking-[0.25em] text-surface-500">
      {#if settingsBusy}
        <span>Loading…</span>
      {/if}
      <button class="btn btn-2xs preset-tonal" type="button" onclick={() => onOpenAdvanced()}>
        Advanced
      </button>
      <button class="btn btn-2xs preset-filled-primary-500" type="button" disabled={busy || !dirty} onclick={() => onSave()}>
        Save
      </button>
    </div>
  </header>

  {#if settingsError}
    <div class="rounded border border-error-500/40 bg-error-500/10 p-3 text-sm text-error-200">
      <p class="font-semibold">Unable to load saved fan settings.</p>
      <p>{settingsError}</p>
      <button class="btn btn-3xs preset-tonal mt-2 uppercase tracking-[0.25em]" type="button" onclick={() => onRetryLoad()}>
        Retry
      </button>
    </div>
  {/if}

  <div class="space-y-3">
    <div class="flex items-center justify-between gap-2">
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Curve points</p>
      <button class="btn btn-xs preset-tonal" type="button" onclick={() => onAddPoint()}>Add point</button>
    </div>
    <div class="overflow-auto">
      <table class="min-w-full text-sm">
        <thead>
          <tr class="border-b border-surface-800 text-micro uppercase tracking-[0.25em] text-surface-500">
            <th class="px-2 py-1 text-left">Temp (°C)</th>
            <th class="px-2 py-1 text-left">Duty (%)</th>
            <th class="px-2 py-1 text-left"></th>
          </tr>
        </thead>
        <tbody class="divide-y divide-surface-800">
          {#each form.curve as point, idx (idx)}
            <tr class={idx === selectedPointIndex ? 'bg-primary-500/5' : ''} onclick={() => onSelectPoint(idx)}>
              <td class="px-2 py-2">
                <input
                  class="input w-full"
                  type="number"
                  min="0"
                  max="120"
                  step="0.01"
                  value={point.temp_c}
                  oninput={(event) => onUpdatePoint(idx, 'temp_c', Number((event.target as HTMLInputElement).value))}
                  disabled={idx === 0 || idx === form.curve.length - 1}
                />
              </td>
              <td class="px-2 py-2">
                <input
                  class="input w-full"
                  type="number"
                  min="0"
                  max="100"
                  value={point.percent}
                  oninput={(event) => onUpdatePoint(idx, 'percent', Number((event.target as HTMLInputElement).value))}
                />
              </td>
              <td class="px-2 py-2 text-right">
                <button
                  class="btn btn-xs preset-tonal"
                  type="button"
                  onclick={() => onRemovePoint(idx)}
                  disabled={form.curve.length <= 2 || idx === 0 || idx === form.curve.length - 1}
                >
                  Remove
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="text-xs text-surface-500">Controller linearly interpolates between points.</p>
  </div>

  {#if error}
    <p class="text-xs text-error-400">{error}</p>
  {/if}
  {#if status}
    <p class="text-xs text-success-400">{status}</p>
  {/if}
  <div class="flex flex-wrap items-center justify-between gap-3 text-xs text-surface-500">
    <span>{busy ? 'Saving…' : dirty ? 'Unsaved changes' : 'Saved'}</span>
  </div>
</div>
