import { estimateCalibrationFovDegs } from '$lib/features/devices/camera/cameraCalibrationUtils';
import type {
  LocalizationDetectionPose,
  LocalizationPoseSpace
} from '$lib/features/localization/localizationConfig';
import { isDeviceImuExternalStreamId } from '$lib/features/localization/externalSourceIds';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import {
  composeTransforms,
  eulerDegreesToQuaternionXYZ,
  invertTransform,
  normalizeQuaternion,
  quaternionToEulerDegreesXYZ,
  type PoseQuaternion,
  type PoseTransform,
  type Vec3
} from '$lib/features/localization/poseMath';
import { mediaImuParentStreamIdForSource } from '$lib/features/localization/utils';
import type { StreamInfo } from '$lib/api/client';
import type { RigCameraInfo } from '$lib/types/rig';
import { imuQuaternionToThree } from '$lib/utils/imuFrames';

export type CameraPovIntrinsics = {
  fx: number;
  fy: number;
  cx: number;
  cy: number;
  width: number;
  height: number;
};

export type CameraPovFovMode = 'undistorted' | 'raw' | 'none';

export type CameraPovState = {
  optionId: string;
  profileId: string;
  sourceId: string;
  cameraKey: string | null;
  transform: PoseTransform;
  intrinsicsUndistorted: CameraPovIntrinsics | null;
  intrinsicsRaw: CameraPovIntrinsics | null;
};

export type ParsedImuRotation = {
  roll: number;
  pitch: number;
  yaw: number;
  quaternion: PoseQuaternion | null;
  translation: { x: number; y: number; z: number } | null;
  sampleTimestampMs: number | null;
};

export type ImuRotationOrientationSample = {
  roll: number;
  pitch: number;
  yaw: number;
  quaternion: PoseQuaternion | null;
} | null;

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

const finiteNumber = (value: unknown): number | null => {
  const parsed =
    typeof value === 'number' ? value : typeof value === 'string' ? Number(value) : Number.NaN;
  return Number.isFinite(parsed) ? parsed : null;
};

const parseQuaternionLike = (value: unknown): PoseQuaternion | null => {
  const record = asRecord(value);
  if (!record) return null;
  const x = finiteNumber(record.x);
  const y = finiteNumber(record.y);
  const z = finiteNumber(record.z);
  const w = finiteNumber(record.w);
  if (x == null || y == null || z == null || w == null) return null;
  return { x, y, z, w };
};

const parseEulerLike = (value: unknown): { roll: number; pitch: number; yaw: number } | null => {
  const record = asRecord(value);
  if (!record) return null;
  const roll = finiteNumber(record.roll);
  const pitch = finiteNumber(record.pitch);
  const yaw = finiteNumber(record.yaw);
  if (roll != null && pitch != null && yaw != null) {
    return { roll, pitch, yaw };
  }

  if (!('w' in record)) {
    const x = finiteNumber(record.x);
    const y = finiteNumber(record.y);
    const z = finiteNumber(record.z);
    if (x != null && y != null && z != null) {
      return { roll: x, pitch: y, yaw: z };
    }
  }
  return null;
};

const parseTranslationLike = (value: unknown): { x: number; y: number; z: number } | null => {
  const record = asRecord(value);
  if (!record) return null;
  const x = finiteNumber(record.x);
  const y = finiteNumber(record.y);
  const z = finiteNumber(record.z);
  if (x == null || y == null || z == null) return null;
  return { x, y, z };
};

const toFinite = (value: unknown): number | null => {
  const parsed =
    typeof value === 'number' ? value : typeof value === 'string' ? Number(value) : Number.NaN;
  return Number.isFinite(parsed) ? parsed : null;
};

const extractResolutionCandidate = (value: unknown): { width: number; height: number } | null => {
  const record = asRecord(value);
  if (!record) return null;
  const width = toFinite(record.width ?? record.w ?? record.cols);
  const height = toFinite(record.height ?? record.h ?? record.rows);
  if (width == null || height == null) return null;
  if (width <= 0 || height <= 0) return null;
  return { width, height };
};

