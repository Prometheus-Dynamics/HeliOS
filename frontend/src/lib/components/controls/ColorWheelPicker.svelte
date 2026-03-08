<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';

  type HSV = { h: number; s: number; v: number; a?: number };

  type Props = {
    value?: string;
    size?: number;
    disabled?: boolean;
  };

  let { value = '#ffffff', size = 160, disabled = false }: Props = $props();

  const dispatch = createEventDispatcher<{ change: { value: string; hsv: HSV } }>();
  const HEX_COLOR_PATTERN = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;

  let canvas: HTMLCanvasElement | null = null;
  let hue = $state(0);
  let sat = $state(0);
  let val = $state(1);
  let alpha = $state(1);
  let dragging = $state(false);
  let lastPropValue = '';

  function clamp(value: number, min: number, max: number): number {
    return Math.min(max, Math.max(min, value));
  }

  function normalizeHex(input: string, fallback: string): string {
    return HEX_COLOR_PATTERN.test(input) ? input : fallback;
  }

  function hexToRgba(hex: string): { r: number; g: number; b: number; a: number } {
    const cleaned = normalizeHex(hex, '#ffffff').replace('#', '');
    const isShort = cleaned.length === 3 || cleaned.length === 4;
    const hasAlpha = cleaned.length === 4 || cleaned.length === 8;
    const r = parseInt(isShort ? cleaned[0] + cleaned[0] : cleaned.slice(0, 2), 16);
    const g = parseInt(isShort ? cleaned[1] + cleaned[1] : cleaned.slice(2, 4), 16);
    const b = parseInt(isShort ? cleaned[2] + cleaned[2] : cleaned.slice(4, 6), 16);
    const a = hasAlpha
      ? parseInt(isShort ? cleaned[3] + cleaned[3] : cleaned.slice(6, 8), 16) / 255
      : 1;
    return { r, g, b, a };
  }

  function rgbToHsv(r: number, g: number, b: number): HSV {
    const rn = r / 255;
    const gn = g / 255;
    const bn = b / 255;
    const max = Math.max(rn, gn, bn);
    const min = Math.min(rn, gn, bn);
    const delta = max - min;
    let h = 0;
    if (delta > 0) {
      if (max === rn) {
        h = ((gn - bn) / delta) % 6;
      } else if (max === gn) {
        h = (bn - rn) / delta + 2;
      } else {
        h = (rn - gn) / delta + 4;
      }
      h *= 60;
      if (h < 0) h += 360;
    }
    const s = max === 0 ? 0 : delta / max;
    return { h, s, v: max };
  }

  function hsvToRgb(h: number, s: number, v: number): { r: number; g: number; b: number } {
    const hue = ((h % 360) + 360) % 360;
    const sat = clamp(s, 0, 1);
    const valc = clamp(v, 0, 1);
    const c = valc * sat;
    const x = c * (1 - Math.abs(((hue / 60) % 2) - 1));
    const m = valc - c;
    let r = 0;
    let g = 0;
    let b = 0;

    if (hue < 60) {
      r = c;
      g = x;
    } else if (hue < 120) {
      r = x;
      g = c;
    } else if (hue < 180) {
      g = c;
      b = x;
    } else if (hue < 240) {
      g = x;
      b = c;
    } else if (hue < 300) {
      r = x;
      b = c;
    } else {
      r = c;
      b = x;
    }

    return {
      r: Math.round(clamp((r + m) * 255, 0, 255)),
      g: Math.round(clamp((g + m) * 255, 0, 255)),
      b: Math.round(clamp((b + m) * 255, 0, 255))
    };
  }

  function hsvToHex(h: number, s: number, v: number, a = 1): string {
    const hue = ((h % 360) + 360) % 360;
    const sat = clamp(s, 0, 1);
    const valc = clamp(v, 0, 1);
    const c = valc * sat;
    const x = c * (1 - Math.abs(((hue / 60) % 2) - 1));
    const m = valc - c;
    let r = 0;
    let g = 0;
    let b = 0;

    if (hue < 60) {
      r = c;
      g = x;
    } else if (hue < 120) {
      r = x;
      g = c;
    } else if (hue < 180) {
      g = c;
      b = x;
    } else if (hue < 240) {
      g = x;
      b = c;
    } else if (hue < 300) {
      r = x;
      b = c;
    } else {
      r = c;
      b = x;
    }

    const toHex = (value: number) =>
      Math.round(clamp((value + m) * 255, 0, 255))
        .toString(16)
        .padStart(2, '0');

    const alphaHex = Math.round(clamp(a, 0, 1) * 255)
      .toString(16)
      .padStart(2, '0');
    return a >= 1 ? `#${toHex(r)}${toHex(g)}${toHex(b)}` : `#${toHex(r)}${toHex(g)}${toHex(b)}${alphaHex}`;
  }

  function setFromHex(hex: string): void {
    const { r, g, b, a } = hexToRgba(hex);
    const hsv = rgbToHsv(r, g, b);
    hue = hsv.h;
    sat = hsv.s;
    val = hsv.v;
    alpha = clamp(a, 0, 1);
  }

  function emitChange(): void {
    const next = hsvToHex(hue, sat, val, alpha);
    dispatch('change', { value: next, hsv: { h: hue, s: sat, v: val, a: alpha } });
  }

  function drawWheel(): void {
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const radius = size / 2;
    const image = ctx.createImageData(size, size);
    const data = image.data;
    for (let y = 0; y < size; y += 1) {
      for (let x = 0; x < size; x += 1) {
        const dx = x - radius;
        const dy = y - radius;
        const dist = Math.sqrt(dx * dx + dy * dy);
        const idx = (y * size + x) * 4;
        if (dist > radius) {
          data[idx + 3] = 0;
          continue;
        }
        const satLocal = dist / radius;
        const angle = Math.atan2(dy, dx);
        const hueLocal = ((angle * 180) / Math.PI + 360) % 360;
        const hex = hsvToHex(hueLocal, satLocal, val);
        const { r, g, b } = hexToRgba(hex);
        data[idx] = r;
        data[idx + 1] = g;
        data[idx + 2] = b;
        data[idx + 3] = 255;
      }
    }
    ctx.putImageData(image, 0, 0);
  }

  function updateFromEvent(event: PointerEvent): void {
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const x = clamp(event.clientX - rect.left, 0, rect.width);
    const y = clamp(event.clientY - rect.top, 0, rect.height);
    const radius = rect.width / 2;
    const dx = x - radius;
    const dy = y - radius;
    const dist = Math.sqrt(dx * dx + dy * dy);
    if (dist > radius) return;
    hue = ((Math.atan2(dy, dx) * 180) / Math.PI + 360) % 360;
    sat = clamp(dist / radius, 0, 1);
    emitChange();
  }

  function handlePointerDown(event: PointerEvent): void {
    if (disabled) return;
    dragging = true;
    updateFromEvent(event);
    window.addEventListener('pointermove', updateFromEvent);
    window.addEventListener('pointerup', handlePointerUp);
  }

  function handlePointerUp(): void {
    dragging = false;
    window.removeEventListener('pointermove', updateFromEvent);
    window.removeEventListener('pointerup', handlePointerUp);
  }

  $effect(() => {
    if (dragging) return;
    if (value === lastPropValue) return;
    setFromHex(normalizeHex(value, '#ffffff'));
    drawWheel();
    lastPropValue = value;
  });

  $effect(() => {
    if (!canvas) return;
    canvas.width = size;
    canvas.height = size;
    drawWheel();
  });

  onMount(() => {
    setFromHex(normalizeHex(value, '#ffffff'));
    drawWheel();
    return () => {
      window.removeEventListener('pointermove', updateFromEvent);
      window.removeEventListener('pointerup', handlePointerUp);
    };
  });
