<script lang="ts">
  // A thin range with its exact value beside it (scrubbable number field).
  import Num from "./Num.svelte";

  let {
    value = $bindable(0),
    min = 0,
    max = 100,
    step = 1,
    unit,
    disabled = false,
    label,
    onchange,
  }: {
    value?: number;
    min?: number;
    max?: number;
    step?: number;
    unit?: string;
    disabled?: boolean;
    label?: string;
    onchange?: (value: number) => void;
  } = $props();

  const fill = $derived(((value - min) / (max - min || 1)) * 100);
</script>

<span class="slider" class:disabled>
  <input
    type="range"
    {min}
    {max}
    {step}
    {disabled}
    aria-label={label}
    bind:value
    oninput={() => onchange?.(value)}
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