const resolvePovResolution = (
  manifest: Record<string, unknown> | null,
  calibration: Record<string, unknown> | null,
  cx: number,
  cy: number
): { width: number; height: number } | null => {
  const capture = asRecord(manifest?.capture);
  const captureMode = asRecord(capture?.mode);
  const captureModeFormat = asRecord(captureMode?.format);
  const captureFormat = asRecord(capture?.format);
  const encoder = asRecord(manifest?.encoder);
  const encoderSettings = asRecord(encoder?.settings) ?? asRecord(asRecord(manifest)?.encoder_settings);
  const candidates = [
    captureModeFormat?.resolution,
    captureMode?.resolution,
    captureModeFormat,
    captureFormat?.resolution,
    captureFormat,
    capture?.resolution,
    encoderSettings?.output_resolution,
    encoderSettings?.resolution,
    asRecord(encoder?.output)?.resolution,
    encoder?.resolution,
    calibration?.resolution,
    calibration?.imageSize,
    calibration?.frameSize,
    calibration
  ];
  for (const candidate of candidates) {
    const parsed = extractResolutionCandidate(candidate);
    if (parsed) return parsed;
  }

  const width = cx * 2;
  const height = cy * 2;
  if (Number.isFinite(width) && Number.isFinite(height) && width > 16 && height > 16) {
    return { width, height };
  }
  return null;
};

const fovDegToFocalPx = (sensorPx: number, fovDeg: number | null): number | null => {
  if (!Number.isFinite(sensorPx) || sensorPx <= 0) return null;
  if (!Number.isFinite(fovDeg) || fovDeg == null || fovDeg <= 0 || fovDeg >= 179.999) return null;
  const half = (fovDeg * Math.PI) / 360;
  const tan = Math.tan(half);
  if (!Number.isFinite(tan) || tan <= 0) return null;
  const focal = sensorPx / (2 * tan);
  if (!Number.isFinite(focal) || focal <= 0) return null;
  return focal;
};

const normalizeRay = (x: number, y: number, z: number): [number, number, number] | null => {
  const mag = Math.hypot(x, y, z);
  if (!Number.isFinite(mag) || mag <= 1e-12) return null;
  return [x / mag, y / mag, z / mag];
};

const angleBetweenDeg = (
  a: [number, number, number] | null,
  b: [number, number, number] | null
): number | null => {
  if (!a || !b) return null;
  const dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const clamped = Math.max(-1, Math.min(1, dot));
  const radians = Math.acos(clamped);
  if (!Number.isFinite(radians)) return null;
  return (radians * 180) / Math.PI;
};

const rayFromPixelPinholeDistorted = (
  u: number,
  v: number,
  intrinsics: {
    fx: number;
    fy: number;
    cx: number;
    cy: number;
    k1: number;
    k2: number;
    p1: number;
    p2: number;
    k3: number;
  }
): [number, number, number] | null => {
  const { fx, fy, cx, cy, k1, k2, p1, p2, k3 } = intrinsics;
  if (!Number.isFinite(fx) || !Number.isFinite(fy) || fx <= 0 || fy <= 0) return null;
  const xd = (u - cx) / fx;
  const yd = (v - cy) / fy;
  if (!Number.isFinite(xd) || !Number.isFinite(yd)) return null;

  let xu = xd;
  let yu = yd;
  for (let i = 0; i < 12; i += 1) {
    const r2 = xu * xu + yu * yu;
    const r4 = r2 * r2;
    const r6 = r4 * r2;
    const radial = 1 + k1 * r2 + k2 * r4 + k3 * r6;
    if (!Number.isFinite(radial) || Math.abs(radial) < 1e-10) break;
    const deltaX = 2 * p1 * xu * yu + p2 * (r2 + 2 * xu * xu);
    const deltaY = p1 * (r2 + 2 * yu * yu) + 2 * p2 * xu * yu;
    const nextXu = (xd - deltaX) / radial;
    const nextYu = (yd - deltaY) / radial;
    if (!Number.isFinite(nextXu) || !Number.isFinite(nextYu)) break;
    if (Math.abs(nextXu - xu) < 1e-10 && Math.abs(nextYu - yu) < 1e-10) {
      xu = nextXu;
      yu = nextYu;
      break;
    }
    xu = nextXu;
    yu = nextYu;
  }

  return normalizeRay(xu, yu, 1);
};

