<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import Panel from '$lib/components/Panel.svelte';
  import CameraRigViewer from '$lib/components/CameraRigViewer.svelte';
  import { rigLayoutStore, type RigLayoutState } from '$lib/stores/rigLayout';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
  import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rig';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { reportError } from '$lib/ui/errorPolicy';
  import { toaster } from '$lib';

  type CameraPoseFormState = {
    x: string;
    y: string;
    z: string;
    roll: string;
    pitch: string;
    yaw: string;
  };

  const METERS_PER_INCH = 0.0254;
  const METERS_PER_FOOT = 0.3048;

  function formatLengthMeters(value: number): string {
    if (!Number.isFinite(value)) return '';
    return `${value.toFixed(3)} m`;
  }

  function formatAngleDegrees(value: number): string {
    if (!Number.isFinite(value)) return '';
    return `${value.toFixed(2)} deg`;
  }

  function parseLengthToMeters(raw: string): number | null {
    const trimmed = raw.trim();
    if (!trimmed.length) return null;
    const normalized = trimmed
      .toLowerCase()
      .replace(/[′’]/g, "'")
      .replace(/[″”]/g, '"');
    const match = normalized.match(/^([+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?)\s*([a-z"']*)$/);
    if (!match) return Number.NaN;

    const value = Number.parseFloat(match[1]);
    if (!Number.isFinite(value)) return Number.NaN;

    const unit = match[2].replace(/\./g, '');
    switch (unit) {
      case '':
      case 'm':
      case 'meter':
      case 'meters':
        return value;
      case 'cm':
      case 'centimeter':
      case 'centimeters':
        return value / 100;
      case 'mm':
      case 'millimeter':
      case 'millimeters':
        return value / 1000;
      case 'in':
      case 'inch':
      case 'inches':
      case '"':
        return value * METERS_PER_INCH;
      case 'ft':
      case 'foot':
      case 'feet':
      case "'":
        return value * METERS_PER_FOOT;
      default:
        return Number.NaN;
    }
  }

  function parseAngleToDegrees(raw: string): number | null {
    const trimmed = raw.trim();
    if (!trimmed.length) return null;
    const normalized = trimmed.toLowerCase().replace(/[°]/g, 'deg').trim();
    const match = normalized.match(/^([+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?)\s*([a-z]*)$/);
    if (!match) return Number.NaN;
    const value = Number.parseFloat(match[1]);
    if (!Number.isFinite(value)) return Number.NaN;
    const unit = match[2];
    switch (unit) {
      case '':
      case 'deg':
      case 'degree':
      case 'degrees':
        return value;
      default:
        return Number.NaN;
    }
  }

  const DEFAULT_POSE_FORM: CameraPoseFormState = {
    x: '0.000 m',
    y: '0.000 m',
    z: '0.000 m',
    roll: '0.00 deg',
    pitch: '0.00 deg',
    yaw: '0.00 deg'
  };

  const { cameraRef } = $props<{ cameraRef: string }>();

  let rigLayoutState = $state<RigLayoutState>({
    layout: { robot: { ...DEFAULT_ROBOT_DIMENSIONS }, cameras: [] },
    loading: false,
    error: null,
    initialized: false
  });

  let cameraPoseForm = $state<CameraPoseFormState>({ ...DEFAULT_POSE_FORM });
  let cameraPoseDirty = $state(false);
  let cameraPoseBusy = $state(false);
  let cameraPoseMessage = $state<string | null>(null);
  let cameraPoseError = $state<string | null>(null);

  const unsubscribe = rigLayoutStore.subscribe((state) => {
    rigLayoutState = state;
  });

  onMount(() => {
    if (!rigLayoutState.initialized) {
      void rigLayoutStore.refresh();
    }
  });

  onDestroy(() => {
    unsubscribe();
  });

  const rigLayout = $derived(rigLayoutState.layout);
  const rigRobot = $derived(rigLayout.robot as RobotDimensions);
  const rigCameras = $derived(rigLayout.cameras as RigCameraInfo[]);

  function matchRank(camera: RigCameraInfo, ref: string): number {
    const normalized = ref.trim();
    const candidates: Array<string | null | undefined> = [
      camera.streamId,
      camera.cameraUid,
      camera.streamAlias ?? null,
      camera.driverCameraId,
      camera.hardwareId ?? null,
      camera.uid
    ];
    const idx = candidates.findIndex((value) => (value ?? '').trim() === normalized);
    return idx === -1 ? 999 : idx;
  }

  const selectedCamera = $derived((() => {
    const ref = cameraRef.trim();
    if (!ref.length) return null;
    const matches = rigCameras.filter((camera) => matchRank(camera, ref) !== 999);
    matches.sort((a, b) => matchRank(a, ref) - matchRank(b, ref));
    return matches[0] ?? null;
  })());

  const selectedCameraId = $derived(selectedCamera?.uid ?? null);
  const selectedStreamId = $derived(selectedCamera?.streamId ?? null);

  function poseFormFromCamera(camera: RigCameraInfo | null): CameraPoseFormState {
    const pose = camera?.pose;
    const translation = pose?.translation ?? { x: 0, y: 0, z: 0 };
    const rotation = pose?.rotation ?? { roll: 0, pitch: 0, yaw: 0 };
    return {
      x: formatLengthMeters(translation.x),
      y: formatLengthMeters(translation.y),
      z: formatLengthMeters(translation.z),
      roll: formatAngleDegrees(rotation.roll),
      pitch: formatAngleDegrees(rotation.pitch),
      yaw: formatAngleDegrees(rotation.yaw)
    };
  }

  $effect(() => {
    if (!selectedCamera) {
      cameraPoseDirty = false;
      cameraPoseMessage = null;
      cameraPoseError = null;
      cameraPoseForm = { ...DEFAULT_POSE_FORM };
      return;
    }
    if (!cameraPoseDirty && !cameraPoseBusy) {
      cameraPoseForm = poseFormFromCamera(selectedCamera);
      cameraPoseMessage = null;
      cameraPoseError = null;
    }
  });

  function updateCameraPoseField(key: keyof CameraPoseFormState, raw: string): void {
    if (cameraPoseForm[key] === raw) return;
    cameraPoseForm = { ...cameraPoseForm, [key]: raw };
    cameraPoseDirty = true;
    cameraPoseMessage = null;
    cameraPoseError = null;
  }

  function buildPreviewPose(): RigCameraInfo['pose'] | null {
    const parseTranslation = (raw: string): number | null => {
      const value = parseLengthToMeters(raw);
      if (value === null) return 0;
      return Number.isFinite(value) ? value : null;
    };
    const parseRotation = (raw: string): number | null => {
      const value = parseAngleToDegrees(raw);
      if (value === null) return 0;
      return Number.isFinite(value) ? value : null;
    };

    const tx = parseTranslation(cameraPoseForm.x);
    const ty = parseTranslation(cameraPoseForm.y);
    const tz = parseTranslation(cameraPoseForm.z);
    const roll = parseRotation(cameraPoseForm.roll);
    const pitch = parseRotation(cameraPoseForm.pitch);
    const yaw = parseRotation(cameraPoseForm.yaw);
    if ([tx, ty, tz, roll, pitch, yaw].some((value) => value == null)) return null;

    return {
      translation: {
        x: Number((tx as number).toFixed(6)),
        y: Number((ty as number).toFixed(6)),
        z: Number((tz as number).toFixed(6))
      },
      rotation: {
        roll: Number((roll as number).toFixed(4)),
        pitch: Number((pitch as number).toFixed(4)),
        yaw: Number((yaw as number).toFixed(4))
      }
    };
  }

  const previewCameras = $derived((() => {
    const id = selectedCameraId;
    if (!id) return rigCameras;
    const previewPose = buildPreviewPose();
    if (!previewPose) return rigCameras;
    return rigCameras.map((camera) => (camera.uid === id ? { ...camera, pose: previewPose } : camera));
  })());

  async function saveCameraPose(): Promise<void> {
    cameraPoseError = null;
    cameraPoseMessage = null;

    if (!selectedCamera) return;
    if (!selectedStreamId) {
      cameraPoseError = 'This camera is missing a stream id; pose cannot be saved.';
      return;
    }

    const parseTranslation = (label: string, raw: string): number => {
      const value = parseLengthToMeters(raw);
      if (value === null) return 0;
      if (!Number.isFinite(value)) {
        throw new Error(`${label} must be a length (examples: 0.2 m, 8", 0.5 ft).`);
      }
      return value;
    };
    const parseRotation = (label: string, raw: string): number => {
      const value = parseAngleToDegrees(raw);
      if (value === null) return 0;
      if (!Number.isFinite(value)) {
        throw new Error(`${label} must be an angle in degrees (examples: 10, 10 deg, 10°).`);
      }
      return value;
    };

    let x: number;
    let y: number;
    let z: number;
    let roll: number;
    let pitch: number;
    let yaw: number;
    try {
      x = parseTranslation('X', cameraPoseForm.x);
      y = parseTranslation('Y', cameraPoseForm.y);
      z = parseTranslation('Z', cameraPoseForm.z);
      roll = parseRotation('Roll', cameraPoseForm.roll);
      pitch = parseRotation('Pitch', cameraPoseForm.pitch);
      yaw = parseRotation('Yaw', cameraPoseForm.yaw);
    } catch (err) {
      cameraPoseError = err instanceof Error ? err.message : 'Invalid pose values.';
      return;
    }

    cameraPoseBusy = true;
    try {
      await StreamsApi.updateStreamPose({
        id: selectedStreamId,
        requestBody: {
          translation: {
            x: Number(x.toFixed(6)),
            y: Number(y.toFixed(6)),
            z: Number(z.toFixed(6))
          },
          rotation: {
            roll: Number(roll.toFixed(4)),
            pitch: Number(pitch.toFixed(4)),
            yaw: Number(yaw.toFixed(4))
          }
        }
      });
      cameraPoseDirty = false;
      cameraPoseMessage = `Saved ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
      toaster.success({ title: 'Pose saved', description: selectedCamera.displayName });
      await rigLayoutStore.refresh({ force: true }).catch(() => {});
    } catch (error) {
      reportError({
        title: 'Save failed',
        error,
        fallback: 'Unable to save the camera pose right now.',
        inline: (message) => {
          cameraPoseError = message;
        }
      });
    } finally {
      cameraPoseBusy = false;
    }
  }

  async function clearCameraPose(): Promise<void> {
    cameraPoseError = null;
    cameraPoseMessage = null;

    if (!selectedCamera) return;
    if (!selectedStreamId) {
      cameraPoseError = 'This camera is missing a stream id; pose cannot be cleared.';
      return;
    }

    cameraPoseBusy = true;
    try {
      await StreamsApi.clearStreamPose({ id: selectedStreamId });
      cameraPoseDirty = false;
      cameraPoseMessage = 'Cleared';
      toaster.success({ title: 'Pose cleared', description: selectedCamera.displayName });
      await rigLayoutStore.refresh({ force: true }).catch(() => {});
    } catch (error) {
      reportError({
        title: 'Clear failed',
        error,
        fallback: 'Unable to clear the camera pose right now.',
        inline: (message) => {
          cameraPoseError = message;
        }
      });
    } finally {
      cameraPoseBusy = false;
    }
  }
</script>

<Panel className="h-full min-h-0 flex flex-col">
  <div class="flex h-full min-h-0 flex-col gap-3">
    {#if rigLayoutState.error}
      <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-sm text-error-200">
        Camera layout unavailable: {rigLayoutState.error}
      </div>
    {/if}

    <div class="flex-1 min-h-0 overflow-hidden">
      <CameraRigViewer robot={rigRobot} cameras={previewCameras} selectedCamera={selectedCameraId} />
    </div>

    <div class="shrink-0 rounded border border-surface-800 bg-surface-950/40 p-4">
      {#if !selectedCamera}
        <p class="text-sm text-surface-400">
          No camera layout entry matched <span class="font-mono">{cameraRef}</span>.
        </p>
        <p class="mt-1 text-xs text-surface-500">
          This camera is not in the rig layout, so pose editing is unavailable. Add it to the layout or select a different stream.
        </p>
      {:else}
        <div class="flex flex-wrap items-center justify-between gap-2">
          <h3 class="text-xs uppercase tracking-[0.35em] text-surface-400">Camera pose</h3>
          <span class="text-micro uppercase tracking-[0.3em] text-surface-500">{selectedCamera.displayName}</span>
        </div>
        <p class="mt-2 text-sm text-surface-400">
          Set the camera transform relative to the robot origin. Rotation values are in degrees.
        </p>

        <form class="mt-4 space-y-4" onsubmit={(event) => { event.preventDefault(); void saveCameraPose(); }}>
          <p class="text-xs text-surface-500">Translation accepts inline units (examples: 0.2 m, 8&quot;, 0.5 ft). Rotation uses degrees (10, 10 deg, 10°).</p>

          <div class="grid gap-3 sm:grid-cols-3">
            <label class="flex flex-col gap-1">
              <span class="text-micro uppercase tracking-[0.3em] text-surface-500">X</span>
              <input
                class="input input-sm"
                type="text"
                placeholder="e.g. 0.2 m or 8&quot;"
                value={cameraPoseForm.x}
                oninput={(event) => updateCameraPoseField('x', (event.currentTarget as HTMLInputElement).value)}
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
                oninput={(event) => updateCameraPoseField('y', (event.currentTarget as HTMLInputElement).value)}
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
                oninput={(event) => updateCameraPoseField('z', (event.currentTarget as HTMLInputElement).value)}
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
                oninput={(event) => updateCameraPoseField('roll', (event.currentTarget as HTMLInputElement).value)}
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
                oninput={(event) => updateCameraPoseField('pitch', (event.currentTarget as HTMLInputElement).value)}
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
                oninput={(event) => updateCameraPoseField('yaw', (event.currentTarget as HTMLInputElement).value)}
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
            <button class="btn btn-sm preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => void clearCameraPose()} disabled={cameraPoseBusy}>
              Clear
            </button>
          </div>
        </form>
      {/if}
    </div>
  </div>
</Panel>
