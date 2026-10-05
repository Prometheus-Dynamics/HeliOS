<script lang="ts">
  // A number field: type, arrow keys (Shift ×10), wheel, or drag the label
  // area horizontally to scrub. Clamped to min/max, snapped to step.
  let {
    value = $bindable(0),
    min = -Infinity,
    max = Infinity,
    step = 1,
    unit,
    width = 64,
    disabled = false,
    label,
    onchange,
  }: {
    value?: number;
    min?: number;
    max?: number;
    step?: number;
    unit?: string;
    width?: number;
    disabled?: boolean;
    label?: string;
    onchange?: (value: number) => void;
  } = $props();

  const decimals = $derived(step >= 1 ? 0 : Math.min(4, Math.ceil(-Math.log10(step))));
  let text = $state("");
  let editing = $state(false);
  $effect(() => {
    if (!editing) text = Number(value).toFixed(decimals);
  });

  function commit(next: number) {
    const snapped = Math.round(next / step) * step;
    const clamped = Math.min(max, Math.max(min, Number(snapped.toFixed(decimals))));
    if (clamped !== value) {
      value = clamped;
      onchange?.(clamped);
    }
    text = clamped.toFixed(decimals);
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      event.preventDefault();
      commit(value + (event.key === "ArrowUp" ? 1 : -1) * step * (event.shiftKey ? 10 : 1));
    } else if (event.key === "Enter") {
      (event.target as HTMLInputElement).blur();
    } else if (event.key === "Escape") {
      text = Number(value).toFixed(decimals);
      (event.target as HTMLInputElement).blur();
    }
  }

  let scrub: { x: number; start: number } | null = null;
  function down(event: PointerEvent) {
    if (disabled) return;
    scrub = { x: event.clientX, start: value };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }
  function move(event: PointerEvent) {
    if (!scrub) return;
    const dx = event.clientX - scrub.x;
    const range = Number.isFinite(max - min) ? (max - min) / 300 : step;
    commit(scrub.start + Math.round(dx) * Math.max(step, range) * (event.shiftKey ? 0.1 : 1));
  }
  function up() {
    scrub = null;
  }
</script>

<span class="num" class:disabled style:width="{width}px">
  <span class="grip" onpointerdown={down} onpointermove={move} onpointerup={up} role="presentation" title="Drag to change"></span>
  <input
    type="text"
    inputmode="decimal"
    bind:value={text}
    {disabled}
    aria-label={label}
    onfocus={() => (editing = true)}
    onblur={() => {
      editing = false;
      const parsed = Number(text);
      if (Number.isFinite(parsed)) commit(parsed);
      else text = Number(value).toFixed(decimals);
    }}
    {onkeydown}
    onwheel={(e) => {
      if (document.activeElement !== e.currentTarget) return;
      e.preventDefault();
      commit(value + (e.deltaY < 0 ? 1 : -1) * step);
    }}
  />
  {#if unit}<span class="unit">{unit}</span>{/if}
</span>

<style>
  .num {
    position: relative;
    display: inline-flex;
    align-items: center;
    height: calc(var(--row) - 4px);
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    flex-shrink: 0;
  }
  .num:focus-within {
    border-color: var(--accent-ring);
  }
  .grip {
    width: 8px;
    align-self: stretch;
    cursor: ew-resize;
    flex-shrink: 0;
  }
  .grip:hover {
    background: var(--s3);
  }
  input {
    width: 0;
    min-width: 0;
    flex: 1;
    height: 100%;
    padding: 0 4px 0 0;
    background: transparent;
    border: 0;
    outline: 0;
    color: var(--fg);
    font-family: var(--font-code);
    font-size: 11.5px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .unit {
    padding-right: 5px;
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .disabled {
    opacity: 0.45;
  }
</style>