</script>

<div class="color-wheel" style={`--wheel-size:${size}px;`}>
  <div class={`wheel-surface ${disabled ? 'is-disabled' : ''}`}>
    <canvas
      bind:this={canvas}
      width={size}
      height={size}
      onpointerdown={handlePointerDown}
    ></canvas>
    <div
      class="wheel-knob"
      style={`--knob-x:${(size / 2) + Math.cos((hue * Math.PI) / 180) * sat * (size / 2)}px; --knob-y:${(size / 2) + Math.sin((hue * Math.PI) / 180) * sat * (size / 2)}px; background:${hsvToHex(hue, sat, val, alpha)};`}
    ></div>
  </div>
  <div class="value-slider">
    <input
      class="range-input w-full hsv-range"
      type="range"
      min={0}
      max={1}
      step={0.01}
      value={val}
      style={`background: linear-gradient(90deg, #000000, ${hsvToHex(hue, sat, 1)});`}
      disabled={disabled}
      oninput={(event) => {
        const next = Number((event.currentTarget as HTMLInputElement).value);
        val = clamp(next, 0, 1);
        drawWheel();
        emitChange();
      }}
    />
  </div>
  <div class="alpha-row">
    <span>A</span>
    <input
      class="range-input w-full hsv-range alpha-range"
      type="range"
      min={0}
      max={1}
      step={0.01}
      value={alpha}
      style={`--alpha-gradient: linear-gradient(90deg, ${hsvToHex(hue, sat, val, 0)}, ${hsvToHex(hue, sat, val, 1)});`}
      disabled={disabled}
      oninput={(event) => {
        const next = Number((event.currentTarget as HTMLInputElement).value);
        alpha = clamp(next, 0, 1);
        emitChange();
      }}
    />
    <input
      type="number"
      min={0}
      max={1}
      step={0.01}
      value={Number(alpha.toFixed(2))}
      disabled={disabled}
      oninput={(event) => {
        const next = Number((event.currentTarget as HTMLInputElement).value);
        alpha = clamp(next, 0, 1);
        emitChange();
      }}
    />
  </div>
  <div class="hsv-inputs">
    <label class="hsv-field">
      <span>H</span>
      <input
        type="number"
        min={0}
        max={360}
        step={1}
        value={Math.round(hue)}
        disabled={disabled}
        oninput={(event) => {
          const next = Number((event.currentTarget as HTMLInputElement).value);
          hue = clamp(next, 0, 360);
          drawWheel();
          emitChange();
        }}
      />
    </label>
    <label class="hsv-field">
      <span>S</span>
      <input
        type="number"
        min={0}
        max={1}
        step={0.01}
        value={Number(sat.toFixed(2))}
        disabled={disabled}
        oninput={(event) => {
          const next = Number((event.currentTarget as HTMLInputElement).value);
          sat = clamp(next, 0, 1);
          drawWheel();
          emitChange();
        }}
      />
    </label>
    <label class="hsv-field">
      <span>V</span>
      <input
        type="number"
        min={0}
        max={1}
        step={0.01}
        value={Number(val.toFixed(2))}
        disabled={disabled}
        oninput={(event) => {
          const next = Number((event.currentTarget as HTMLInputElement).value);
          val = clamp(next, 0, 1);
          drawWheel();
          emitChange();
        }}
      />
    </label>
  </div>
  <div class="rgb-inputs">
    <label class="hsv-field">
      <span>R</span>
      <input
        type="number"
        min={0}
        max={255}
        step={1}
        value={hsvToRgb(hue, sat, val).r}
        disabled={disabled}
        oninput={(event) => {
          const rgb = hsvToRgb(hue, sat, val);
          const next = clamp(Number((event.currentTarget as HTMLInputElement).value), 0, 255);
          const hsv = rgbToHsv(next, rgb.g, rgb.b);
          hue = hsv.h;
          sat = hsv.s;
          val = hsv.v;
          drawWheel();
          emitChange();
        }}
      />
    </label>
    <label class="hsv-field">
      <span>G</span>
      <input
        type="number"
        min={0}
        max={255}
        step={1}
        value={hsvToRgb(hue, sat, val).g}
        disabled={disabled}
        oninput={(event) => {
          const rgb = hsvToRgb(hue, sat, val);
          const next = clamp(Number((event.currentTarget as HTMLInputElement).value), 0, 255);
          const hsv = rgbToHsv(rgb.r, next, rgb.b);
          hue = hsv.h;
          sat = hsv.s;
          val = hsv.v;
          drawWheel();
          emitChange();
        }}
      />
    </label>
    <label class="hsv-field">
      <span>B</span>
      <input
        type="number"
        min={0}
        max={255}
        step={1}
        value={hsvToRgb(hue, sat, val).b}
        disabled={disabled}
        oninput={(event) => {
          const rgb = hsvToRgb(hue, sat, val);
          const next = clamp(Number((event.currentTarget as HTMLInputElement).value), 0, 255);
          const hsv = rgbToHsv(rgb.r, rgb.g, next);
          hue = hsv.h;
          sat = hsv.s;
          val = hsv.v;
          drawWheel();
          emitChange();
        }}
      />
    </label>
  </div>
