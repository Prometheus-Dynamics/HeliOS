<script lang="ts">
  import type {
    LocalizationSolverRuntimeTuningConfig,
    LocalizationTemporalStabilizationConfig
  } from '$lib/features/localization/localizationConfig';
  import type { RuntimeTuningFieldKey, RuntimeTuningGroup } from './localizationConfigEditorTypes';

  type Props = {
    hasActiveProfile?: boolean;
    localizationConfigLoading?: boolean;
    profileTemporalStabilization: LocalizationTemporalStabilizationConfig;
    activeSolverRuntimeTuning: LocalizationSolverRuntimeTuningConfig;
    runtimeTuningGroups: RuntimeTuningGroup[];
    onSetProfileTemporalEnabled?: (enabled: boolean) => void;
    onSetProfileTemporalNumeric?: (field: string, value: string) => void;
    onSetSolverRuntimeTuningNumeric?: (field: RuntimeTuningFieldKey, value: string) => void;
  };

  let {
    hasActiveProfile = false,
    localizationConfigLoading = false,
    profileTemporalStabilization,
    activeSolverRuntimeTuning,
    runtimeTuningGroups,
    onSetProfileTemporalEnabled,
    onSetProfileTemporalNumeric,
    onSetSolverRuntimeTuningNumeric
  }: Props = $props();

  function readInputValue(event: Event): string | null {
    const input = event.currentTarget;
    return input instanceof HTMLInputElement ? input.value : null;
  }

  function readInputChecked(event: Event): boolean | null {
    const input = event.currentTarget;
    return input instanceof HTMLInputElement ? input.checked : null;
  }

  function handleSetProfileTemporalEnabled(event: Event) {
    const checked = readInputChecked(event);
    if (checked != null) onSetProfileTemporalEnabled?.(checked);
  }

  function handleSetProfileTemporalNumeric(field: string, event: Event) {
    const value = readInputValue(event);
    if (value != null) onSetProfileTemporalNumeric?.(field, value);
  }

  function handleSetSolverRuntimeTuningNumeric(field: RuntimeTuningFieldKey, event: Event) {
    const value = readInputValue(event);
    if (value != null) onSetSolverRuntimeTuningNumeric?.(field, value);
  }
</script>

