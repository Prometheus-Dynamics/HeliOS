<script lang="ts">
  import type { RigCameraInfo } from '$lib/types/rig';

  type RobotFormState = {
    width: string;
    length: string;
    bumperHeight: string;
    bumperThickness: string;
    groundClearance: string;
  };

  type CameraLayoutEditorProps = {
    robotForm: RobotFormState;
    robotDirty: boolean;
    robotBusy: boolean;
    robotMessage: string | null;
    robotError: string | null;
    rigCameras: RigCameraInfo[];
    selectedCameraUid: string | null;
    onUpdateRobotField: (key: keyof RobotFormState, raw: string) => void;
    onSaveRobotDimensions: () => void;
    onRevertRobotForm: () => void;
    onSelectCamera: (uid: string) => void;
  };

  const {
    robotForm,
    robotDirty,
    robotBusy,
    robotMessage,
    robotError,
    rigCameras,
    selectedCameraUid,
    onUpdateRobotField,
    onSaveRobotDimensions,
    onRevertRobotForm,
    onSelectCamera
  }: CameraLayoutEditorProps = $props();

  function handleRobotFieldInput(key: keyof RobotFormState, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    onUpdateRobotField(key, target.value);
  }

  export type $$Props = CameraLayoutEditorProps;
</script>

<div class="rounded border border-surface-800 bg-surface-950/40 p-4">
  <form class="space-y-4" onsubmit={(event) => { event.preventDefault(); onSaveRobotDimensions(); }}>
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h3 class="text-xs uppercase tracking-[0.35em] text-surface-400">Robot geometry</h3>
      <span
        class={`text-micro uppercase tracking-[0.3em] ${
          robotDirty ? 'text-warning-300' : 'text-surface-500'
        }`}
      >
        {robotDirty ? 'Pending changes' : 'In sync'}
      </span>
    </div>
    <p class="text-sm text-surface-400">
      Adjust the chassis envelope shared across calibration tools, overlays, and 3D viewers.
    </p>
    <p class="text-xs text-surface-500">Type units directly (examples: 0.6 m, 24&quot;, 2 ft). If omitted, meters are assumed.</p>
    <div class="grid gap-3 sm:grid-cols-2">
      <label class="space-y-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Width</span>
        <input
          class="input w-full"
          type="text"
          placeholder="e.g. 0.6 m or 24&quot;"
          value={robotForm.width}
          oninput={(event) => handleRobotFieldInput('width', event)}
        />
      </label>
      <label class="space-y-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Length</span>
        <input
          class="input w-full"
          type="text"
          placeholder="e.g. 0.8 m or 30&quot;"
          value={robotForm.length}
          oninput={(event) => handleRobotFieldInput('length', event)}
        />
      </label>
      <label class="space-y-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Bumper height</span>
        <input
          class="input w-full"
          type="text"
          placeholder="e.g. 4 in"
          value={robotForm.bumperHeight}
          oninput={(event) => handleRobotFieldInput('bumperHeight', event)}
        />
      </label>
      <label class="space-y-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Bumper thickness</span>
        <input
          class="input w-full"
          type="text"
          placeholder="e.g. 2 in"
          value={robotForm.bumperThickness}
          oninput={(event) => handleRobotFieldInput('bumperThickness', event)}
        />
      </label>
      <label class="space-y-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Ground clearance</span>
        <input
          class="input w-full"
          type="text"
          placeholder="e.g. 0.0 m or 1.5 in"
          value={robotForm.groundClearance}
          oninput={(event) => handleRobotFieldInput('groundClearance', event)}
        />
      </label>
    </div>
    {#if robotError}
      <p class="text-xs text-error-400">{robotError}</p>
    {:else if robotMessage}
      <p class="text-xs text-success-400">{robotMessage}</p>
    {/if}
    <div class="flex flex-wrap items-center gap-2">
      <button
        class="btn preset-filled-primary-500 px-4 py-2 text-xs font-semibold uppercase tracking-[0.3em]"
        type="submit"
        disabled={robotBusy || !robotDirty}
      >
        {robotBusy ? 'Saving…' : 'Save'}
      </button>
      <button
        class="btn preset-tonal uppercase tracking-[0.3em]"
        type="button"
        onclick={onRevertRobotForm}
        disabled={robotBusy || !robotDirty}
      >
        Revert
      </button>
    </div>
  </form>
</div>

<div class="rounded border border-surface-800 bg-surface-950/40 p-4">
  <h3 class="text-xs uppercase tracking-[0.35em] text-surface-400">Cameras</h3>
  {#if !rigCameras.length}
    <p class="mt-2 text-sm text-surface-500">No calibrated cameras detected.</p>
  {:else}
    <div class="mt-3 max-h-[min(42vh,22rem)] overflow-y-auto pr-1">
      <ul class="space-y-2">
        {#each rigCameras as camera (camera.uid)}
          <li>
            <button
              type="button"
              class={`flex w-full items-start justify-between rounded border px-3 py-2 text-left text-sm transition hover:border-primary-400 hover:text-primary-100 ${
                camera.uid === selectedCameraUid
                  ? 'border-primary-500 bg-primary-500/10 text-primary-100'
                  : 'border-surface-800 text-surface-300'
              }`}
              onclick={() => onSelectCamera(camera.uid)}
            >
              <span class="font-medium">{camera.displayName}</span>
              <span class="text-micro uppercase tracking-[0.35em] text-surface-500">
                {camera.pose ? 'Calibrated' : 'Pending'}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>