const estimatePinholeDistortedFovDegs = (
  resolution: { width: number; height: number },
  intrinsics: {
    fx: number;
    fy: number;
    cx: number;
    cy: number;
    k1: number;
    k2: number;
    p1: number;
    p2: number;
    k3: number;
  }
): { hfov: number | null; vfov: number | null } => {
  const width = resolution.width;
  const height = resolution.height;
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return { hfov: null, vfov: null };
  }

  const uL = 0.5;
  const uR = width - 0.5;
  const vT = 0.5;
  const vB = height - 0.5;
  const uM = width * 0.5;
  const vM = height * 0.5;

  const left = rayFromPixelPinholeDistorted(uL, vM, intrinsics);
  const right = rayFromPixelPinholeDistorted(uR, vM, intrinsics);
  const top = rayFromPixelPinholeDistorted(uM, vT, intrinsics);
  const bottom = rayFromPixelPinholeDistorted(uM, vB, intrinsics);
  const hfov = angleBetweenDeg(left, right);
  const vfov = angleBetweenDeg(top, bottom);
  const sane = (value: number | null): number | null =>
    value != null && Number.isFinite(value) && value > 0 && value < 179.999 ? value : null;
  return { hfov: sane(hfov), vfov: sane(vfov) };
};

const hasImuToken = (value: string | null | undefined): boolean => {
  const normalized = typeof value === 'string' ? value.trim().toLowerCase() : '';
  if (!normalized) return false;
  return normalized.split(/[^a-z0-9]+/).some((token) => token === 'imu');
};

export const parseImuRotationSample = (value: unknown): ParsedImuRotation | null => {
  const root = asRecord(value);
  if (!root) return null;
  const pose = asRecord(root.pose);
  const imu = asRecord(root.imu);

  const rotationCandidates: unknown[] = [
    root.rotation,
    root.orientation,
    pose?.rotation,
    pose?.orientation,
    imu?.rotation,
    imu?.orientation,
    root
  ];

  let roll: number | null = null;
  let pitch: number | null = null;
  let yaw: number | null = null;
  let quaternion: PoseQuaternion | null = null;
  for (const candidate of rotationCandidates) {
    const euler = parseEulerLike(candidate);
    const candidateRecord = asRecord(candidate);
    const quat = parseQuaternionLike(candidateRecord?.quaternion ?? candidate);
    if (euler) {
      roll = euler.roll;
      pitch = euler.pitch;
      yaw = euler.yaw;
      quaternion = quat;
      break;
    }
    if (quat) {
      const eulerFromQuat = quaternionToEulerDegreesXYZ(quat);
      roll = eulerFromQuat.roll;
      pitch = eulerFromQuat.pitch;
      yaw = eulerFromQuat.yaw;
      quaternion = quat;
      break;
    }
  }
  if (roll == null || pitch == null || yaw == null) return null;

  const translation =
    parseTranslationLike(root.translation) ??
    parseTranslationLike(root.position) ??
    parseTranslationLike(pose?.translation) ??
    parseTranslationLike(imu?.translation);

  const meta = asRecord(root.meta);
  const sampleTimestampMs =
    finiteNumber(root.timestampMs) ??
    finiteNumber(root.timestamp_ms) ??
    finiteNumber(root.sampleTimestampMs) ??
    finiteNumber(root.sample_timestamp_ms) ??
    finiteNumber(root.t_ms) ??
    finiteNumber(root.time_ms) ??
    finiteNumber(meta?.timestampMs) ??
    finiteNumber(meta?.timestamp_ms) ??
    null;

  return {
    roll,
    pitch,
    yaw,
    quaternion,
    translation,
    sampleTimestampMs
  };
};

