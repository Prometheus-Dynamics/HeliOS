import type { LocalizationMarker } from '$lib';
import type { StreamMetrics } from '$lib/ts-bindings/http/client';
import type { RigCameraInfo } from '$lib/types/rig';
import type { FieldMapDocument } from '$lib/features/localization/fieldMaps';
import type { CustomField } from '$lib/features/localization/types';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import type {
  LocalizationFieldOriginConfig,
  LocalizationFieldOriginMode,
  LocalizationProfile,
  LocalizationSolveResponse,
  LocalizationSourceSampleStatus
} from '$lib/features/localization/localizationConfig';
import {
  identityTransform,
  type PoseTransform,
  type PoseQuaternion,
  type Vec3
} from '$lib/features/localization/poseMath';
import {
  frcOriginDefinition,
  transformFromFieldCenter,
  type PlanarFieldOrigin
} from '$lib/features/localization/fieldOrigins';
import {
  cameraKeyForSource,
  mediaImuParentStreamIdForSource,
  profileColorForId
} from '$lib/features/localization/utils';
import {
  hasDeviceImuExternalStreamPrefix,
  isDeviceImuCameraUid
} from '$lib/features/localization/externalSourceIds';
import { pipelineGraphTotalMs } from '$lib/features/localization/page/localizationMetricsUtils';

export type LocalizationBaseFrame = 'camera' | 'robot' | 'field';

export type LocalizationSourceGroup = {
  key: string;
  kind: 'stream' | 'profile' | 'peer' | 'peripheral';
  label: string;
  path: string;
  pipelines: { key: string; label: string; sources: LocalizationPipelineSource[] }[];
};

export type SourceStatusRow = {
  source: LocalizationPipelineSource;
  pollMs: number;
  detections: number;
  tagSize: number | null;
  graphMs: number | null;
  metricsUpdatedAt: number | null;
  metricsError: string | null;
  error: string | null;
};

export type ViewProfileOverlay = {
  id: string;
  label?: string;
  color?: string;
  transform: { position: Vec3; quaternion?: PoseQuaternion } | null;
};

export type ViewerTransform = { position: Vec3; quaternion?: PoseQuaternion } | null;

