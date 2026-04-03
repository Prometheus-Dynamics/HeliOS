<script lang="ts">
  import type { PipelineUiControl } from '$lib/features/pipelines/pipelineUiTypes';

  type Props = {
    control: PipelineUiControl;
    isSettable: boolean;
    readLocalValue: (key: string, fallback: string) => string;
    setLocalValue: (key: string, value: string) => void;
  };

  let { control, isSettable, readLocalValue, setLocalValue }: Props = $props();

  const clampNumber = (value: number, min: number, max: number): number => {
    if (!Number.isFinite(value)) return min;
    return Math.min(max, Math.max(min, value));
  };

  function hsvToHex(h: number, s: number, v: number): string {
    const hue = ((h % 360) + 360) % 360;
    const sat = clampNumber(s, 0, 1);
    const val = clampNumber(v, 0, 1);
    const c = val * sat;
    const x = c * (1 - Math.abs(((hue / 60) % 2) - 1));
    const m = val - c;
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

    const toHex = (value: number) => Math.round((value + m) * 255).toString(16).padStart(2, '0');
    return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
  }
</script>

{#if control.type === 'hsv'}
  {@const hsvKey = control.id}
  {@const hsvDefaults = control.hsvDefaults ?? { h: 120, s: 0.7, v: 0.8 }}
  {@const hKey = `${hsvKey}:h`}
  {@const sKey = `${hsvKey}:s`}
  {@const vKey = `${hsvKey}:v`}
  {@const hValue = clampNumber(Number(readLocalValue(hKey, String(hsvDefaults.h))), 0, 360)}
  {@const sValue = clampNumber(Number(readLocalValue(sKey, String(hsvDefaults.s))), 0, 1)}
  {@const vValue = clampNumber(Number(readLocalValue(vKey, String(hsvDefaults.v))), 0, 1)}
  {@const previewHex = hsvToHex(hValue, sValue, vValue)}
  {@const satStart = hsvToHex(hValue, 0, vValue)}
  {@const satEnd = hsvToHex(hValue, 1, vValue)}
  {@const valEnd = hsvToHex(hValue, sValue, 1)}
  <div class="space-y-3">
    <div class="flex items-center gap-2">
      <div class="h-7 w-7 rounded border border-surface-700" style={`background:${previewHex};`}></div>
      <span class="text-xs text-surface-300">{previewHex}</span>
    </div>
    <label class="block space-y-2">
      <span class="text-micro uppercase tracking-[0.2em] text-surface-500">Hue</span>
      <input
        class="range-input w-full simple-range"
        type="range"
        min={0}
        max={360}
        step={1}
        value={hValue}
        disabled={!isSettable}
        style={`--range-fill:${`linear-gradient(90deg, #ff0000 0%, #ffff00 17%, #00ff00 33%, #00ffff 50%, #0000ff 67%, #ff00ff 83%, #ff0000 100%)`}; --range-progress: 100%;`}
        oninput={(event) => setLocalValue(hKey, event.currentTarget.value)}
      />
    </label>
    <label class="block space-y-2">
      <span class="text-micro uppercase tracking-[0.2em] text-surface-500">Saturation</span>
      <input
        class="range-input w-full simple-range"
        type="range"
        min={0}
        max={1}
        step={0.01}
        value={sValue}
        style={`--range-fill:${`linear-gradient(90deg, ${satStart}, ${satEnd})`}; --range-progress: 100%;`}
        disabled={!isSettable}
        oninput={(event) => setLocalValue(sKey, event.currentTarget.value)}
      />
    </label>
    <label class="block space-y-2">
      <span class="text-micro uppercase tracking-[0.2em] text-surface-500">Value</span>
      <input
        class="range-input w-full simple-range"
        type="range"
        min={0}
        max={1}
        step={0.01}
        value={vValue}
        style={`--range-fill:${`linear-gradient(90deg, #000000, ${valEnd})`}; --range-progress: 100%;`}
        disabled={!isSettable}
        oninput={(event) => setLocalValue(vKey, event.currentTarget.value)}
      />
    </label>
  </div>
{:else if control.type === 'hsv_range'}
  {@const hsvRangeDefaults = control.hsvRangeDefaults ?? { h: [0, 360], s: [0, 1], v: [0, 1] }}
  {@const rangeMode = control.hsvRangeMode ?? (control.id.includes('exclude') ? 'exclude' : 'include')}
  {@const hsvKey = control.id}
  {@const hMinKey = `${hsvKey}:hMin`}
  {@const hMaxKey = `${hsvKey}:hMax`}
  {@const sMinKey = `${hsvKey}:sMin`}
  {@const sMaxKey = `${hsvKey}:sMax`}
  {@const vMinKey = `${hsvKey}:vMin`}
  {@const vMaxKey = `${hsvKey}:vMax`}
  {@const hMinVal = clampNumber(Number(readLocalValue(hMinKey, String(hsvRangeDefaults.h[0]))), 0, 360)}
  {@const hMaxVal = clampNumber(Number(readLocalValue(hMaxKey, String(hsvRangeDefaults.h[1]))), 0, 360)}
  {@const sMinVal = clampNumber(Number(readLocalValue(sMinKey, String(hsvRangeDefaults.s[0]))), 0, 1)}
  {@const sMaxVal = clampNumber(Number(readLocalValue(sMaxKey, String(hsvRangeDefaults.s[1]))), 0, 1)}
  {@const vMinVal = clampNumber(Number(readLocalValue(vMinKey, String(hsvRangeDefaults.v[0]))), 0, 1)}
  {@const vMaxVal = clampNumber(Number(readLocalValue(vMaxKey, String(hsvRangeDefaults.v[1]))), 0, 1)}
  {@const hMid = (hMinVal + hMaxVal) * 0.5}
  {@const sMid = (sMinVal + sMaxVal) * 0.5}
  {@const vMid = (vMinVal + vMaxVal) * 0.5}
  {@const satBase = `linear-gradient(90deg, ${hsvToHex(hMid, 0, vMid)}, ${hsvToHex(hMid, 1, vMid)})`}
  {@const valBase = `linear-gradient(90deg, #000000, ${hsvToHex(hMid, sMid, 1)})`}
  <div class="space-y-4">
    <p class="text-micro uppercase tracking-[0.2em] text-surface-500">
      {rangeMode === 'exclude' ? 'Exclude' : 'Include'} HSV range
    </p>
    <div class="space-y-3">
      <label class="block space-y-2">
        <span class="text-micro uppercase tracking-[0.2em] text-surface-500">Hue</span>
        <div class="flex items-center gap-2">
          <div class="dual-range hsv-dual flex-1">
            <div
              class="hsv-track"
              style={`--range-fill:${`linear-gradient(90deg, #ff0000, #00ff00, #0000ff, #ff0000)`};`}
            ></div>
          <input
            class="dual-range-input range-input dual-range-min hsv-range simple-range"
            type="range"
            min={0}
            max={360}
              step={1}
              value={hMinVal}
              disabled={!isSettable}
              oninput={(event) => {
                const next = clampNumber(Number(event.currentTarget.value), 0, 360);
                const maxCurrent = Number.isFinite(Number(hMaxVal)) ? Number(hMaxVal) : 360;
                if (next > maxCurrent) {
                  setLocalValue(hMaxKey, String(next));
                }
                setLocalValue(hMinKey, String(Math.min(next, maxCurrent)));
              }}
            />
            <input
            class="dual-range-input range-input dual-range-max hsv-range simple-range"
              type="range"
              min={0}
              max={360}
              step={1}
              value={hMaxVal}
              disabled={!isSettable}
              oninput={(event) => {
                const next = clampNumber(Number(event.currentTarget.value), 0, 360);
                const minCurrent = Number.isFinite(Number(hMinVal)) ? Number(hMinVal) : 0;
                if (next < minCurrent) {
                  setLocalValue(hMinKey, String(next));
                }
                setLocalValue(hMaxKey, String(Math.max(next, minCurrent)));
              }}
            />
          </div>
          <input
            class="w-16 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
            type="number"
            min={0}
            max={360}
            step={1}
            value={hMaxVal.toFixed(0)}
            disabled={!isSettable}
            oninput={(event) => {
              const raw = event.currentTarget.value;
              const next = clampNumber(Number(raw), 0, 360);
              const minCurrent = Number.isFinite(Number(hMinVal)) ? Number(hMinVal) : 0;
              if (next < minCurrent) {
                setLocalValue(hMinKey, String(next));
              }
              setLocalValue(hMaxKey, String(Math.max(next, minCurrent)));
            }}
          />
        </div>
      </label>

      <label class="block space-y-2">
        <span class="text-micro uppercase tracking-[0.2em] text-surface-500">Saturation</span>
        <div class="flex items-center gap-2">
          <div class="dual-range hsv-dual flex-1">
            <div class="hsv-track" style={`--range-fill:${satBase};`}></div>
            <input
            class="dual-range-input range-input dual-range-min simple-range"
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={sMinVal}
              disabled={!isSettable}
              oninput={(event) => {
                const raw = event.currentTarget.value;
                const next = clampNumber(Number(raw), 0, 1);
                const maxCurrent = Number.isFinite(Number(sMaxVal)) ? Number(sMaxVal) : 1;
                if (next > maxCurrent) {
                  setLocalValue(sMaxKey, String(next));
                }
                setLocalValue(sMinKey, String(Math.min(next, maxCurrent)));
              }}
            />
            <input
            class="dual-range-input range-input dual-range-max simple-range"
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={sMaxVal}
              disabled={!isSettable}
              oninput={(event) => {
                const raw = event.currentTarget.value;
                const next = clampNumber(Number(raw), 0, 1);
                const minCurrent = Number.isFinite(Number(sMinVal)) ? Number(sMinVal) : 0;
                if (next < minCurrent) {
                  setLocalValue(sMinKey, String(next));
                }
                setLocalValue(sMaxKey, String(Math.max(next, minCurrent)));
              }}
            />
          </div>
          <input
            class="w-20 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
            type="number"
            min={0}
            max={1}
            step={0.01}
            value={sMaxVal.toFixed(2)}
            disabled={!isSettable}
            oninput={(event) => {
              const raw = event.currentTarget.value;
              const next = clampNumber(Number(raw), 0, 1);
              const minCurrent = Number.isFinite(Number(sMinVal)) ? Number(sMinVal) : 0;
              if (next < minCurrent) {
                setLocalValue(sMinKey, String(next));
              }
              setLocalValue(sMaxKey, String(Math.max(next, minCurrent)));
            }}
          />
        </div>
      </label>

      <label class="block space-y-2">
        <span class="text-micro uppercase tracking-[0.2em] text-surface-500">Value</span>
        <div class="flex items-center gap-2">
          <div class="dual-range hsv-dual flex-1">
            <div class="hsv-track" style={`--range-fill:${valBase};`}></div>
            <input
            class="dual-range-input range-input dual-range-min simple-range"
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={vMinVal}
              disabled={!isSettable}
              oninput={(event) => {
                const raw = event.currentTarget.value;
                const next = clampNumber(Number(raw), 0, 1);
                const maxCurrent = Number.isFinite(Number(vMaxVal)) ? Number(vMaxVal) : 1;
                if (next > maxCurrent) {
                  setLocalValue(vMaxKey, String(next));
                }
                setLocalValue(vMinKey, String(Math.min(next, maxCurrent)));
              }}
            />
            <input
            class="dual-range-input range-input dual-range-max simple-range"
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={vMaxVal}
              disabled={!isSettable}
              oninput={(event) => {
                const raw = event.currentTarget.value;
                const next = clampNumber(Number(raw), 0, 1);
                const minCurrent = Number.isFinite(Number(vMinVal)) ? Number(vMinVal) : 0;
                if (next < minCurrent) {
                  setLocalValue(vMinKey, String(next));
                }
                setLocalValue(vMaxKey, String(Math.max(next, minCurrent)));
              }}
            />
          </div>
          <input
            class="w-20 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
            type="number"
            min={0}
            max={1}
            step={0.01}
            value={vMaxVal.toFixed(2)}
            disabled={!isSettable}
            oninput={(event) => {
              const raw = event.currentTarget.value;
              const next = clampNumber(Number(raw), 0, 1);
              const minCurrent = Number.isFinite(Number(vMinVal)) ? Number(vMinVal) : 0;
              if (next < minCurrent) {
                setLocalValue(vMinKey, String(next));
              }
              setLocalValue(vMaxKey, String(Math.max(next, minCurrent)));
            }}
          />
        </div>
      </label>
    </div>
  </div>
{/if}

<style>
  @import '../PipelineUiControl.css';

  .hsv-track {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    transform: translateY(-50%);
    height: var(--dual-track-height, 0.5rem);
    border-radius: 999px;
    z-index: 0;
    pointer-events: none;
    background-image: var(--range-fill, linear-gradient(90deg, #22d3ee, #fb7185));
    background-size: 100% 100%;
    background-repeat: no-repeat;
  }
</style>