</div>

<style>
  .color-wheel {
    display: grid;
    gap: 0.5rem;
    width: var(--wheel-size);
  }

  .wheel-surface {
    position: relative;
    width: var(--wheel-size);
    height: var(--wheel-size);
  }

  .wheel-surface canvas {
    width: var(--wheel-size);
    height: var(--wheel-size);
    border-radius: 999px;
    cursor: crosshair;
    display: block;
  }

  .wheel-surface.is-disabled {
    opacity: 0.5;
    pointer-events: none;
  }

  .wheel-knob {
    position: absolute;
    width: 0.75rem;
    height: 0.75rem;
    border-radius: 999px;
    border: 2px solid #fff;
    box-shadow: 0 0 0 2px rgba(15, 23, 42, 0.6);
    left: 0;
    top: 0;
    transform: translate(calc(var(--knob-x) - 0.375rem), calc(var(--knob-y) - 0.375rem));
    pointer-events: none;
  }

  .value-slider {
    width: var(--wheel-size);
  }

  .alpha-row {
    display: grid;
    grid-template-columns: 1rem minmax(0, 1fr) 3.5rem;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.7rem;
    color: rgb(148 163 184);
  }

  .alpha-row input[type='number'] {
    width: 100%;
    border-radius: 0.375rem;
    border: 1px solid rgba(51, 65, 85, 0.7);
    background: rgba(15, 23, 42, 0.7);
    color: rgb(226, 232, 240);
    font-size: 0.75rem;
    padding: 0.25rem 0.4rem;
  }

  :global(.alpha-range) {
    --checker-size: 8px;
    background-image:
      var(--alpha-gradient),
      linear-gradient(45deg, rgba(148, 163, 184, 0.35) 25%, transparent 25%),
      linear-gradient(-45deg, rgba(148, 163, 184, 0.35) 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, rgba(148, 163, 184, 0.35) 75%),
      linear-gradient(-45deg, transparent 75%, rgba(148, 163, 184, 0.35) 75%);
    background-size:
      100% 100%,
      var(--checker-size) var(--checker-size),
      var(--checker-size) var(--checker-size),
      var(--checker-size) var(--checker-size),
      var(--checker-size) var(--checker-size);
    background-position:
      0 0,
      0 0,
      0 calc(var(--checker-size) / 2),
      calc(var(--checker-size) / 2) calc(var(--checker-size) / 2),
      calc(var(--checker-size) / 2) 0;
  }

  .hsv-inputs {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.5rem;
  }

  .rgb-inputs {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.5rem;
  }

  .hsv-field {
    display: grid;
    gap: 0.25rem;
    font-size: 0.7rem;
    color: rgb(148 163 184);
  }

  .hsv-field span {
    letter-spacing: 0.2em;
    text-transform: uppercase;
  }

  .hsv-field input {
    width: 100%;
    border-radius: 0.375rem;
    border: 1px solid rgba(51, 65, 85, 0.7);
    background: rgba(15, 23, 42, 0.7);
    color: rgb(226, 232, 240);
    font-size: 0.75rem;
    padding: 0.25rem 0.4rem;
  }
</style>