export const groupLocalizationSources = (
  compatibleSources: LocalizationPipelineSource[]
): LocalizationSourceGroup[] => {
  const UUID_LIKE_RE =
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

  const sourceKind = (source: LocalizationPipelineSource): LocalizationSourceGroup['kind'] => {
    const streamId = (source.streamId ?? '').trim();
    const id = (source.id ?? '').trim();
    const cameraPath = (source.cameraPath ?? '').trim();
    const pipelineId = (source.pipelineId ?? '').trim();
    const outputKey = (source.outputKey ?? '').trim();
    const pipelineLabel = (source.pipelineLabel ?? '').trim().toLowerCase();

    if (
      streamId.startsWith('profile:') ||
      id.startsWith('profile:') ||
      cameraPath.startsWith('profile:') ||
      pipelineId.startsWith('profile-solver:') ||
      outputKey.startsWith('solver:')
    ) {
      return 'profile';
    }
    if (streamId.startsWith('peer:') || id.startsWith('peer:')) return 'peer';
    if (pipelineId === 'peer' || pipelineLabel === 'peer' || pipelineLabel.includes('(peer)')) return 'peer';
    if (streamId.startsWith('external:') || id.startsWith('external:') || cameraPath.startsWith('device:')) {
      return 'peripheral';
    }
    return 'stream';
  };
  const kindRank: Record<LocalizationSourceGroup['kind'], number> = { stream: 1, profile: 2, peripheral: 3, peer: 4 };
  const mergeKind = (
    current: LocalizationSourceGroup['kind'],
    incoming: LocalizationSourceGroup['kind']
  ): LocalizationSourceGroup['kind'] => (kindRank[incoming] > kindRank[current] ? incoming : current);

  const parentStreamGroups = new Map<
    string,
    { key: string; label: string; path: string; kind: LocalizationSourceGroup['kind'] }
  >();
  for (const source of compatibleSources) {
    const streamId = (source.streamId ?? '').trim();
    if (!UUID_LIKE_RE.test(streamId)) continue;
    const key = cameraKeyForSource(source) ?? source.id;
    if (!key) continue;
    parentStreamGroups.set(streamId, {
      key,
      label: source.streamLabel || source.cameraUid || source.streamId || source.id,
      path: source.cameraPath || '',
      kind: sourceKind(source)
    });
  }

  const groups = new Map<
    string,
    {
      key: string;
      kind: LocalizationSourceGroup['kind'];
      label: string;
      path: string;
      pipelines: Map<string, { key: string; label: string; sources: LocalizationPipelineSource[] }>;
    }
  >();

  for (const source of compatibleSources) {
    const incomingKind = sourceKind(source);
    const parentStreamId = mediaImuParentStreamIdForSource(source);
    const parentGroup = parentStreamId ? parentStreamGroups.get(parentStreamId) : null;

    const kind: LocalizationSourceGroup['kind'] = parentGroup ? 'stream' : parentStreamId ? 'stream' : incomingKind;
    const cameraKey =
      parentGroup?.key ||
      parentStreamId ||
      ((kind === 'peer' || kind === 'profile')
        ? (source.streamId || source.id || source.cameraUid)
        : (source.cameraUid || source.streamId || source.id));
    const cameraLabel =
      parentGroup?.label ||
      (parentStreamId
        ? (source.streamLabel || '').replace(/\s+imu$/i, '').trim() || parentStreamId
        : source.streamLabel || source.cameraUid || source.streamId || source.id);
    const cameraPath = parentGroup?.path || source.cameraPath || '';
    const pipelineKey = source.pipelineId || source.pipelineLabel || 'pipeline';
    const pipelineLabel = source.pipelineLabel || source.pipelineId || 'Pipeline';

    const cameraGroup =
      groups.get(cameraKey) ??
      {
        key: cameraKey,
        kind,
        label: cameraLabel,
        path: cameraPath,
        pipelines: new Map()
      };
    cameraGroup.kind = mergeKind(cameraGroup.kind, kind);

    const pipelineGroup =
      cameraGroup.pipelines.get(pipelineKey) ??
      {
        key: pipelineKey,
        label: pipelineLabel,
        sources: []
      };

    pipelineGroup.sources.push(source);
    cameraGroup.pipelines.set(pipelineKey, pipelineGroup);
    groups.set(cameraKey, cameraGroup);
  }

  return Array.from(groups.values())
    .map((group) => ({
      ...group,
      pipelines: Array.from(group.pipelines.values())
        .map((pipeline) => ({
          ...pipeline,
          sources: [...pipeline.sources].sort((a, b) => a.outputKey.localeCompare(b.outputKey))
        }))
        .sort((a, b) => a.label.localeCompare(b.label))
    }))
    .sort((a, b) => a.label.localeCompare(b.label));
};

export const selectPrimarySource = (
  primarySourceId: string | null,
  sources: LocalizationPipelineSource[],
  selectedSources: LocalizationPipelineSource[]
): LocalizationPipelineSource | null => {
  if (primarySourceId) {
    const picked = sources.find((entry) => entry.id === primarySourceId) ?? null;
    if (picked) return picked;
  }
  return selectedSources[0] ?? null;
};

export const buildAnchorOptions = (liveMarkers: LocalizationMarker[]) =>
  liveMarkers
    .filter((marker) => (marker.targetType ?? 'aruco') === 'aruco-plane')
    .map((marker) => {
      const sourceLabel = marker.source?.streamLabel || marker.source?.streamId || '';
      return {
        id: marker.id,
        label: sourceLabel ? `${sourceLabel} · ${marker.label}` : marker.label
      };
    })
    .sort((a, b) => a.label.localeCompare(b.label));

