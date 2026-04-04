import * as THREE from 'three';
import type { RigCameraInfo } from '$lib/types/rig';
import type {
  ArucoMarker,
  LocalizationMarker,
  LocalizationViewerProps,
  PolygonMarker
} from '$lib/features/localization/viewers/localizationViewerTypes';
import {
  buildMarker,
  updateMarkerTransform
} from '$lib/features/localization/viewers/localizationViewerMarkers';
import { disposeObject } from '$lib/features/localization/viewers/localizationSceneLifecycle';

export type LocalizationViewerMarkerContext = {
  get arucoTextureCache(): Map<string, THREE.Texture>;
  get browser(): boolean;
  get cameraLayout(): RigCameraInfo[];
  get cameraTransforms(): LocalizationViewerProps['cameraTransforms'];
  get markerColors(): { tracking: string; idle: string; offline: string };
  get markers(): LocalizationMarker[];
  get markerGroup(): THREE.Group | undefined;
  get markerMeshes(): Map<string, THREE.Group>;
  get referenceMarkerGroup(): THREE.Group | undefined;
  get referenceMarkerMeshes(): Map<string, THREE.Group>;
  get referenceMarkers(): LocalizationMarker[];
  get showTagLines(): boolean;
  get tagLineGroup(): THREE.Group | undefined;
  get tagLineMarkers(): LocalizationMarker[];
  get tagLineMeshes(): Map<string, THREE.Line>;
};

export function updateMarkers(ctx: LocalizationViewerMarkerContext) {
  if (!ctx.markerGroup || !ctx.markers) return;

  const nextIds = new Set<string>();
  for (const marker of ctx.markers) {
    nextIds.add(marker.id);
  }

  for (const [id, mesh] of ctx.markerMeshes) {
    if (nextIds.has(id)) continue;
    ctx.markerGroup.remove(mesh);
    disposeObject(mesh);
    ctx.markerMeshes.delete(id);
  }

  for (const marker of ctx.markers) {
    const existing = ctx.markerMeshes.get(marker.id);
    const visualKey = markerVisualKey(marker);
    if (!existing) {
      const built = buildMarker(marker, {
        browser: ctx.browser,
        markerColors: ctx.markerColors,
        arucoTextureCache: ctx.arucoTextureCache
      });
      if (!built) continue;
      built.name = marker.id;
      built.userData.__visualKey = visualKey;
      ctx.markerGroup.add(built);
      ctx.markerMeshes.set(marker.id, built);
      continue;
    }
    const existingVisualKey = String(existing.userData?.__visualKey ?? '');
    if (existingVisualKey !== visualKey) {
      ctx.markerGroup.remove(existing);
      disposeObject(existing);
      const rebuilt = buildMarker(marker, {
        browser: ctx.browser,
        markerColors: ctx.markerColors,
        arucoTextureCache: ctx.arucoTextureCache
      });
      if (!rebuilt) {
        ctx.markerMeshes.delete(marker.id);
        continue;
      }
      rebuilt.name = marker.id;
      rebuilt.userData.__visualKey = visualKey;
      ctx.markerGroup.add(rebuilt);
      ctx.markerMeshes.set(marker.id, rebuilt);
      continue;
    }

    updateMarkerTransform(existing, marker);
  }
}

