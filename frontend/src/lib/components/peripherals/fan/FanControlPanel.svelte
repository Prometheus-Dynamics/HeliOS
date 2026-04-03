<script lang="ts">
  import FanCurveEditor from '../../charts/FanCurveEditor.svelte';
  import type { FanConfig, FanCurvePoint } from '../../../../routes/settings/types';

  type Props = {
    form: FanConfig;
    mode: 'fixed' | 'curve' | 'disabled';
    displayMode: 'fixed' | 'curve' | 'disabled' | null;
    currentTemp: number | null;
    selectedPointIndex: number;
    manualInput: string;
    fixedPercent: number;
    tempMin: number;
    tempMax: number;
    onSelectPoint: (index: number) => void;
    onCurveChange: (curve: FanCurvePoint[]) => void;
    onControlModeChange: (next: 'curve' | 'fixed') => void;
    onManualInput: (value: string) => void;
    onFixedPercentChange: (value: number) => void;
  };

  const {
    form,
    mode,
    displayMode,
    currentTemp,
    selectedPointIndex,
    manualInput,
    fixedPercent,
    tempMin,
    tempMax,
    onSelectPoint,
    onCurveChange,
    onControlModeChange,
    onManualInput,
    onFixedPercentChange
  }: Props = $props();
</script>

<div class="space-y-3">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <div>
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Control mode</p>
      <p class="text-xs text-surface-500">
        {displayMode === 'fixed' ? 'Manual duty override' : displayMode === 'disabled' ? 'Writes paused' : 'Follows curve'}
      </p>
    </div>
    <div class="flex items-center gap-2 text-micro uppercase tracking-[0.25em]">
      <button
        type="button"
        class={`rounded px-2 py-1 ${displayMode === 'curve' ? 'bg-success-500/15 text-success-100' : 'bg-surface-800 text-surface-400'}`}
        disabled={!form.enabled}
        onclick={() => onControlModeChange('curve')}
      >
        Curve
      </button>
      <button
        type="button"
        class={`rounded px-2 py-1 ${displayMode === 'fixed' ? 'bg-primary-500/20 text-primary-50' : 'bg-surface-800 text-surface-400'}`}
        disabled={!form.enabled}
        onclick={() => onControlModeChange('fixed')}
      >
        Fixed
      </button>
    </div>
  </div>

  <FanCurveEditor
    curve={form.curve}
    tone="primary"
    tempMin={tempMin}
    tempMax={tempMax}
    currentTemp={currentTemp}
    selectedIndex={selectedPointIndex}
    onSelect={(index) => onSelectPoint(index)}
    onCurveChange={(curve) => onCurveChange(curve)}
  />

  {#if mode === 'fixed' && form.enabled}
    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Fixed duty</p>
      </div>
      <div class="grid gap-2 md:grid-cols-[1fr_auto] md:items-center">
        <input
          class="input w-full"
          type="number"
          min="0"
          max="100"
          value={manualInput || String(fixedPercent)}
          oninput={(event) => {
            const value = event.currentTarget.value;
            onManualInput(value);
            onFixedPercentChange(Number(value));
          }}
        />
        <input
          class="range range-primary"
          type="range"
          min="0"
          max="100"
          step="1"
          value={manualInput || String(fixedPercent)}
          oninput={(event) => {
            const value = event.currentTarget.value;
            onManualInput(value);
            onFixedPercentChange(Number(value));
          }}
        />
      </div>
      <p class="text-xs text-surface-500">Saves as a manual_percent override.</p>
    </div>
  {/if}
</div>
