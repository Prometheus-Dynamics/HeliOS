<script lang="ts">
  import { createEventDispatcher } from 'svelte';

  type Props = {
    min?: number;
    max?: number;
    step?: number;
    valueMin?: number;
    valueMax?: number;
    disabled?: boolean;
    gradient?: string;
  };

  let {
    min = 0,
    max = 100,
    step = undefined,
    valueMin = 0,
    valueMax = 100,
    disabled = false,
    gradient = 'var(--color-surface-800)'
  }: Props = $props();

  let internalMin = $state(0);
  let internalMax = $state(0);

  const dispatch = createEventDispatcher<{ change: { min: number; max: number } }>();

  const clamp = (value: number) => Math.min(max, Math.max(min, value));

  $effect(() => {
    const minBound = min;
    const maxBound = max;
    internalMin = Math.min(maxBound, Math.max(minBound, valueMin));
    internalMax = Math.min(maxBound, Math.max(minBound, valueMax));
  });

  const emitChange = (nextMin: number, nextMax: number) => {
    dispatch('change', { min: nextMin, max: nextMax });
  };

  const updateMin = (event: Event) => {
    const nextRaw = Number((event.currentTarget as HTMLInputElement).value);
    let next = clamp(nextRaw);
    if (next > internalMax) next = internalMax;
    internalMin = next;
    emitChange(internalMin, internalMax);
  };

  const updateMax = (event: Event) => {
    const nextRaw = Number((event.currentTarget as HTMLInputElement).value);
    let next = clamp(nextRaw);
    if (next < internalMin) next = internalMin;
    internalMax = next;
    emitChange(internalMin, internalMax);
  };

  const percent = (value: number) => {
    if (max <= min) return 0;
    return ((value - min) / (max - min)) * 100;
  };

  const minPercent = $derived(Math.min(100, Math.max(0, percent(internalMin))));
  const maxPercent = $derived(Math.min(100, Math.max(0, percent(internalMax))));
  const rangeLeft = $derived(Math.min(minPercent, maxPercent));
  const rangeRight = $derived(Math.max(minPercent, maxPercent));
  const minThumbZ = $derived(minPercent > 90 ? 4 : 3);
  const maxThumbZ = $derived(minPercent > 90 ? 3 : 4);
</script>

<div
  class={`range-band ${disabled ? 'range-band--disabled' : ''}`}
  style={`--range-gradient: ${gradient}; --range-min: ${rangeLeft}; --range-max: ${rangeRight};`}
>
  <div class="range-band__track"></div>
  <div class="range-band__selection"></div>
  <input
    class="range-band__input range-band__input--min"
    type="range"
    min={min}
    max={max}
    step={step}
    value={internalMin}
    oninput={updateMin}
    disabled={disabled}
    style={`z-index: ${minThumbZ};`}
  />
  <input
    class="range-band__input range-band__input--max"
    type="range"
    min={min}
    max={max}
    step={step}
    value={internalMax}
    oninput={updateMax}
    disabled={disabled}
    style={`z-index: ${maxThumbZ};`}
  />
</div>

<style>
  .range-band {
    position: relative;
    height: 28px;
  }

  .range-band__track {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 8px;
    transform: translateY(-50%);
    background: var(--range-gradient);
    border-radius: 999px;
    border: 1px solid var(--color-surface-700);
  }

  .range-band__selection {
    position: absolute;
    top: 50%;
    height: 8px;
    transform: translateY(-50%);
    left: calc(var(--range-min) * 1%);
    right: calc(100% - (var(--range-max) * 1%));
    background: var(--color-primary-500);
    opacity: 0.55;
    border-radius: 999px;
    pointer-events: none;
  }

  .range-band__input {
    position: absolute;
    inset: 0;
    width: 100%;
    background: none;
    pointer-events: none;
    appearance: none;
    outline: none;
  }

  .range-band__input:focus,
  .range-band__input:focus-visible {
    outline: none;
  }

  .range-band__input::-webkit-slider-thumb {
    appearance: none;
    pointer-events: auto;
    width: 16px;
    height: 16px;
    border-radius: 999px;
    background: var(--color-surface-50);
    border: 2px solid var(--color-primary-400);
    cursor: pointer;
  }

  .range-band__input::-moz-range-thumb {
    pointer-events: auto;
    width: 16px;
    height: 16px;
    border-radius: 999px;
    background: var(--color-surface-50);
    border: 2px solid var(--color-primary-400);
    cursor: pointer;
  }

  .range-band__input::-moz-focus-outer {
    border: 0;
  }

  .range-band__input::-webkit-slider-runnable-track {
    background: transparent;
    height: 8px;
  }

  .range-band__input::-moz-range-track {
    background: transparent;
    height: 8px;
  }

  .range-band--disabled {
    opacity: 0.55;
  }

  .range-band--disabled .range-band__input::-webkit-slider-thumb {
    cursor: not-allowed;
  }

  .range-band--disabled .range-band__input::-moz-range-thumb {
    cursor: not-allowed;
  }
</style>
