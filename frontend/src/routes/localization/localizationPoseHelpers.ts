import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import type { LocalizationMarker } from '$lib/features/localization/viewers/localizationViewerTypes';
import type { RigCameraInfo } from '$lib/types/rig';
import type { CameraExtrinsics } from '$lib/features/localization/types';
import type { PoseQuaternion, PoseTransform, Vec3 } from '$lib/features/localization/poseMath';

type CameraPoseInputs = {
  x: string;
  y: string;
  z: string;
  roll: string;
  pitch: string;
  yaw: string;
};

export type LocalizationPoseHelpersDeps = {
  getCameraExtrinsics: () => Record<string, CameraExtrinsics>;
  setCameraExtrinsics: (next: Record<string, CameraExtrinsics>) => void;
  getPrimaryCameraKey: () => string | null;
  getRigCameras: () => RigCameraInfo[];
  getOriginFromFieldCenterForEditor: () => PoseTransform;
  getCameraPoseInputs: () => CameraPoseInputs;
  setCameraPoseInputs: (next: CameraPoseInputs) => void;
  setCameraPoseEditorError: (message: string | null) => void;
  parseLengthToMeters: (value: string, unit?: string) => { meters: number } | null;
  formatMeters: (value: number, unit: string, digits: number) => string;
  toNumber: (value: string, fallback: number) => number;
  normalizeQuaternion: (q: PoseQuaternion) => PoseQuaternion;
  yawDegreesToQuaternion: (yaw: number) => PoseQuaternion;
  eulerDegreesToQuaternionXYZ: (euler: { pitch: number; yaw: number; roll: number }) => PoseQuaternion;
  quaternionToEulerDegreesXYZ: (q: PoseQuaternion) => { roll: number; pitch: number; yaw: number };
  invertTransform: (transform: PoseTransform) => PoseTransform;
  composeTransforms: (a: PoseTransform, b: PoseTransform) => PoseTransform;
};

