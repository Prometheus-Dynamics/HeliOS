<script lang="ts">
  import LedRingPreview from '../LedRingPreview.svelte';

  type PreviewFrame = {
    colors: string[];
    whites: number[];
    brightness: number;
  };

  type Props = {
    liveBusy: boolean;
    previewFrame: PreviewFrame;
    liveBrightness: number;
    onPreviewCurrent: () => void;
    onTurnOff: () => void;
    onBrightnessPreset: (value: number) => void;
    onBrightnessInput: (value: number) => void;
  };

  const {
    liveBusy,
    previewFrame,
    liveBrightness,
    onPreviewCurrent,
    onTurnOff,
    onBrightnessPreset,
    onBrightnessInput
  }: Props = $props();
</script>

<div class="space-y-5">
  <header class="flex flex-wrap items-center justify-between gap-4">
    <div class="space-y-1">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Live output</p>
      <p class="text-sm text-surface-400">Send current frame/timeline to device and monitor ring state.</p>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <button class="btn btn-xs preset-tonal" type="button" disabled={liveBusy} onclick={() => onPreviewCurrent()}>
        Preview current
      </button>
      <button class="btn btn-xs preset-filled-primary-500" type="button" disabled={liveBusy} onclick={() => onTurnOff()}>
        Turn off
      </button>
    </div>
  </header>

  <div class="rounded-2xl border border-surface-800/80 bg-surface-950/40 p-5">
    <div class="pb-4">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Ring state</p>
      <p class="text-xs text-surface-500">UI animation preview and frame editor removed. Use timeline and device preview.</p>
    </div>
    <LedRingPreview
      count={previewFrame.colors.length}
      colors={previewFrame.colors}
      whites={previewFrame.whites}
      indexOffset={-2}
      brightness={previewFrame.brightness}
      selected={-1}
      interactive={false}
    />
  </div>

  <div class="rounded-2xl border border-surface-800/80 bg-surface-950/40 p-5">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Brightness</p>
        <p class="text-xs text-surface-500">Applies to device preview/output commands.</p>
      </div>
      <div class="flex items-center gap-2">
        <button class="btn btn-3xs preset-tonal" type="button" onclick={() => onBrightnessPreset(64)}>
          Low
        </button>
        <button class="btn btn-3xs preset-tonal" type="button" onclick={() => onBrightnessPreset(128)}>
          Mid
        </button>
        <button class="btn btn-3xs preset-tonal" type="button" onclick={() => onBrightnessPreset(200)}>
          High
        </button>
      </div>
    </div>
    <div class="mt-4 flex items-center gap-3">
      <input
        class="w-full"
        type="range"
        min="0"
        max="255"
        value={liveBrightness}
        oninput={(event) => onBrightnessInput(Number((event.target as HTMLInputElement).value))}
      />
      <input
        class="input w-24"
        type="number"
        min="0"
        max="255"
        value={liveBrightness}
        oninput={(event) => onBrightnessInput(Number((event.target as HTMLInputElement).value))}
      />
    </div>
  </div>
</div>
