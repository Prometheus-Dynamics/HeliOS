<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import CameraLayoutEditor from './CameraLayoutEditor.svelte';
  import CameraLayoutViewport from './CameraLayoutViewport.svelte';
  import CameraPoseForm from './CameraPoseForm.svelte';
  import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rig';
  import { rigLayoutStore } from '$lib/stores/rigLayout';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
  import { DeviceService, type UpdateRobotDimensionsRequest } from '$lib/ts-bindings/http/client';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';

  type RigLayoutViewState = {
    layout: { robot: RobotDimensions; cameras: RigCameraInfo[] };
    loading: boolean;
    error: string | null;
    initialized: boolean;
  };

  type RobotFormState = {
    width: string;
    length: string;
    bumperHeight: string;
    bumperThickness: string;
    groundClearance: string;
  };

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

  function toRobotForm(robot: RobotDimensions): RobotFormState {
    return {
      width: formatLengthMeters(robot.width),
      length: formatLengthMeters(robot.length),
      bumperHeight: formatLengthMeters(robot.bumperHeight),
      bumperThickness: formatLengthMeters(robot.bumperThickness),
      groundClearance: formatLengthMeters(robot.groundClearance)
    };
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

  let rigLayoutState = $state<RigLayoutViewState>({
    layout: { robot: { ...DEFAULT_ROBOT_DIMENSIONS }, cameras: [] },
    loading: false,
    error: null,
    initialized: false
  });

  let robotForm = $state<RobotFormState>(toRobotForm(DEFAULT_ROBOT_DIMENSIONS));
  let robotDirty = $state(false);
  let robotBusy = $state(false);
  let robotMessage = $state<string | null>(null);
  let robotError = $state<string | null>(null);

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
  const rigRobot = $derived(rigLayout.robot);
  const rigCameras = $derived(rigLayout.cameras);
  let selectedCameraUid = $state<string | null>(null);
  const previewRobot = $derived(robotFormToMeters(robotForm, rigRobot));

  $effect(() => {
    const nextRobot = rigRobot;
    if (!robotDirty && !robotBusy) {
      robotForm = toRobotForm(nextRobot);
    }
  });

  function updateRobotField(key: keyof RobotFormState, raw: string): void {
    if (robotForm[key] === raw) return;
    robotForm = { ...robotForm, [key]: raw };
    robotDirty = true;
    robotMessage = null;
    robotError = null;
  }

  function robotFormToMeters(form: RobotFormState, fallback: RobotDimensions): RobotDimensions {
    const parse = (raw: string, defaultValue: number, options: { allowZero?: boolean } = {}): number => {
      const value = parseLengthToMeters(raw);
      if (value === null) return defaultValue;
      if (!Number.isFinite(value)) return defaultValue;
      if (!options.allowZero && value <= 0) return defaultValue;
      if (options.allowZero && value < 0) return defaultValue;
      return value;
    };
    return {
      width: parse(form.width, fallback.width),
      length: parse(form.length, fallback.length),
      bumperHeight: parse(form.bumperHeight, fallback.bumperHeight),
      bumperThickness: parse(form.bumperThickness, fallback.bumperThickness),
      groundClearance: parse(form.groundClearance, fallback.groundClearance, { allowZero: true })
    };
  }

  function revertRobotForm(): void {
    robotForm = toRobotForm(rigRobot);
    robotDirty = false;
    robotMessage = null;
    robotError = null;
  }

  function parseRobotInput(label: string, raw: string, options: { allowZero?: boolean } = {}): number {
    const value = parseLengthToMeters(raw);
    if (value === null) {
      throw new Error(`${label} is required.`);
    }
    if (!Number.isFinite(value)) {
      throw new Error(`${label} must be a length (examples: 0.6 m, 24", 2 ft).`);
    }
    if (!options.allowZero && value <= 0) {
      throw new Error(`${label} must be positive.`);
    }
    if (options.allowZero && value < 0) {
      throw new Error(`${label} must be zero or greater.`);
    }
    return value;
  }

  async function saveRobotDimensions(): Promise<void> {
    robotError = null;
    robotMessage = null;

    let width: number;
    let length: number;
    let bumperHeight: number;
    let bumperThickness: number;
    let groundClearance: number;

    try {
      width = parseRobotInput('Robot width', robotForm.width);
      length = parseRobotInput('Robot length', robotForm.length);
      bumperHeight = parseRobotInput('Bumper height', robotForm.bumperHeight);
      bumperThickness = parseRobotInput('Bumper thickness', robotForm.bumperThickness);
      groundClearance = parseRobotInput('Ground clearance', robotForm.groundClearance, { allowZero: true });
    } catch (validationError) {
      robotError = validationError instanceof Error ? validationError.message : 'Invalid robot dimensions.';
      return;
    }

    const request: UpdateRobotDimensionsRequest = {
      width_m: Number(width.toFixed(4)),
      length_m: Number(length.toFixed(4)),
      bumper_height_m: Number(bumperHeight.toFixed(4)),
      bumper_thickness_m: Number(bumperThickness.toFixed(4)),
      ground_clearance_m: Number(groundClearance.toFixed(4))
    };

    robotBusy = true;
    try {
      await DeviceService.updateRobotDimensions({ requestBody: request });
      robotDirty = false;
      robotMessage = `Saved ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
      robotForm = toRobotForm({
        width,
        length,
        bumperHeight,
        bumperThickness,
        groundClearance
      });
      await rigLayoutStore.refresh({ force: true }).catch(() => {
        // refreshing is best-effort; the viewer continues rendering with local state
      });
    } catch (error) {
      robotError = buildErrorMessage({ error, fallback: 'Unable to save robot dimensions.' });
    } finally {
      robotBusy = false;
    }
  }

  let selectedCamera = $state<RigCameraInfo | null>(null);
  const DEFAULT_POSE_FORM: CameraPoseFormState = {
    x: '0.000 m',
    y: '0.000 m',
    z: '0.000 m',
    roll: '0.00 deg',
    pitch: '0.00 deg',
    yaw: '0.00 deg'
  };
  let cameraPoseForm = $state<CameraPoseFormState>({ ...DEFAULT_POSE_FORM });
  let cameraPoseDirty = $state(false);
  let cameraPoseBusy = $state(false);
  let cameraPoseMessage = $state<string | null>(null);
  let cameraPoseError = $state<string | null>(null);

  $effect(() => {
    const current = rigCameras;
    if (!current.length) {
      selectedCameraUid = null;
      selectedCamera = null;
      return;
    }
    if (!selectedCameraUid || !current.some((camera) => camera.uid === selectedCameraUid)) {
      const next = current.find((camera) => camera.pose) ?? current[0];
      selectedCameraUid = next?.uid ?? null;
    }
    selectedCamera = current.find((camera) => camera.uid === selectedCameraUid) ?? null;
  });

  function updateCameraPoseField(key: keyof CameraPoseFormState, raw: string): void {
    if (cameraPoseForm[key] === raw) return;
    cameraPoseForm = { ...cameraPoseForm, [key]: raw };
    cameraPoseDirty = true;
    cameraPoseMessage = null;
    cameraPoseError = null;
  }

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

  async function saveCameraPose(): Promise<void> {
    if (!selectedCamera) return;
    cameraPoseError = null;
    cameraPoseMessage = null;
    if (!selectedCamera.streamId) {
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
        id: selectedCamera.streamId,
        requestBody: {
          translation: { x: Number(x.toFixed(6)), y: Number(y.toFixed(6)), z: Number(z.toFixed(6)) },
          rotation: { roll: Number(roll.toFixed(4)), pitch: Number(pitch.toFixed(4)), yaw: Number(yaw.toFixed(4)) }
        }
      });
      cameraPoseDirty = false;
      cameraPoseMessage = `Saved ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
      await rigLayoutStore.refresh({ force: true }).catch(() => {});
    } catch (error) {
      cameraPoseError = buildErrorMessage({ error, fallback: 'Unable to save camera pose.' });
    } finally {
      cameraPoseBusy = false;
    }
  }

  async function clearSelectedCameraPose(): Promise<void> {
    if (!selectedCamera) return;
    cameraPoseError = null;
    cameraPoseMessage = null;
    if (!selectedCamera.streamId) {
      cameraPoseError = 'This camera is missing a stream id; pose cannot be cleared.';
      return;
    }
    cameraPoseBusy = true;
    try {
      await StreamsApi.clearStreamPose({ id: selectedCamera.streamId });
      cameraPoseDirty = false;
      cameraPoseMessage = 'Cleared';
      await rigLayoutStore.refresh({ force: true }).catch(() => {});
    } catch (error) {
      cameraPoseError = buildErrorMessage({ error, fallback: 'Unable to clear camera pose.' });
    } finally {
      cameraPoseBusy = false;
    }
  }

  function handleViewerSelect(uid: string | null) {
    selectedCameraUid = uid ?? null;
  }

</script>

<div class="flex h-full min-h-0 flex-1 flex-col gap-6 overflow-hidden">
  <div class="grid min-h-0 flex-1 gap-6 overflow-hidden xl:grid-cols-[minmax(0,2.4fr)_minmax(0,1.6fr)]">
    <CameraLayoutViewport
      robot={previewRobot}
      cameras={rigCameras}
      selectedCamera={selectedCameraUid}
      error={rigLayoutState.error}
      onSelect={handleViewerSelect}
    />
    <div class="flex min-h-0 min-w-0 flex-col gap-6 overflow-y-auto pr-1">
      <CameraLayoutEditor
        robotForm={robotForm}
        robotDirty={robotDirty}
        robotBusy={robotBusy}
        robotMessage={robotMessage}
        robotError={robotError}
        rigCameras={rigCameras}
        selectedCameraUid={selectedCameraUid}
        onUpdateRobotField={updateRobotField}
        onSaveRobotDimensions={() => void saveRobotDimensions()}
        onRevertRobotForm={revertRobotForm}
        onSelectCamera={(uid) => (selectedCameraUid = uid)}
      />
      <CameraPoseForm
        selectedCamera={selectedCamera}
        cameraPoseForm={cameraPoseForm}
        cameraPoseBusy={cameraPoseBusy}
        cameraPoseDirty={cameraPoseDirty}
        cameraPoseError={cameraPoseError}
        cameraPoseMessage={cameraPoseMessage}
        onUpdateField={updateCameraPoseField}
        onSave={() => void saveCameraPose()}
        onClear={() => void clearSelectedCameraPose()}
      />
    </div>
  </div>
</div>