export const isImuSource = (source: LocalizationPipelineSource): boolean =>
  source.streamId.startsWith('external:media-imu-') ||
  hasImuToken(source.streamId) ||
  hasImuToken(source.streamLabel) ||
  hasImuToken(source.cameraUid) ||
  hasImuToken(source.cameraPath) ||
  hasImuToken(source.pipelineLabel) ||
  hasImuToken(source.outputKey);

export const imuSourcePriority = (source: LocalizationPipelineSource): number => {
  if (source.streamId.startsWith('external:media-imu-')) return 0;
  if (isDeviceImuExternalStreamId(source.streamId)) return 1;
  if (source.streamId.startsWith('external:') && hasImuToken(source.streamLabel)) return 2;
  return 3;
};

export const imuSampleQuaternion = (sample: ImuRotationOrientationSample): PoseQuaternion | null => {
  if (!sample) return null;
  const fromSample = sample.quaternion;
  if (
    fromSample &&
    [fromSample.x, fromSample.y, fromSample.z, fromSample.w].every(
      (value) => typeof value === 'number' && Number.isFinite(value)
    )
  ) {
    const viewerQuat = imuQuaternionToThree({
      w: fromSample.w,
      x: fromSample.x,
      y: fromSample.y,
      z: fromSample.z
    });
    return normalizeQuaternion({
      x: viewerQuat.x,
      y: viewerQuat.y,
      z: viewerQuat.z,
      w: viewerQuat.w
    });
  }
  if (![sample.roll, sample.pitch, sample.yaw].every((value) => Number.isFinite(value))) {
    return null;
  }
  return normalizeQuaternion(
    eulerDegreesToQuaternionXYZ({
      pitch: sample.pitch,
      yaw: sample.yaw,
      roll: sample.roll
    })
  );
};

export const cameraKeyVariants = (value: string | null | undefined): string[] => {
  const trimmed = typeof value === 'string' ? value.trim() : '';
  if (!trimmed) return [];
  const out = new Set<string>([trimmed]);
  const stripped = trimmed.startsWith('device:')
    ? trimmed.slice('device:'.length)
    : trimmed.startsWith('stream:')
      ? trimmed.slice('stream:'.length)
      : trimmed;
  if (stripped) {
    out.add(stripped);
    out.add(`device:${stripped}`);
    out.add(`stream:${stripped}`);
  }
  return Array.from(out.values());
};

export const collectKeyVariants = (values: Array<string | null | undefined>): string[] => {
  const out = new Set<string>();
  for (const value of values) {
    for (const key of cameraKeyVariants(value)) {
      out.add(key);
    }
  }
  return Array.from(out.values());
};

export const cameraPovOptionId = (profileId: string, sourceId: string): string =>
  `${profileId}::${sourceId}`;

export const rigPoseToViewerTransform = (
  pose:
    | {
        translation?: { x?: number; y?: number; z?: number };
        rotation?: { roll?: number; pitch?: number; yaw?: number };
      }
    | null
    | undefined
): PoseTransform | null => {
  if (!pose) return null;
  const tx = pose.translation?.x;
  const ty = pose.translation?.y;
  const tz = pose.translation?.z;
  const roll = pose.rotation?.roll;
  const pitch = pose.rotation?.pitch;
  const yaw = pose.rotation?.yaw;
  if (![tx, ty, tz, roll, pitch, yaw].every((value) => typeof value === 'number' && Number.isFinite(value))) {
    return null;
  }

  const quaternion = eulerDegreesToQuaternionXYZ({ pitch: -pitch, yaw, roll });
  return {
    position: [ty, tz, tx] as Vec3,
    quaternion
  };
};

