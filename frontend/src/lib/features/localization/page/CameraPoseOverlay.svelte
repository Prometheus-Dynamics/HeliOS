<script lang="ts">
  type CameraPoseOverlayProps = {
    onClose?: () => void;
    primaryCameraKey?: string | null;
    cameraPoseXInput?: string;
    cameraPoseYInput?: string;
    cameraPoseZInput?: string;
    cameraPosePitchDeg?: string;
    cameraPoseYawDeg?: string;
    cameraPoseRollDeg?: string;
    cameraPoseEditorError?: string | null;
    onReset?: () => void;
    onApply?: () => void;
  };

  let {
    onClose,
    primaryCameraKey = null,
    cameraPoseXInput = $bindable(''),
    cameraPoseYInput = $bindable(''),
    cameraPoseZInput = $bindable(''),
    cameraPosePitchDeg = $bindable('0'),
    cameraPoseYawDeg = $bindable('0'),
    cameraPoseRollDeg = $bindable('0'),
    cameraPoseEditorError = null,
    onReset,
    onApply
  }: CameraPoseOverlayProps = $props();

  export type $$Props = CameraPoseOverlayProps;
</script>

<div class="pointer-events-auto max-h-full w-full max-w-[28rem] overflow-auto rounded border border-surface-800 bg-surface-950/85 p-4 text-xs text-surface-300 shadow-xl backdrop-blur">
  <div class="flex items-start justify-between gap-3">
    <div>
      <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Field camera pose</p>
      <p class="mt-1 text-xs text-surface-500">X right, Y up, forward is -Z · pitch(X), yaw(Y), roll(Z).</p>
      {#if primaryCameraKey}
        <p class="mt-1 text-micro text-surface-500">Key: {primaryCameraKey}</p>
      {/if}
    </div>
    <button class="btn btn-ghost btn-xs uppercase tracking-[0.3em]" type="button" onclick={onClose}>
      Close
    </button>
  </div>

  <div class="mt-3 grid grid-cols-3 gap-2">
    <input
      class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
      bind:value={cameraPoseXInput}
      placeholder="X (e.g. 1m)"
    />
    <input
      class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
      bind:value={cameraPoseYInput}
      placeholder="Y (e.g. 0.5m)"
    />
    <input
      class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
      bind:value={cameraPoseZInput}
      placeholder="Z (e.g. -2m)"
    />
  </div>
  <div class="mt-2 grid grid-cols-3 gap-2">
    <input
      class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
      bind:value={cameraPosePitchDeg}
      placeholder="Pitch°"
    />
    <input
      class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
      bind:value={cameraPoseYawDeg}
      placeholder="Yaw°"
    />
    <input
      class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1 text-xs text-surface-50 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
      bind:value={cameraPoseRollDeg}
      placeholder="Roll°"
    />
  </div>
  <div class="mt-3 flex items-center justify-between gap-3">
    <button class="btn btn-ghost btn-xs uppercase tracking-[0.3em]" type="button" onclick={onReset}>
      Reset
    </button>
    <button
      class="btn btn-ghost btn-xs uppercase tracking-[0.3em]"
      type="button"
      onclick={onApply}
      disabled={!primaryCameraKey}
    >
      Apply
    </button>
  </div>
  {#if cameraPoseEditorError}
    <p class="mt-2 text-xs text-error-300">{cameraPoseEditorError}</p>
  {/if}
</div>