export const createLocalizationPoseHelpers = (deps: LocalizationPoseHelpersDeps) => {
  const markerQuaternion = (marker: LocalizationMarker): PoseQuaternion => {
    const quaternion = (marker as unknown as { quaternion?: PoseQuaternion }).quaternion;
    if (
      quaternion &&
      [quaternion.x, quaternion.y, quaternion.z, quaternion.w].every((value) => typeof value === 'number' && Number.isFinite(value))
    ) {
      return deps.normalizeQuaternion(quaternion);
    }
    const yaw = typeof marker.heading === 'number' && Number.isFinite(marker.heading) ? marker.heading : 0;
    return deps.yawDegreesToQuaternion(yaw);
  };

  const cameraExtrinsicsTransform = (key: string): PoseTransform => {
    const config = deps.getCameraExtrinsics()[key] ?? { position: { x: 0, y: 0, z: 0 }, rotation: { roll: 0, pitch: 0, yaw: 0 } };
    const quaternion = deps.eulerDegreesToQuaternionXYZ({
      pitch: config.rotation.pitch,
      yaw: config.rotation.yaw,
      roll: config.rotation.roll
    });
    const position: Vec3 = [config.position.x, config.position.y, config.position.z];
    return { position, quaternion };
  };

  const rigCameraForMarker = (marker: LocalizationMarker): RigCameraInfo | null => {
    const source = marker.source;
    if (!source) return null;
    const refs = [
      source.cameraUid ?? null,
      source.cameraPath ?? null,
      source.streamId ?? null,
      source.streamLabel ?? null,
      source.id ?? null
    ]
      .map((value) => (typeof value === 'string' ? value.trim() : ''))
      .filter(Boolean);
    if (!refs.length) return null;

    for (const camera of deps.getRigCameras()) {
      const candidates = [
        camera.streamId ?? null,
        camera.cameraUid ?? null,
        camera.streamAlias ?? null,
        camera.driverCameraId ?? null,
        camera.hardwareId ?? null,
        camera.uid ?? null
      ]
        .map((value) => (typeof value === 'string' ? value.trim() : ''))
        .filter(Boolean);
      if (refs.some((ref) => candidates.includes(ref))) {
        return camera;
      }
    }
    return null;
  };

  const rigCameraForSource = (source: LocalizationPipelineSource | null): RigCameraInfo | null => {
    if (!source) return null;
    const refs = [source.cameraUid, source.cameraPath, source.streamId, source.streamLabel, source.id]
      .map((value) => (typeof value === 'string' ? value.trim() : ''))
      .filter(Boolean);
    if (!refs.length) return null;

    for (const camera of deps.getRigCameras()) {
      const candidates = [
        camera.streamId ?? null,
        camera.cameraUid ?? null,
        camera.streamAlias ?? null,
        camera.driverCameraId ?? null,
        camera.hardwareId ?? null,
        camera.uid ?? null
      ]
        .map((value) => (typeof value === 'string' ? value.trim() : ''))
        .filter(Boolean);
      if (refs.some((ref) => candidates.includes(ref))) {
        return camera;
      }
    }
    return null;
  };

  const applyPrimaryCameraPose = (): void => {
    const key = deps.getPrimaryCameraKey();
    if (!key) return;

    const { x: xRaw, y: yRaw, z: zRaw, roll: rollRaw, pitch: pitchRaw, yaw: yawRaw } = deps.getCameraPoseInputs();
    const x = deps.parseLengthToMeters(xRaw, 'm')?.meters ?? NaN;
    const y = deps.parseLengthToMeters(yRaw, 'm')?.meters ?? NaN;
    const z = deps.parseLengthToMeters(zRaw, 'm')?.meters ?? NaN;
    if (![x, y, z].every((value) => Number.isFinite(value))) {
      deps.setCameraPoseEditorError('Camera position must be valid lengths (example: 1.2m, 24in, 3ft).');
      return;
    }

    const roll = deps.toNumber(rollRaw, NaN);
    const pitch = deps.toNumber(pitchRaw, NaN);
    const yaw = deps.toNumber(yawRaw, NaN);
    if (![roll, pitch, yaw].every((value) => Number.isFinite(value))) {
      deps.setCameraPoseEditorError('Camera rotation must be valid degrees.');
      return;
    }

    const originFromCamera: PoseTransform = {
      position: [x, y, z],
      quaternion: deps.eulerDegreesToQuaternionXYZ({ pitch, yaw, roll })
    };
    const centerFromOrigin = deps.invertTransform(deps.getOriginFromFieldCenterForEditor());
    const centerFromCamera = deps.composeTransforms(centerFromOrigin, originFromCamera);
    const eulerCenter = deps.quaternionToEulerDegreesXYZ(centerFromCamera.quaternion);

    const nextEntry: CameraExtrinsics = {
      position: {
        x: centerFromCamera.position[0],
        y: centerFromCamera.position[1],
        z: centerFromCamera.position[2]
      },
      rotation: {
        roll: eulerCenter.roll,
        pitch: eulerCenter.pitch,
        yaw: eulerCenter.yaw
      }
    };
    const nextMap: Record<string, CameraExtrinsics> = { ...deps.getCameraExtrinsics(), [key]: nextEntry };
    deps.setCameraExtrinsics(nextMap);
    deps.setCameraPoseEditorError(null);

    deps.setCameraPoseInputs({
      x: deps.formatMeters(x, 'm', 3),
      y: deps.formatMeters(y, 'm', 3),
      z: deps.formatMeters(z, 'm', 3),
      roll: roll.toFixed(2),
      pitch: pitch.toFixed(2),
      yaw: yaw.toFixed(2)
    });
  };

  const resetPrimaryCameraPoseInputs = (): void => {
    deps.setCameraPoseInputs({
      x: deps.formatMeters(0, 'm', 3),
      y: deps.formatMeters(0, 'm', 3),
      z: deps.formatMeters(0, 'm', 3),
      roll: '0',
      pitch: '0',
      yaw: '0'
    });
    deps.setCameraPoseEditorError(null);
  };

  return {
    markerQuaternion,
    cameraExtrinsicsTransform,
    rigCameraForMarker,
    rigCameraForSource,
    applyPrimaryCameraPose,
    resetPrimaryCameraPoseInputs
  };
};
