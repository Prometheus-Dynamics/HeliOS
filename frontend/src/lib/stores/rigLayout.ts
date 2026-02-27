import { get, writable, type Readable } from 'svelte/store';
import { DeviceService } from '$lib/ts-bindings/http/client';
import type { CameraLayoutResponse } from '$lib/ts-bindings/http/client';
import type { RigCameraInfo, RigLayout, RobotDimensions } from '$lib/types/rig';
import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rig';
import { buildErrorMessage } from '$lib/ui/errorPolicy';
import { resolveStreamLabel } from '$lib/utils/streamLabels';

export type RigLayoutState = {
  layout: RigLayout;
  loading: boolean;
  error: string | null;
  initialized: boolean;
};

const initialState: RigLayoutState = {
  layout: { robot: DEFAULT_ROBOT_DIMENSIONS, cameras: [] },
  loading: false,
  error: null,
  initialized: false
};

const store = writable<RigLayoutState>(initialState);
let pending: Promise<void> | null = null;

export type RigLayoutStore = Readable<RigLayoutState> & {
  refresh: (options?: { force?: boolean }) => Promise<void>;
};

export const rigLayoutStore: RigLayoutStore = {
  subscribe: store.subscribe,
  refresh
};

async function refresh(options: { force?: boolean } = {}): Promise<void> {
  const { force = false } = options;
  const current = get(store);
  if (current.loading && !force) {
    return pending ?? Promise.resolve();
  }

  if (pending) {
    return pending;
  }

  store.update((state) => ({
    ...state,
    loading: true,
    error: null,
    initialized: true
  }));

  pending = (async () => {
    try {
      const response = await DeviceService.getCameraLayout();
      if (!response) {
        store.set({
          layout: { robot: DEFAULT_ROBOT_DIMENSIONS, cameras: [] },
          loading: false,
          error: null,
          initialized: true
        });
        return;
      }
      const layout = mapLayout(response);
      store.set({ layout, loading: false, error: null, initialized: true });
    } catch (error) {
      store.update((state) => ({
        ...state,
        loading: false,
        error: formatError(error)
      }));
    } finally {
      pending = null;
    }
  })();

  return pending;
}

function mapLayout(response: CameraLayoutResponse): RigLayout {
  const robot = mapRobot(response.robot);
  const cameras = normalizeCameras((response.cameras ?? []).filter((camera) => Boolean(camera?.stream_id)));
  return { robot, cameras };
}

function normalizeCameras(entries: CameraLayoutResponse['cameras']): RigCameraInfo[] {
  if (!Array.isArray(entries)) {
    return [];
  }
  const camerasByUid = new Map<string, RigCameraInfo>();

  for (const entry of entries) {
    const camera = mapCameraEntry(entry);
    if (!camera) continue;
    const existing = camerasByUid.get(camera.uid);
    if (!existing) {
      camerasByUid.set(camera.uid, camera);
    } else {
      camerasByUid.set(camera.uid, mergeCameraEntries(existing, camera));
    }
  }

  return Array.from(camerasByUid.values()).sort((a, b) => a.displayName.localeCompare(b.displayName));
}

function mapCameraEntry(camera: CameraLayoutResponse['cameras'][number]): RigCameraInfo | null {
  if (!camera) {
    return null;
  }

  const streamId = trimOrNull(camera.stream_id);
  const cameraUid = trimOrNull(camera.camera_uid);
  const alias = trimOrNull(camera.stream_alias);
  const hardwareId = trimOrNull(camera.hardware_id);
  const driverCameraId = trimOrNull(camera.driver_camera_id) ?? cameraUid ?? hardwareId ?? streamId ?? alias;
  const identity = cameraUid ?? driverCameraId ?? hardwareId ?? streamId ?? alias;

  if (!identity) {
    return null;
  }

  const pose = camera.pose
    ? {
        translation: {
          x: camera.pose.translation?.x ?? 0,
          y: camera.pose.translation?.y ?? 0,
          z: camera.pose.translation?.z ?? 0
        },
        rotation: {
          roll: camera.pose.rotation?.roll ?? 0,
          pitch: camera.pose.rotation?.pitch ?? 0,
          yaw: camera.pose.rotation?.yaw ?? 0
        },
        updatedAt: camera.pose.updated_at ?? undefined
      }
    : {
        translation: { x: 0, y: 0, z: 0 },
        rotation: { roll: 0, pitch: 0, yaw: 0 },
        updatedAt: undefined
      };

  const displayName = resolveStreamLabel(camera, identity ?? 'Camera');

  const driverIdentifier = driverCameraId ?? cameraUid ?? hardwareId ?? identity;

  return {
    uid: identity,
    streamId,
    cameraUid,
    streamAlias: alias,
    driverCameraId: driverIdentifier,
    displayName,
    backend: trimOrNull(camera.backend) ?? 'unknown',
    hardwareId,
    pose
  };
}

