<script lang="ts">
  import ColorDropperButton from '$lib/components/controls/ColorDropperButton.svelte';

  type LightingAnimationKind = 'frame' | 'chase' | 'pulse' | 'rainbow' | 'breathing_rainbow';
  type LightingAnimationMode = LightingAnimationKind | 'timeline';

  type Props = {
    animationKind: LightingAnimationMode;
    animationModeOptions: { value: LightingAnimationMode; label: string; helper: string }[];
    liveBusy: boolean;
    animationColor: string;
    animationWhite: number;
    animationSpeedHz: number;
    animationLow: number;
    animationHigh: number;
    animationPeriodMs: number;
    onSetAnimationMode: (mode: LightingAnimationMode) => void;
    onAnimationColorChange: (value: string) => void;
    onAnimationWhiteChange: (value: number) => void;
    onAnimationSpeedChange: (value: number) => void;
    onAnimationLowChange: (value: number) => void;
    onAnimationHighChange: (value: number) => void;
    onAnimationPeriodChange: (value: number) => void;
  };

  const {
    animationKind,
    animationModeOptions,
    liveBusy,
    animationColor,
    animationWhite,
    animationSpeedHz,
    animationLow,
    animationHigh,
    animationPeriodMs,
    onSetAnimationMode,
    onAnimationColorChange,
    onAnimationWhiteChange,
    onAnimationSpeedChange,
    onAnimationLowChange,
    onAnimationHighChange,
    onAnimationPeriodChange
  }: Props = $props();

  const clampNumber = (value: number, min: number, max: number): number => Math.min(max, Math.max(min, value));
</script>

<div class="space-y-4 rounded-2xl border border-surface-800/80 bg-surface-950/40 p-5">
  <div class="space-y-2">
    <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Mode</p>
    <div class="grid gap-2 sm:grid-cols-2">
      {#each animationModeOptions as option (option.value)}
        <button
          class={`flex flex-col gap-1 rounded-lg border px-3 py-2 text-left text-xs transition ${
            animationKind === option.value
              ? 'border-primary-400/70 bg-primary-500/15 text-primary-50'
              : 'border-surface-800/70 bg-surface-950/60 text-surface-300 hover:border-surface-600'
          }`}
          type="button"
          disabled={liveBusy}
          onclick={() => onSetAnimationMode(option.value)}
        >
          <span class="text-sm font-semibold">{option.label}</span>
          <span class="text-micro uppercase tracking-[0.25em] text-surface-500">{option.helper}</span>
        </button>
      {/each}
    </div>
  </div>

  <div class="grid gap-4 sm:grid-cols-2">
    <label class="space-y-1 text-sm">
      <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Accent colour</span>
      <div class="flex items-center gap-2">
        <input
          class="h-11 w-full flex-1 cursor-pointer rounded border border-surface-700 bg-surface-900"
          type="color"
          value={animationColor}
          oninput={(event) => onAnimationColorChange(event.currentTarget.value)}
        />
        <ColorDropperButton
          title="Pick color from screen"
          ariaLabel="Pick color from screen"
          onPick={(hex) => onAnimationColorChange(hex)}
        />
      </div>
    </label>
    <label class="space-y-1 text-sm">
      <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Accent white</span>
      <input
        class="input w-full"
        type="number"
        min="0"
        max="255"
        value={animationWhite}
        oninput={(event) => onAnimationWhiteChange(clampNumber(Number(event.currentTarget.value), 0, 255))}
      />
    </label>
  </div>

  {#if animationKind !== 'frame' && animationKind !== 'timeline'}
    <div class="grid gap-4 sm:grid-cols-2">
      <label class="space-y-1 text-sm">
        <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Speed (Hz)</span>
        <input
          class="input w-full"
          type="number"
          min="0.1"
          step="0.1"
          value={animationSpeedHz}
          oninput={(event) => onAnimationSpeedChange(Number(event.currentTarget.value))}
        />
      </label>
      {#if animationKind === 'pulse' || animationKind === 'breathing_rainbow'}
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Period (ms)</span>
          <input
            class="input w-full"
            type="number"
            min="50"
            step="10"
            value={animationPeriodMs}
            oninput={(event) => onAnimationPeriodChange(Number(event.currentTarget.value))}
          />
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Low</span>
          <input
            class="input w-full"
            type="number"
            min="0"
            max="255"
            value={animationLow}
            oninput={(event) => onAnimationLowChange(Number(event.currentTarget.value))}
          />
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">High</span>
          <input
            class="input w-full"
            type="number"
            min="0"
            max="255"
            value={animationHigh}
            oninput={(event) => onAnimationHighChange(Number(event.currentTarget.value))}
          />
        </label>
      {/if}
    </div>
  {:else if animationKind === 'frame'}
    <p class="text-xs text-surface-500">Frame mode uses the editor in the preview panel.</p>
  {:else}
    <p class="text-xs text-surface-500">Timeline mode uses keyframes, easing curves, and interpolation.</p>
  {/if}
</div>