export const computeDefaultAnchorId = (options: {
  tagAnchorEnabled: boolean;
  liveMarkers: LocalizationMarker[];
  primarySourceId: string | null;
}) => {
  if (!options.tagAnchorEnabled) return '';
  const candidates = options.liveMarkers.filter((marker) => (marker.targetType ?? 'aruco') === 'aruco-plane');
  if (candidates.length === 0) return '';

  const preferred = options.primarySourceId
    ? candidates.filter((marker) => marker.source?.id === options.primarySourceId)
    : candidates;
  const pool = preferred.length ? preferred : candidates;

  let best: { id: string; dist: number } | null = null;
  for (const marker of pool) {
    const [x, y, z] = marker.position;
    const dist = x * x + y * y + z * z;
    if (!best || dist < best.dist) {
      best = { id: marker.id, dist };
    }
  }
  return best?.id ?? '';
};

export const getViewerCameraIdForMarker = (options: {
  marker: LocalizationMarker;
  baseFrame: LocalizationBaseFrame;
  rigCameraForMarker: (marker: LocalizationMarker) => RigCameraInfo | null;
}): string | null => {
  if (options.baseFrame === 'robot') {
    return options.rigCameraForMarker(options.marker)?.uid ?? null;
  }
  const source = options.marker.source;
  return cameraKeyForSource(source ?? null) || source?.id || null;
};

export const buildSourceStatusRows = (options: {
  pollResults: LocalizationSourceSampleStatus[];
  sources: LocalizationPipelineSource[];
  streamMetricsById: Record<string, StreamMetrics>;
  streamMetricsUpdatedAtById: Record<string, number>;
  streamMetricsErrorById: Record<string, string>;
  detectionsBySourceId: Record<string, number>;
  toNumber: (value: string | number | null | undefined, fallback: number) => number;
}): SourceStatusRow[] =>
  options.pollResults
    .map((entry) => {
      const normalizedError = String(entry.error ?? '')
        .trim()
        .toLowerCase();
      const suppressSetupError =
        normalizedError === 'missing tag size for detections';
      const source = options.sources.find((candidate) => candidate.id === entry.sourceId) ?? null;
      if (!source) return null;
      const metrics = options.streamMetricsById[entry.streamId] ?? null;
      const graphMs = pipelineGraphTotalMs(metrics);
      const metricsUpdatedAt = options.streamMetricsUpdatedAtById[entry.streamId] ?? null;
      const metricsError = options.streamMetricsErrorById[entry.streamId] ?? null;
      const detections = options.detectionsBySourceId[entry.sourceId] ?? 0;
      const tagSize = entry.tagSize ? options.toNumber(entry.tagSize, NaN) : NaN;
      return {
        source,
        pollMs: entry.pollMs,
        detections,
        tagSize: Number.isFinite(tagSize) && tagSize > 0 ? tagSize : null,
        graphMs,
        metricsUpdatedAt,
        metricsError,
        error: suppressSetupError ? null : entry.error ?? null
      };
    })
    .filter((row): row is SourceStatusRow => Boolean(row));