function mergeCameraEntries(existing: RigCameraInfo, incoming: RigCameraInfo): RigCameraInfo {
  const pose = selectPose(existing.pose, incoming.pose);
  const streamAlias = existing.streamAlias ?? incoming.streamAlias ?? null;
  const streamId = existing.streamId ?? incoming.streamId ?? null;
  const cameraUid = existing.cameraUid ?? incoming.cameraUid ?? null;
  const hardwareId = existing.hardwareId ?? incoming.hardwareId ?? null;
  const backend =
    existing.backend === 'unknown' && incoming.backend !== 'unknown' ? incoming.backend : existing.backend;
  const driverCameraId = selectDriverCameraId(existing.driverCameraId, incoming.driverCameraId, existing.uid);
  const displayName = selectDisplayName(existing.displayName, incoming.displayName, driverCameraId);

  return {
    ...existing,
    streamId,
    cameraUid,
    streamAlias,
    hardwareId,
    backend,
    driverCameraId,
    pose,
    displayName
  };
}

function selectPose(
  existing: RigCameraInfo['pose'],
  incoming: RigCameraInfo['pose']
): RigCameraInfo['pose'] {
  if (!incoming) {
    return existing;
  }
  if (!existing) {
    return incoming;
  }
  const incomingTime = toTimestamp(incoming.updatedAt);
  const existingTime = toTimestamp(existing.updatedAt);
  if (incomingTime === null) {
    return existing;
  }
  if (existingTime === null) {
    return incoming;
  }
  return incomingTime >= existingTime ? incoming : existing;
}

function selectDisplayName(current: string, incoming: string, driverId: string): string {
  const currentScore = displayNameScore(current, driverId);
  const incomingScore = displayNameScore(incoming, driverId);
  return incomingScore > currentScore ? incoming : current;
}

function displayNameScore(value: string, driverId: string): number {
  if (!value) return 0;
  if (value === driverId) return 1;
  if (value.toLowerCase() === driverId.toLowerCase()) return 2;
  if (value.length > driverId.length) return 3;
  return 4;
}

function toTimestamp(value: string | null | undefined): number | null {
  if (!value) return null;
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : null;
}

function mapRobot(robot: CameraLayoutResponse['robot'] | undefined): RobotDimensions {
  if (!robot) return DEFAULT_ROBOT_DIMENSIONS;
  return {
    width: robot.width_m ?? DEFAULT_ROBOT_DIMENSIONS.width,
    length: robot.length_m ?? DEFAULT_ROBOT_DIMENSIONS.length,
    bumperHeight: robot.bumper_height_m ?? DEFAULT_ROBOT_DIMENSIONS.bumperHeight,
    bumperThickness: robot.bumper_thickness_m ?? DEFAULT_ROBOT_DIMENSIONS.bumperThickness,
    groundClearance: robot.ground_clearance_m ?? DEFAULT_ROBOT_DIMENSIONS.groundClearance
  };
}

function formatError(error: unknown): string {
  const message = sanitizeMessage(buildErrorMessage({ error, fallback: 'Unable to load camera layout.' }));
  return message ?? 'Unable to load camera layout.';
}

function sanitizeMessage(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!trimmed.length) return null;
  const lower = trimmed.toLowerCase();
  if (lower.startsWith('<!doctype') || lower.startsWith('<html') || lower.includes('</html>')) {
    return null;
  }
  return trimmed.length > 240 ? `${trimmed.slice(0, 240)}…` : trimmed;
}

function trimOrNull(value: string | null | undefined): string | null {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length ? trimmed : null;
}
function selectDriverCameraId(
  current: string | null | undefined,
  incoming: string | null | undefined,
  identity: string
): string {
  const normalizedCurrent = current ?? identity;
  const normalizedIncoming = incoming ?? identity;
  const currentIsFallback = normalizedCurrent === identity;
  const incomingIsFallback = normalizedIncoming === identity;
  if (currentIsFallback && !incomingIsFallback) {
    return normalizedIncoming;
  }
  if (!currentIsFallback && incomingIsFallback) {
    return normalizedCurrent;
  }
  if (!currentIsFallback && !incomingIsFallback) {
    return normalizedCurrent;
  }
  return normalizedCurrent;
}
