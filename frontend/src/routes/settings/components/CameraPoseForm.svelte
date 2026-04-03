<script lang="ts">
  import type { RigCameraInfo } from '$lib/types/rig';

  type CameraPoseFormState = {
    x: string;
    y: string;
    z: string;
    roll: string;
    pitch: string;
    yaw: string;
  };

  type CameraPoseFormProps = {
    selectedCamera: RigCameraInfo | null;
    cameraPoseForm: CameraPoseFormState;
    cameraPoseBusy: boolean;
    cameraPoseDirty: boolean;
    cameraPoseError: string | null;
    cameraPoseMessage: string | null;
    onUpdateField: (key: keyof CameraPoseFormState, raw: string) => void;
    onSave: () => void;
    onClear: () => void;
  };

  const {
    selectedCamera,
    cameraPoseForm,
    cameraPoseBusy,
    cameraPoseDirty,
    cameraPoseError,
    cameraPoseMessage,
    onUpdateField,
    onSave,
    onClear
  }: CameraPoseFormProps = $props();

  function handlePoseFieldInput(key: keyof CameraPoseFormState, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    onUpdateField(key, target.value);
  }

  export type $$Props = CameraPoseFormProps;
</script>

<div class="rounded border border-surface-800 bg-surface-950/40 p-4">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <h3 class="text-xs uppercase tracking-[0.35em] text-surface-400">Camera pose</h3>
    <span class="text-micro uppercase tracking-[0.3em] text-surface-500">
      {selectedCamera ? selectedCamera.displayName : 'Select a camera'}
    </span>
  </div>
  <p class="mt-2 text-sm text-surface-400">
    Set the camera transform relative to the robot origin. Rotation values are in degrees.
  </p>

  {#if selectedCamera}
    <form class="mt-4 space-y-4" onsubmit={(event) => { event.preventDefault(); onSave(); }}>
      <p class="text-xs text-surface-500">Translation accepts inline units (examples: 0.2 m, 8&quot;, 0.5 ft). Rotation uses degrees (10, 10 deg, 10°).</p>

      <div class="grid gap-3 sm:grid-cols-3">
        <label class="flex flex-col gap-1">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">X</span>
          <input
            class="input input-sm"
            type="text"
            placeholder="e.g. 0.2 m or 8&quot;"
            value={cameraPoseForm.x}
            oninput={(event) => handlePoseFieldInput('x', event)}
            disabled={cameraPoseBusy}
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Y</span>
          <input
            class="input input-sm"
            type="text"
            placeholder="e.g. 0.0 m"
            value={cameraPoseForm.y}
            oninput={(event) => handlePoseFieldInput('y', event)}
            disabled={cameraPoseBusy}
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Z</span>
          <input
            class="input input-sm"
            type="text"
            placeholder="e.g. 0.4 m"
            value={cameraPoseForm.z}
            oninput={(event) => handlePoseFieldInput('z', event)}
            disabled={cameraPoseBusy}
          />
        </label>
      </div>

      <div class="grid gap-3 sm:grid-cols-3">
        <label class="flex flex-col gap-1">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Roll (deg)</span>
          <input
            class="input input-sm"
            type="text"
            placeholder="e.g. 10 deg"
            value={cameraPoseForm.roll}
            oninput={(event) => handlePoseFieldInput('roll', event)}
            disabled={cameraPoseBusy}
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Pitch (deg)</span>
          <input
            class="input input-sm"
            type="text"
            placeholder="e.g. -5°"
            value={cameraPoseForm.pitch}
            oninput={(event) => handlePoseFieldInput('pitch', event)}
            disabled={cameraPoseBusy}
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">Yaw (deg)</span>
          <input
            class="input input-sm"
            type="text"
            placeholder="e.g. 90"
            value={cameraPoseForm.yaw}
            oninput={(event) => handlePoseFieldInput('yaw', event)}
            disabled={cameraPoseBusy}
          />
        </label>
      </div>

      {#if cameraPoseError}
        <p class="text-xs text-error-300">{cameraPoseError}</p>
      {/if}
      {#if cameraPoseMessage}
        <p class="text-xs text-surface-400">{cameraPoseMessage}</p>
      {/if}

      <div class="flex flex-wrap gap-2">
        <button class="btn btn-sm preset-filled-primary-500 uppercase tracking-[0.3em]" type="submit" disabled={cameraPoseBusy || !cameraPoseDirty}>
          {cameraPoseBusy ? 'Saving…' : 'Save pose'}
        </button>
        <button class="btn btn-sm preset-tonal uppercase tracking-[0.3em]" type="button" onclick={onClear} disabled={cameraPoseBusy}>
          Clear
        </button>
      </div>
    </form>
  {:else}
    <div class="mt-4 rounded border border-surface-800/60 bg-surface-900/30 p-3 text-sm text-surface-400">
      Click a camera in the 3D viewer to edit its pose.
    </div>
  {/if}
</div>