export const buildViewerCameras = (options: {
  baseFrame: LocalizationBaseFrame;
  rigCameras: RigCameraInfo[];
  selectedSources: LocalizationPipelineSource[];
}): RigCameraInfo[] => {
  if (options.baseFrame === 'robot') {
    return options.rigCameras;
  }

  const isImuLikeSource = (source: LocalizationPipelineSource): boolean => {
    const streamId = String(source.streamId ?? '').trim().toLowerCase();
    const outputKey = String(source.outputKey ?? '').trim().toLowerCase();
    const cameraUid = String(source.cameraUid ?? '').trim().toLowerCase();
    return (
      hasDeviceImuExternalStreamPrefix(streamId) ||
      streamId.startsWith('external:media-imu-') ||
      outputKey.includes('imu') ||
      isDeviceImuCameraUid(cameraUid)
    );
  };

  const sourceScoreForGroup = (source: LocalizationPipelineSource, groupKey: string): number => {
    let score = 0;
    const normalizedGroupKey = groupKey.trim();
    const sourceCameraKey = cameraKeyForSource(source);
    const parentStreamId = mediaImuParentStreamIdForSource(source);
    const streamId = String(source.streamId ?? '').trim();
    const cameraUid = String(source.cameraUid ?? '').trim();
    if (sourceCameraKey && sourceCameraKey === normalizedGroupKey) score += 100;
    if (parentStreamId && parentStreamId === normalizedGroupKey) score += 80;
    if (cameraUid && cameraUid === normalizedGroupKey) score += 60;
    if (streamId && streamId === normalizedGroupKey) score += 40;
    if (!isImuLikeSource(source)) score += 50;
    if (streamId && !streamId.startsWith('external:')) score += 15;
    if (String(source.cameraPath ?? '').trim()) score += 8;
    if (String(source.streamLabel ?? '').trim()) score += 4;
    return score;
  };

  const canonicalStreamIdForGroup = (sources: LocalizationPipelineSource[]): string | null => {
    for (const source of sources) {
      const streamId = String(source.streamId ?? '').trim();
      if (streamId && !streamId.startsWith('external:')) return streamId;
    }
    for (const source of sources) {
      const parentStreamId = mediaImuParentStreamIdForSource(source);
      if (parentStreamId) return parentStreamId;
    }
    for (const source of sources) {
      const streamId = String(source.streamId ?? '').trim();
      if (streamId) return streamId;
    }
    return null;
  };

  const groups = groupLocalizationSources(options.selectedSources);
  return groups.map((group) => {
    const sources = group.pipelines.flatMap((pipeline) => pipeline.sources);
    const representative =
      [...sources].sort((left, right) => sourceScoreForGroup(right, group.key) - sourceScoreForGroup(left, group.key))[0] ??
      null;
    const streamId = canonicalStreamIdForGroup(sources);
    const nonImuCameraUid =
      sources
        .map((source) => String(source.cameraUid ?? '').trim())
        .find((cameraUid) => cameraUid.length > 0 && !isDeviceImuCameraUid(cameraUid)) ?? null;
    const cameraUid = nonImuCameraUid ?? (representative ? String(representative.cameraUid ?? '').trim() || null : null);
    const streamAlias =
      String(group.label ?? '').trim() ||
      (representative ? String(representative.streamLabel ?? '').trim() : '') ||
      group.key;
    const driverCameraId =
      String(group.path ?? '').trim() ||
      (representative ? String(representative.cameraPath ?? '').trim() : '') ||
      group.key;

    return {
      uid: group.key,
      streamId,
      cameraUid,
      streamAlias,
      driverCameraId,
      displayName: streamAlias || group.key,
      backend: 'Localization',
      pose: { translation: { x: 0, y: 0, z: 0 }, rotation: { roll: 0, pitch: 0, yaw: 0 } }
    };
  });
};

export const buildViewProfileOverlays = (options: {
  baseFrame: LocalizationBaseFrame;
  showRobotContext: boolean;
  coordinateSpace: string;
  viewProfiles: LocalizationProfile[];
  solveResponsesByProfile: Record<string, LocalizationSolveResponse>;
  profiles: LocalizationProfile[];
  profileIndexById: Map<string, number>;
  profileColors: readonly string[];
}): ViewProfileOverlay[] => {
  if (options.baseFrame !== 'field') return [];
  if (!options.showRobotContext && options.coordinateSpace !== 'robot_in_field') return [];
  if (options.viewProfiles.length === 0) return [];

  return options.viewProfiles.map((profile) => {
    const result = options.solveResponsesByProfile[profile.id] ?? null;
    const solverResult = result?.solvers?.[0] ?? null;
    const pose = solverResult?.outputs?.robotInField?.pose ?? null;
    return {
      id: profile.id,
      label: profile.name,
      color: profileColorForId(profile.id, options.profiles, options.profileIndexById, options.profileColors),
      transform: pose
        ? { position: [pose.translation.x, pose.translation.y, pose.translation.z], quaternion: pose.rotation.quaternion }
        : null
    };
  });
};