export const cameraFramePositionForDetection = (
  detection: LocalizationDetectionPose,
  coordinateSpace: LocalizationPoseSpace,
  selectedPov: CameraPovState | null
): Vec3 | null => {
  const pose = detection.pose;
  const tagFromParent: PoseTransform = {
    position: [pose.translation.x, pose.translation.y, pose.translation.z],
    quaternion: pose.rotation.quaternion
  };
  if (coordinateSpace === 'tag_in_camera') {
    return tagFromParent.position;
  }
  if (coordinateSpace === 'tag_in_robot') {
    const robotFromCamera = selectedPov?.transform ?? null;
    if (!robotFromCamera) return null;
    const cameraFromRobot = invertTransform(robotFromCamera);
    const cameraFromTag = composeTransforms(cameraFromRobot, tagFromParent);
    return cameraFromTag.position;
  }
  if (coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field') {
    const fieldFromCamera = selectedPov?.transform ?? null;
    if (!fieldFromCamera) return null;
    const cameraFromField = invertTransform(fieldFromCamera);
    const cameraFromTag = composeTransforms(cameraFromField, tagFromParent);
    return cameraFromTag.position;
  }
  return null;
};

export const detectionInsidePovFov = (
  detection: LocalizationDetectionPose,
  coordinateSpace: LocalizationPoseSpace,
  selectedPov: CameraPovState | null,
  intrinsics: CameraPovIntrinsics | null,
  forwardSign: 1 | -1 = 1
): boolean => {
  if (!intrinsics) return true;
  const position = cameraFramePositionForDetection(detection, coordinateSpace, selectedPov);
  if (!position) return true;
  const [x, y, z] = position;
  if (![x, y, z].every((value) => Number.isFinite(value))) return false;
  const depth = z * forwardSign;
  if (depth <= 0) return false;

  const { fx, fy, cx, cy, width, height } = intrinsics;
  if (![fx, fy, width, height].every((value) => Number.isFinite(value) && value > 0)) return true;
  if (![cx, cy].every((value) => Number.isFinite(value))) return true;

  const nx = (x * forwardSign) / depth;
  const ny = y / depth;
  const u = fx * nx + cx;
  const vDown = fy * ny + cy;
  const vUp = fy * -ny + cy;
  const insideX = u >= 0 && u <= width;
  const insideY = (vDown >= 0 && vDown <= height) || (vUp >= 0 && vUp <= height);
  return insideX && insideY;
};

export const extractPovIntrinsicsSet = (
  stream: StreamInfo
): { undistorted: CameraPovIntrinsics | null; raw: CameraPovIntrinsics | null } | null => {
  const manifest = asRecord(stream.manifest);
  const camera = asRecord(manifest?.camera);
  const calibration =
    asRecord(manifest?.calibration) ??
    asRecord(camera?.calibration) ??
    asRecord(camera?.intrinsics) ??
    asRecord(manifest?.intrinsics) ??
    null;
  if (!calibration) return null;

  const fx = toFinite(calibration.fx);
  const fy = toFinite(calibration.fy);
  const cx = toFinite(calibration.cx);
  const cy = toFinite(calibration.cy);
  if ([fx, fy].some((value) => value == null || value <= 0)) return null;
  if ([cx, cy].some((value) => value == null)) return null;

  const resolution = resolvePovResolution(manifest, calibration, cx, cy);
  if (!resolution) return null;
  const { width, height } = resolution;
  const undistorted: CameraPovIntrinsics = { fx, fy, cx, cy, width, height };

  const k1 = toFinite(calibration.k1) ?? 0;
  const k2 = toFinite(calibration.k2) ?? 0;
  const p1 = toFinite(calibration.p1) ?? 0;
  const p2 = toFinite(calibration.p2) ?? 0;
  const k3 = toFinite(calibration.k3) ?? 0;
  const lensModelRaw = String(calibration.lensModel ?? calibration.lens_model ?? 'pinhole')
    .trim()
    .toLowerCase();
  const lensModel = lensModelRaw === 'fisheye' ? 'fisheye' : 'pinhole';
  const hasPinholeDistortion = [k1, k2, p1, p2, k3].some((value) => Math.abs(value) > 1e-7);
  const fovEstimate =
    lensModel === 'pinhole' && hasPinholeDistortion
      ? estimatePinholeDistortedFovDegs({ width, height }, { fx, fy, cx, cy, k1, k2, p1, p2, k3 })
      : estimateCalibrationFovDegs(
          { width, height },
          {
            fx,
            fy,
            cx,
            cy,
            k1,
            k2,
            p1,
            p2,
            k3,
            lensModel
          }
        );
  const rawFx = fovDegToFocalPx(width, fovEstimate.hfov) ?? fx;
  const rawFy = fovDegToFocalPx(height, fovEstimate.vfov) ?? fy;
  const raw: CameraPovIntrinsics = { fx: rawFx, fy: rawFy, cx, cy, width, height };

  return { undistorted, raw };
};