export function markerVisualKey(marker: LocalizationMarker): string {
  const arucoMarker: ArucoMarker | null =
    marker.targetType === 'polygon' ? null : marker;
  const common = [
    marker.targetType ?? 'aruco',
    marker.color ?? '',
    marker.status ?? '',
    String(arucoMarker?.tagId ?? ''),
    String(arucoMarker?.tagSize ?? ''),
    String(arucoMarker?.tagHeight ?? ''),
    String(arucoMarker?.tagBorderRatio ?? ''),
    String(arucoMarker?.codeRotation ?? '')
  ];

  const bits = arucoMarker?.tagBits;
  if (bits && Number.isFinite(bits.width) && Array.isArray(bits.rows)) {
    common.push(`bits:${bits.width}:${bits.border}:${bits.rows.join('')}`);
  } else {
    common.push('bits:');
  }

  if (marker.targetType === 'polygon') {
    const polygon: PolygonMarker = marker;
    const outline = Array.isArray(polygon.outline)
      ? polygon.outline
          .map(
            (pair: [number, number]) =>
              `${Number(pair?.[0] ?? 0).toFixed(4)},${Number(pair?.[1] ?? 0).toFixed(4)}`
          )
          .join(';')
      : '';
    common.push(`outline:${outline}`);
    common.push(`thickness:${String(polygon.thickness ?? '')}`);
    common.push(`outlineColor:${String(polygon.outlineColor ?? '')}`);
    common.push(`fillOpacity:${String(polygon.fillOpacity ?? '')}`);
  }

  return common.join('|');
}

export function updateReferenceMarkers(ctx: LocalizationViewerMarkerContext) {
  if (!ctx.referenceMarkerGroup || !ctx.referenceMarkers) return;

  const nextIds = new Set<string>();
  for (const marker of ctx.referenceMarkers) {
    nextIds.add(marker.id);
  }

  for (const [id, mesh] of ctx.referenceMarkerMeshes) {
    if (nextIds.has(id)) continue;
    ctx.referenceMarkerGroup.remove(mesh);
    disposeObject(mesh);
    ctx.referenceMarkerMeshes.delete(id);
  }

  for (const marker of ctx.referenceMarkers) {
    const existing = ctx.referenceMarkerMeshes.get(marker.id);
    const visualKey = markerVisualKey(marker);
    if (!existing) {
      const built = buildMarker(marker, {
        browser: ctx.browser,
        markerColors: ctx.markerColors,
        arucoTextureCache: ctx.arucoTextureCache
      });
      if (!built) continue;
      built.name = marker.id;
      built.userData.__visualKey = visualKey;
      enableMinimapLayer(built);
      ctx.referenceMarkerGroup.add(built);
      ctx.referenceMarkerMeshes.set(marker.id, built);
      continue;
    }
    const existingVisualKey = String(existing.userData?.__visualKey ?? '');
    if (existingVisualKey !== visualKey) {
      ctx.referenceMarkerGroup.remove(existing);
      disposeObject(existing);
      const rebuilt = buildMarker(marker, {
        browser: ctx.browser,
        markerColors: ctx.markerColors,
        arucoTextureCache: ctx.arucoTextureCache
      });
      if (!rebuilt) {
        ctx.referenceMarkerMeshes.delete(marker.id);
        continue;
      }
      rebuilt.name = marker.id;
      rebuilt.userData.__visualKey = visualKey;
      enableMinimapLayer(rebuilt);
      ctx.referenceMarkerGroup.add(rebuilt);
      ctx.referenceMarkerMeshes.set(marker.id, rebuilt);
      continue;
    }

    updateMarkerTransform(existing, marker);
  }
}

export function cameraKeyVariants(value: string | null | undefined): string[] {
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
}

export function cameraLayoutKeys(camera: RigCameraInfo): string[] {
  const out = new Set<string>();
  const add = (value: string | null | undefined) => {
    for (const key of cameraKeyVariants(value)) out.add(key);
  };
  add(camera.uid);
  add(camera.cameraUid ?? null);
  add(camera.streamId ?? null);
  add(camera.driverCameraId ?? null);
  add(camera.hardwareId ?? null);
  add(camera.streamAlias ?? null);
  return Array.from(out.values());
}

export function viewerPositionFromRigTranslation(
  translation: { x?: number; y?: number; z?: number } | null | undefined
): [number, number, number] | null {
  if (!translation) return null;
  const tx = translation.x;
  const ty = translation.y;
  const tz = translation.z;
  if (![tx, ty, tz].every((value) => typeof value === 'number' && Number.isFinite(value))) {
    return null;
  }
  return [ty, tz, tx];
}