export const buildViewerCameraTransforms = (options: {
  baseFrame: LocalizationBaseFrame;
  solverCameraTransforms: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null;
  viewerCameras: RigCameraInfo[];
  separateCameras: boolean;
  primaryCameraKey?: string | null;
}): Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null => {
  if (options.baseFrame === 'robot') return options.solverCameraTransforms;
  if (options.baseFrame === 'field') return options.solverCameraTransforms;

  const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = {};
  const stride = 2.2;
  const quaternion = identityTransform().quaternion;
  const cameraKeys = options.viewerCameras.map((camera) => camera.uid).filter(Boolean);
  if (cameraKeys.length === 0) return out;
  let order = cameraKeys;
  const primary = options.primaryCameraKey;
  if (primary && cameraKeys.includes(primary)) {
    order = [primary, ...cameraKeys.filter((key) => key !== primary)];
  }
  order.forEach((key, index) => {
    const offset = options.separateCameras && order.length > 1 ? index * stride : 0;
    out[key] = { position: [offset, 0, 0], quaternion };
  });
  return out;
};

export const buildViewerRobotTransform = (options: {
  baseFrame: LocalizationBaseFrame;
  showRobotContext: boolean;
  coordinateSpace: string;
  viewProfileOverlays: ViewProfileOverlay[];
  solverRobotTransform: ViewerTransform;
}): ViewerTransform => {
  if (options.baseFrame !== 'field') return null;
  if (!options.showRobotContext && options.coordinateSpace !== 'robot_in_field') return null;
  if (options.viewProfileOverlays.length > 0) return null;
  return options.solverRobotTransform;
};

export const buildViewerShowRobot = (options: {
  baseFrame: LocalizationBaseFrame;
  showRobotContext: boolean;
  coordinateSpace: string;
}) => {
  if (options.baseFrame === 'camera') return false;
  if (options.baseFrame === 'robot') return options.showRobotContext;
  return options.showRobotContext || options.coordinateSpace === 'robot_in_field';
};

export const buildActiveFieldMapBitsStatus = (doc: FieldMapDocument | null) => {
  if (!doc) return null;
  const total = doc.markers.length;
  if (!total) return 'bits: 0/0';
  const withBits = doc.markers.filter((marker) => {
    const bits = marker.tagBits;
    return Boolean(bits && Array.isArray(bits.rows) && typeof bits.width === 'number' && bits.rows.length === bits.width);
  }).length;
  return `bits: ${withBits}/${total}`;
};

export const buildActiveCustomField = (options: {
  viewMode: string;
  selectedCustomField: CustomField | null;
  activeFieldMapDoc: FieldMapDocument | null;
}) => {
  const mapDoc = options.activeFieldMapDoc;
  const normalizeOverlay = (doc: FieldMapDocument) => {
    const raw = doc.overlay ?? null;
    if (!raw) return null;
    // Limelight .fmap pngBase64 exports are commonly rotated relative to our viewer plane.
    // Interpret `rotationDeg` as a UV rotation and default to 90deg when absent.
    const kind = doc.source?.kind ?? '';
    const isLimelight = kind === 'limelightFmap' || kind === 'limelight-fmap';
    if (isLimelight && (raw.rotationDeg == null || !Number.isFinite(raw.rotationDeg))) {
      return { ...raw, rotationDeg: 90 };
    }
    return raw;
  };

  // In field mode, reuse the active field map dimensions/overlay so the viewer can show the
  // Limelight fmap floor image when present.
  if (options.viewMode === 'frc-field') {
    if (mapDoc) {
      return { name: mapDoc.name, width: mapDoc.widthM, depth: mapDoc.depthM, overlay: normalizeOverlay(mapDoc) };
    }
    return null;
  }

  if (options.viewMode !== 'custom-field') return null;
  const field = options.selectedCustomField;
  if (!field) return null;
  if (mapDoc) {
    return { name: field.name, width: mapDoc.widthM, depth: mapDoc.depthM, overlay: normalizeOverlay(mapDoc) };
  }
  return { name: field.name, width: field.width, depth: field.depth };
};

