<script lang="ts">
  // A thin range with its exact value beside it (scrubbable number field). `log` spreads
  // min..max evenly in log space, for spans such as 9 µs .. 476 ms (both bounds above zero).
  import Num from "./Num.svelte";
  import { fromLogFraction, toLogFraction } from "./scale.js";

  let {
    value = $bindable(0),
    min = 0,
    max = 100,
    step = 1,
    unit,
    disabled = false,
    label,
    log = false,
    onchange,
  }: {
    value?: number;
    min?: number;
    max?: number;
    step?: number;
    unit?: string;
    disabled?: boolean;
    label?: string;
    log?: boolean;
    onchange?: (value: number) => void;
  } = $props();

  // The range input runs over 0..STEPS for a log scale, so its ticks stay even on screen.
  const STEPS = 1000;
  const position = $derived(log ? toLogFraction(value, min, max) * STEPS : value);
  const fill = $derived(((position - (log ? 0 : min)) / ((log ? STEPS : max - min) || 1)) * 100);

  function input(event: Event) {
    const raw = Number((event.currentTarget as HTMLInputElement).value);
    value = log ? fromLogFraction(raw / STEPS, min, max) : raw;
    onchange?.(value);
  }
</script>

<span class="slider" class:disabled>
  <input
    type="range"
    min={log ? 0 : min}
    max={log ? STEPS : max}
    step={log ? 1 : step}
    {disabled}
    aria-label={label}
    aria-valuetext={unit ? `${value} ${unit}` : undefined}
    value={position}
    oninput={input}
    style:--fill="{fill}%"
  />
  <Num bind:value {min} {max} {step} {unit} {disabled} {label} {onchange} width={unit ? 72 : 58} />
</span>

<style>
  .slider {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-width: 0;
  }
  input[type="range"] {
    flex: 1;
    min-width: 40px;
    height: 16px;
    appearance: none;
    background: transparent;
  }
  input[type="range"]::-webkit-slider-runnable-track {
    height: 3px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--accent) var(--fill), var(--line-strong) var(--fill));
  }
  input[type="range"]::-moz-range-track {
    height: 3px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--accent) var(--fill), var(--line-strong) var(--fill));
  }
  input[type="range"]::-webkit-slider-thumb {
    appearance: none;
    width: 11px;
    height: 11px;
    margin-top: -4px;
    border-radius: 50%;
    background: var(--fg);
    border: 2px solid var(--s1);
  }
  input[type="range"]::-moz-range-thumb {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--fg);
    border: 2px solid var(--s1);
  }
  .disabled {
    opacity: 0.45;
  }
</style>