export const selectedPovIntrinsics = (
  selectedPov: CameraPovState | null,
  mode: CameraPovFovMode
): CameraPovIntrinsics | null => {
  if (!selectedPov || mode === 'none') return null;
  if (mode === 'raw') {
    return selectedPov.intrinsicsRaw ?? selectedPov.intrinsicsUndistorted ?? null;
  }
  return selectedPov.intrinsicsUndistorted ?? selectedPov.intrinsicsRaw ?? null;
};

export const intrinsicsEqual = (
  a: CameraPovIntrinsics | null,
  b: CameraPovIntrinsics | null
): boolean => {
  if (a == null && b == null) return true;
  if (a == null || b == null) return false;
  return (
    a.fx === b.fx &&
    a.fy === b.fy &&
    a.cx === b.cx &&
    a.cy === b.cy &&
    a.width === b.width &&
    a.height === b.height
  );
};

export const streamIdentityKeys = (stream: StreamInfo): string[] => {
  const manifest = asRecord(stream.manifest);
  const identity = asRecord(manifest?.identity);
  const capture = asRecord(manifest?.capture);
  const identityKeys = Array.isArray(identity?.keys)
    ? identity.keys.map((value: unknown) => String(value ?? '').trim())
    : [];
  const captureKeys = Array.isArray(capture?.device_keys)
    ? capture.device_keys.map((value: unknown) => String(value ?? '').trim())
    : [];
  const identityDisplay = typeof identity?.display === 'string' ? identity.display : null;
  const identityHardwareId = typeof identity?.hardware_id === 'string' ? identity.hardware_id : null;
  const identityAlias = typeof identity?.alias === 'string' ? identity.alias : null;
  const identityId = typeof identity?.id === 'string' ? identity.id : null;
  return collectKeyVariants([
    stream.id,
    identityDisplay,
    identityHardwareId,
    identityAlias,
    identityId,
    ...identityKeys,
    ...captureKeys
  ]);
};

export const sourceKeys = (source: LocalizationPipelineSource | null): string[] => {
  if (!source) return [];
  return collectKeyVariants([
    source.id,
    source.cameraUid,
    source.streamId,
    mediaImuParentStreamIdForSource(source),
    source.cameraPath,
    ...(source.cameraKeys ?? [])
  ]);
};

export const rigCameraKeys = (camera: RigCameraInfo | null): string[] => {
  if (!camera) return [];
  return collectKeyVariants([
    camera.uid,
    camera.cameraUid ?? null,
    camera.streamId ?? null,
    camera.driverCameraId ?? null,
    camera.hardwareId ?? null,
    camera.streamAlias ?? null
  ]);
};