export const buildFieldSpaceLabel = (options: {
  viewMode: string;
  activeFieldOrigin: PlanarFieldOrigin | null;
  selectedCustomField: CustomField | null;
}) => {
  if (options.viewMode === 'frc-field') {
    const originId = options.activeFieldOrigin?.id ?? 'field';
    return `frcfield.${originId}`;
  }
  if (options.viewMode === 'custom-field') {
    return options.selectedCustomField?.name ?? 'custom';
  }
  return 'field';
};

export const buildActiveFieldDimensions = (options: {
  viewMode: string;
  activeCustomField: { width: number; depth: number } | null;
}): { width: number; depth: number } | null => {
  if (options.viewMode === 'frc-field') {
    if (options.activeCustomField) {
      return { width: options.activeCustomField.width, depth: options.activeCustomField.depth };
    }
    // Keep in sync with `FRC_FIELD_DIMENSIONS`.
    return { width: 8.2296, depth: 16.4592 };
  }
  if (options.viewMode === 'custom-field') {
    const field = options.activeCustomField;
    if (field) {
      return { width: field.width, depth: field.depth };
    }
  }
  return null;
};

export const buildActiveFieldOrigin = (options: {
  viewMode: string;
  profileFieldOrigin: LocalizationFieldOriginConfig | null;
  selectedCustomField: CustomField | null;
  selectedCustomFieldOriginId: string | null;
  activeFieldDimensions: { width: number; depth: number } | null;
}): PlanarFieldOrigin | null => {
  const dims = options.activeFieldDimensions;
  if (!dims) return null;
  if (options.viewMode === 'frc-field') {
    const mode: LocalizationFieldOriginMode = options.profileFieldOrigin?.mode ?? 'blue';
    if (mode === 'custom') {
      const custom = options.profileFieldOrigin?.custom ?? null;
      if (custom) {
        return {
          id: 'custom',
          name: 'Custom origin',
          x: custom.x,
          z: custom.z,
          yawDeg: custom.yawDeg
        };
      }
      return frcOriginDefinition('blue', dims);
    }
    return frcOriginDefinition(mode, dims);
  }
  if (options.viewMode === 'custom-field') {
    const field = options.selectedCustomField;
    if (!field) return null;
    const originId = options.selectedCustomFieldOriginId ?? field.origins[0]?.id ?? null;
    if (!originId) return null;
    const origin = field.origins.find((entry) => entry.id === originId) ?? null;
    if (!origin) return null;
    return { id: origin.id, name: origin.name, x: origin.x, z: origin.z, yawDeg: origin.yawDeg };
  }
  return null;
};

export const buildFieldSceneTransform = (options: {
  baseFrame: LocalizationBaseFrame;
  activeFieldOrigin: PlanarFieldOrigin | null;
}): PoseTransform | null => {
  if (options.baseFrame !== 'field') return null;
  const origin = options.activeFieldOrigin;
  if (!origin) return null;
  return transformFromFieldCenter(origin);
};

export const buildOriginFromFieldCenterForEditor = (activeFieldOrigin: PlanarFieldOrigin | null): PoseTransform => {
  if (!activeFieldOrigin) return identityTransform();
  return transformFromFieldCenter(activeFieldOrigin);
};
