<script lang="ts">
  import LightingLivePanel from './LightingLivePanel.svelte';

  const {
    liveBusy,
    previewFrame,
    liveBrightness,
    liveError,
    liveStatus,
    onPreviewCurrent,
    onTurnOff,
    onBrightnessPreset,
    onBrightnessInput
  } = $props<{
    liveBusy: boolean;
    previewFrame: { colors: string[]; whites: number[]; brightness: number };
    liveBrightness: number;
    liveError: string | null;
    liveStatus: string | null;
    onPreviewCurrent: () => void;
    onTurnOff: () => void;
    onBrightnessPreset: (value: number) => void;
    onBrightnessInput: (value: number) => void;
  }>();
</script>

<div class="space-y-5">
  <LightingLivePanel
    {liveBusy}
    {previewFrame}
    {liveBrightness}
    onPreviewCurrent={onPreviewCurrent}
    onTurnOff={onTurnOff}
    onBrightnessPreset={onBrightnessPreset}
    onBrightnessInput={onBrightnessInput}
  />

  {#if liveError}
    <p class="text-xs text-error-400">{liveError}</p>
  {/if}
  {#if liveStatus}
    <p class="text-xs text-success-400">{liveStatus}</p>
  {/if}
</div>