<section class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
  <div class="flex items-center justify-between">
    <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Advanced solver tuning</p>
  </div>
  <div class="mt-3 space-y-3">
    <details class="localization-accordion rounded border border-surface-800/70 bg-surface-950/60">
      <summary class="flex cursor-pointer items-start gap-3 px-3 py-2.5 text-left transition hover:bg-surface-900/40">
        <span class="accordion-chevron mt-[0.18rem] text-[0.7rem] text-surface-400" aria-hidden="true">▶</span>
        <div class="min-w-0 flex-1">
          <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-400">Temporal profile defaults</p>
          <p class="mt-1 text-micro text-surface-500">Base smoothing values used unless a solver override is enabled.</p>
        </div>
        <span class="rounded border border-surface-700/60 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-300">
          7 knobs
        </span>
      </summary>
      <div class="border-t border-surface-800/70 px-3 py-3">
        <label class={`flex items-center justify-between gap-3 rounded border px-3 py-2 text-micro ${
          profileTemporalStabilization.enabled
            ? 'border-primary-500/40 bg-primary-500/10 text-primary-100'
            : 'border-surface-800/70 bg-surface-950/60 text-surface-300'
        } ${!hasActiveProfile || localizationConfigLoading ? 'opacity-60' : ''}`}>
          <span class="uppercase tracking-[0.3em]">Enable smoothing</span>
          <input
            type="checkbox"
            checked={profileTemporalStabilization.enabled}
            disabled={!hasActiveProfile || localizationConfigLoading}
            onchange={handleSetProfileTemporalEnabled}
          />
        </label>
        <div class="mt-2 grid gap-2">
          <label class="grid gap-1">
            <span class="uppercase tracking-[0.3em] text-surface-500">Single-tag translation alpha</span>
            <input type="number" step="0.01" value={profileTemporalStabilization.singleTagTranslationAlpha} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetProfileTemporalNumeric('singleTagTranslationAlpha', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
          </label>
          <label class="grid gap-1">
            <span class="uppercase tracking-[0.3em] text-surface-500">Single-tag rotation alpha</span>
            <input type="number" step="0.01" value={profileTemporalStabilization.singleTagRotationAlpha} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetProfileTemporalNumeric('singleTagRotationAlpha', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
          </label>
          <label class="grid gap-1">
            <span class="uppercase tracking-[0.3em] text-surface-500">Multi-tag translation alpha</span>
            <input type="number" step="0.01" value={profileTemporalStabilization.multiTagTranslationAlpha} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetProfileTemporalNumeric('multiTagTranslationAlpha', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
          </label>
          <label class="grid gap-1">
            <span class="uppercase tracking-[0.3em] text-surface-500">Multi-tag rotation alpha</span>
            <input type="number" step="0.01" value={profileTemporalStabilization.multiTagRotationAlpha} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetProfileTemporalNumeric('multiTagRotationAlpha', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
          </label>
          <label class="grid gap-1">
            <span class="uppercase tracking-[0.3em] text-surface-500">Max jump translation (m)</span>
            <input type="number" step="0.01" value={profileTemporalStabilization.maxTranslationJumpM} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetProfileTemporalNumeric('maxTranslationJumpM', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
          </label>
          <label class="grid gap-1">
            <span class="uppercase tracking-[0.3em] text-surface-500">Max jump rotation (deg)</span>
            <input type="number" step="0.1" value={profileTemporalStabilization.maxRotationJumpDeg} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetProfileTemporalNumeric('maxRotationJumpDeg', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
          </label>
          <label class="grid gap-1">
            <span class="uppercase tracking-[0.3em] text-surface-500">Reanchor reject window (ms)</span>
            <input type="number" step="10" value={profileTemporalStabilization.reanchorRejectWindowMs} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetProfileTemporalNumeric('reanchorRejectWindowMs', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
          </label>
        </div>
      </div>
    </details>

    {#each runtimeTuningGroups as group (group.id)}
      <details class="localization-accordion rounded border border-surface-800/70 bg-surface-950/60">
        <summary class="flex cursor-pointer items-start gap-3 px-3 py-2.5 text-left transition hover:bg-surface-900/40">
          <span class="accordion-chevron mt-[0.18rem] text-[0.7rem] text-surface-400" aria-hidden="true">▶</span>
          <div class="min-w-0 flex-1">
            <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-400">{group.label}</p>
            <p class="mt-1 text-micro text-surface-500">{group.description}</p>
          </div>
          <span class="rounded border border-surface-700/60 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-300">
            {group.fields.length} knobs
          </span>
        </summary>
        <div class="grid gap-2 border-t border-surface-800/70 px-3 py-3">
          {#each group.fields as field (field.key)}
            <label class="grid gap-1">
              <span class="uppercase tracking-[0.3em] text-surface-500">{field.label}</span>
              <input
                type="number"
                step={field.step}
                value={activeSolverRuntimeTuning[field.key]}
                disabled={localizationConfigLoading}
                onchange={(event) => handleSetSolverRuntimeTuningNumeric(field.key, event)}
                class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
              />
            </label>
          {/each}
        </div>
      </details>
    {/each}
  </div>
</section>

<style>
  .localization-accordion > summary {
    list-style: none;
  }

  .localization-accordion > summary::-webkit-details-marker {
    display: none;
  }

  .localization-accordion .accordion-chevron {
    transform: rotate(0deg);
    transition: transform 150ms ease;
  }

  .localization-accordion[open] .accordion-chevron {
    transform: rotate(90deg);
  }
</style>
