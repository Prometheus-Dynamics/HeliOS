<script lang="ts">
  import LedRingPreview from '../LedRingPreview.svelte';

  type Props = {
    ledColors: string[];
    ledWhites: number[];
    ledBrightnesses: number[];
    selectedLedIndices: number[];
    editorColor: string;
    editorBrightness: number;
    previewBusy: boolean;
    stopBusy: boolean;
    onToggleLed: (index: number) => void;
    onEditorColorChange: (value: string) => void;
    onEditorBrightnessChange: (value: number) => void;
    onPreview: () => void;
    onStop: () => void;
    onSelectAll: () => void;
    onClearSelection: () => void;
  };

  const {
    ledColors,
    ledWhites,
    ledBrightnesses,
    selectedLedIndices,
    editorColor,
    editorBrightness,
    previewBusy,
    stopBusy,
    onToggleLed,
    onEditorColorChange,
    onEditorBrightnessChange,
    onPreview,
    onStop,
    onSelectAll,
    onClearSelection
  }: Props = $props();
</script>

<section class="rounded-xl bg-surface-950/60 p-3">
  <div class="grid gap-3 md:grid-cols-[minmax(0,0.33fr)_minmax(0,0.67fr)] md:items-start">
    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <p class="text-xs text-surface-400">Editor frame preview</p>
        <div class="flex items-center gap-2">
          <p class="text-xs text-surface-500">{selectedLedIndices.length} selected</p>
          <button class="btn btn-3xs preset-tonal" type="button" disabled={previewBusy} onclick={() => onPreview()}>
            Preview frame
          </button>
          <button class="btn btn-3xs preset-outline" type="button" disabled={stopBusy} onclick={() => onStop()}>
            Stop
          </button>
        </div>
      </div>
      <LedRingPreview
        count={ledColors.length}
        colors={ledColors}
        whites={ledWhites}
        brightnesses={ledBrightnesses}
        indexOffset={-2}
        brightness={255}
        selectedIndices={selectedLedIndices}
        interactive={true}
        onSelect={onToggleLed}
        hideHeader={true}
        showSwatches={false}
        svgClass="h-40 w-full select-none"
      />
      <p class="text-[0.7rem] text-surface-500">Click LEDs to add or remove them from selection.</p>
    </div>

    <div class="space-y-3">
      <p class="text-xs text-surface-400">LED / color apply controls</p>
      <div class="space-y-2">
        <label class="space-y-1 text-xs text-surface-300">
          <span class="text-micro uppercase tracking-[0.18em] text-surface-500">Color</span>
          <input
            class="h-10 w-full cursor-pointer rounded border border-surface-700 bg-surface-900"
            type="color"
            value={editorColor}
            oninput={(event) => onEditorColorChange((event.target as HTMLInputElement).value)}
          />
        </label>

        <label class="space-y-1 text-xs text-surface-300">
          <div class="flex items-center justify-between">
            <span class="text-micro uppercase tracking-[0.18em] text-surface-500">Brightness</span>
            <span class="text-[0.68rem] text-surface-500">{Math.round(editorBrightness)}</span>
          </div>
          <input
            class="w-full"
            type="range"
            min="0"
            max="255"
            step="1"
            value={editorBrightness}
            oninput={(event) => onEditorBrightnessChange(Number((event.target as HTMLInputElement).value))}
          />
        </label>

        <div class="grid gap-2 sm:grid-cols-2">
          <button class="btn btn-2xs preset-tonal" type="button" onclick={() => onSelectAll()}>
            Select All
          </button>
          <button class="btn btn-2xs preset-tonal" type="button" onclick={() => onClearSelection()}>
            Clear Selection
          </button>
        </div>
        <p class="text-[0.68rem] text-surface-500">Shortcuts: Ctrl/Cmd+Z undo, Ctrl/Cmd+Shift+Z redo.</p>
      </div>
    </div>
  </div>
</section>