export function isFinitePosition(value: unknown): value is [number, number, number] {
  return (
    Array.isArray(value) &&
    value.length === 3 &&
    Number.isFinite(value[0]) &&
    Number.isFinite(value[1]) &&
    Number.isFinite(value[2])
  );
}

export function resolveSourceCameraPosition(
  ctx: LocalizationViewerMarkerContext,
  marker: LocalizationMarker
): [number, number, number] | null {
  const source = marker.source;
  if (!source) return null;
  const sourceKeys = new Set<string>();
  const addSourceKey = (value: string | null | undefined) => {
    for (const key of cameraKeyVariants(value)) sourceKeys.add(key);
  };
  addSourceKey(source.cameraUid);
  addSourceKey(source.streamId);
  addSourceKey(source.cameraPath);
  addSourceKey(source.id);
  if (sourceKeys.size === 0) return null;

  for (const key of sourceKeys) {
    const transform = ctx.cameraTransforms?.[key];
    if (transform && isFinitePosition(transform.position)) {
      return transform.position;
    }
  }

  const matchedCamera = ctx.cameraLayout.find((camera) =>
    cameraLayoutKeys(camera).some((key) => sourceKeys.has(key))
  );
  if (matchedCamera) {
    for (const key of cameraLayoutKeys(matchedCamera)) {
      const transform = ctx.cameraTransforms?.[key];
      if (transform && isFinitePosition(transform.position)) {
        return transform.position;
      }
    }

    const fallback = viewerPositionFromRigTranslation(matchedCamera.pose?.translation);
    if (fallback) return fallback;
  }

  return null;
}

export function updateTagLines(ctx: LocalizationViewerMarkerContext) {
  if (!ctx.tagLineGroup) return;
  const activeMarkers = Array.isArray(ctx.tagLineMarkers)
    ? ctx.tagLineMarkers
    : ctx.markers;
  const lineMarkers = Array.isArray(activeMarkers) ? activeMarkers : [];
  const lineTargets: Array<{ id: string; marker: LocalizationMarker }> = lineMarkers.map(
    (marker) => ({
      id: marker.id,
      marker
    })
  );

  if (!ctx.showTagLines || lineTargets.length === 0) {
    for (const line of ctx.tagLineMeshes.values()) {
      ctx.tagLineGroup.remove(line);
      disposeObject(line);
    }
    ctx.tagLineMeshes.clear();
    ctx.tagLineGroup.visible = false;
    return;
  }

  const nextIds = new Set<string>();
  for (const target of lineTargets) {
    const marker = target.marker;
    const cameraPosition = resolveSourceCameraPosition(ctx, marker);
    if (!cameraPosition) continue;
    const lineId = target.id;
    nextIds.add(lineId);

    const points = [
      new THREE.Vector3(...cameraPosition),
      new THREE.Vector3(...marker.position)
    ];
    const color = marker.color ?? '#38bdf8';
    const existing = ctx.tagLineMeshes.get(lineId);
    if (!existing) {
      const geometry = new THREE.BufferGeometry().setFromPoints(points);
      const material = new THREE.LineBasicMaterial({
        color,
        transparent: true,
        opacity: 0.6
      });
      const line = new THREE.Line(geometry, material);
      line.renderOrder = 3;
      ctx.tagLineGroup.add(line);
      ctx.tagLineMeshes.set(lineId, line);
      continue;
    }

    const geometry = existing.geometry as THREE.BufferGeometry;
    geometry.setFromPoints(points);
    geometry.computeBoundingSphere();
    const material = existing.material as THREE.LineBasicMaterial;
    material.color.set(color);
    material.transparent = true;
    material.opacity = 0.6;
  }

  for (const [id, line] of ctx.tagLineMeshes.entries()) {
    if (nextIds.has(id)) continue;
    ctx.tagLineGroup.remove(line);
    disposeObject(line);
    ctx.tagLineMeshes.delete(id);
  }

  ctx.tagLineGroup.visible = true;
}

export function enableMinimapLayer(object: THREE.Object3D) {
  object.traverse((node) => {
    node.layers.enable(1);
  });
}
