import * as THREE from 'three';
import type {
  ArucoMarker,
  LocalizationMarker,
  PolygonMarker
} from './localizationViewerTypes';

type MarkerColors = {
  tracking: string;
  idle: string;
  offline: string;
};

type MarkerBuildDeps = {
  browser: boolean;
  markerColors: MarkerColors;
  arucoTextureCache: Map<string, THREE.Texture>;
};

export function updateMarkerTransform(group: THREE.Group, marker: LocalizationMarker): void {
  group.position.set(...marker.position);
  if (marker.quaternion) {
    group.quaternion.set(marker.quaternion.x, marker.quaternion.y, marker.quaternion.z, marker.quaternion.w);
    group.quaternion.normalize();
    return;
  }
  const yaw = THREE.MathUtils.degToRad(marker.heading ?? 0);
  group.rotation.set(0, yaw, 0);
}

export function buildMarker(marker: LocalizationMarker, deps: MarkerBuildDeps): THREE.Group | null {
  const group = new THREE.Group();
  group.userData.markerType = marker.targetType ?? 'aruco';
  group.userData.tagId = marker && 'tagId' in marker ? marker.tagId ?? null : null;
  group.userData.tagSize = marker && 'tagSize' in marker ? marker.tagSize ?? null : null;
  updateMarkerTransform(group, marker);

  const targetType = marker.targetType ?? 'aruco';
  const baseColor =
    marker.color ??
    deps.markerColors[(marker.status as keyof MarkerColors) ?? 'tracking'] ??
    '#a855f7';

  if (targetType === 'polygon') {
    const polygon = createPolygonTarget(marker as PolygonMarker, baseColor);
    if (polygon) {
      group.add(polygon);
    }
  } else if (targetType === 'aruco-plane') {
    const tag = createArucoTagPlane(marker as ArucoMarker, baseColor, deps);
    group.add(tag);
  } else {
    const arucoTag = createArucoTagTarget(marker as ArucoMarker, baseColor, deps);
    group.add(arucoTag);
  }

  const arrow = createHeadingArrow(marker);
  if (arrow) {
    group.add(arrow);
  }

  return group;
}

function addMeshOutline(mesh: THREE.Mesh, color: string): void {
  const geometry = new THREE.EdgesGeometry(mesh.geometry);
  const material = new THREE.LineBasicMaterial({ color });
  const outline = new THREE.LineSegments(geometry, material);
  outline.renderOrder = 1;
  mesh.add(outline);
}

function createArucoTagTarget(marker: ArucoMarker, baseColor: string, deps: MarkerBuildDeps): THREE.Group {
  const tagSize = marker.tagSize ?? 0.6;
  const tagHeight = marker.tagHeight ?? 0.9;
  const pedestalRadius = Math.max(tagSize * 0.2, 0.15);

  const tagGroup = new THREE.Group();

  const base = new THREE.Mesh(
    new THREE.CylinderGeometry(pedestalRadius * 1.1, pedestalRadius * 1.25, 0.08, 24),
    new THREE.MeshStandardMaterial({ color: '#0f172a', roughness: 0.7, metalness: 0.2 })
  );
  base.receiveShadow = true;
  tagGroup.add(base);

  const column = new THREE.Mesh(
    new THREE.CylinderGeometry(pedestalRadius * 0.55, pedestalRadius * 0.65, tagHeight, 28),
    new THREE.MeshStandardMaterial({ color: baseColor, roughness: 0.35, metalness: 0.45 })
  );
  column.castShadow = true;
  column.position.y = tagHeight / 2 + 0.04;
  tagGroup.add(column);

  // Use an unlit material so tag bits remain readable regardless of scene lighting.
  const boardMaterial = new THREE.MeshBasicMaterial({ color: 0xffffff, side: THREE.FrontSide });

  const texture = ensureArucoTexture(marker, deps);
  if (texture) {
    boardMaterial.map = texture;
    boardMaterial.needsUpdate = true;
  }

  const board = new THREE.Mesh(new THREE.PlaneGeometry(tagSize, tagSize), boardMaterial);
  // Tag planes are rendered in an XY plane (normal +Z). Our marker quaternions are expected
  // to already encode the correct tag frame orientation, so avoid additional pre-rotation here.
  board.position.y = tagHeight + tagSize * 0.5 + 0.04;
  board.castShadow = true;
  addMeshOutline(board, baseColor);
  tagGroup.add(board);

  return tagGroup;
}

function createArucoTagPlane(marker: ArucoMarker, baseColor: string, deps: MarkerBuildDeps): THREE.Group {
  const tagSize = marker.tagSize ?? 0.6;

  // Use an unlit material so tag bits remain readable regardless of scene lighting.
  const boardMaterial = new THREE.MeshBasicMaterial({ color: 0xffffff, side: THREE.FrontSide });

  const texture = ensureArucoTexture(marker, deps);
  if (texture) {
    boardMaterial.map = texture;
    boardMaterial.needsUpdate = true;
  }

  const board = new THREE.Mesh(new THREE.PlaneGeometry(tagSize, tagSize), boardMaterial);
  // See createArucoTagTarget: avoid extra basis twiddling; the quaternion should be authoritative.
  board.castShadow = true;
  addMeshOutline(board, baseColor);

  const group = new THREE.Group();
  group.add(board);
  return group;
}

function createPolygonTarget(marker: PolygonMarker, baseColor: string): THREE.Group | null {
  if (!marker.outline || marker.outline.length < 3) {
    return null;
  }

  const thickness = Math.max(marker.thickness ?? 0.12, 0.01);
  const shape = new THREE.Shape();
  marker.outline.forEach(([x, z], index) => {
    if (index === 0) {
      shape.moveTo(x, z);
    } else {
      shape.lineTo(x, z);
    }
  });
  shape.closePath();

  const extrude = new THREE.ExtrudeGeometry(shape, { depth: thickness, bevelEnabled: false });
  extrude.rotateX(-Math.PI / 2);
  extrude.translate(0, thickness / 2, 0);

  const fillMaterial = new THREE.MeshStandardMaterial({
    color: baseColor,
    transparent: true,
    opacity: marker.fillOpacity ?? 0.45,
    roughness: 0.55,
    metalness: 0.1,
    side: THREE.DoubleSide
  });

  const mesh = new THREE.Mesh(extrude, fillMaterial);
  mesh.receiveShadow = true;

  const outlineGeometry = new THREE.BufferGeometry().setFromPoints(
    marker.outline.map(([x, z]) => new THREE.Vector3(x, thickness + 0.005, z))
  );
  const outlineMaterial = new THREE.LineBasicMaterial({
    color: marker.outlineColor ?? '#f8fafc',
    linewidth: 2
  });
  const loop = new THREE.LineLoop(outlineGeometry, outlineMaterial);

  const polygonGroup = new THREE.Group();
  polygonGroup.add(mesh);
  polygonGroup.add(loop);

  return polygonGroup;
}

function createHeadingArrow(marker: LocalizationMarker): THREE.ArrowHelper | null {
  if (typeof marker.heading !== 'number') return null;
  const targetType = marker.targetType ?? 'aruco';
  if (targetType === 'aruco-plane') {
    return null;
  }
  const baseHeight =
    targetType === 'polygon'
      ? Math.max((marker as PolygonMarker).thickness ?? 0.12, 0.01) + 0.05
      : ((marker as ArucoMarker).tagHeight ?? 0.9) + ((marker as ArucoMarker).tagSize ?? 0.6) * 0.5;

  const direction = new THREE.Vector3(0, 0, 1);
  const origin = new THREE.Vector3(0, baseHeight, 0);
  const arrowLength = targetType === 'polygon' ? 1 : 1.1;
  const arrow = new THREE.ArrowHelper(direction.normalize(), origin, arrowLength, 0xffffff);
  return arrow;
}

function ensureArucoTexture(marker: ArucoMarker, deps: MarkerBuildDeps): THREE.Texture | null {
  if (!deps.browser) return null;
  const tagId = marker.tagId ?? marker.id;
  const bits = marker.tagBits;
  const bitsKey = bits && bits.rows.length === bits.width ? `${bits.width}:${bits.rows.join('')}` : 'missing-bits';
  // Keyed only by bits; `codeRotation` should not affect the rendered reference texture.
  const cacheKey = `v3:${tagId}-${marker.tagSize ?? 0.6}-${bitsKey}`;
  const cached = deps.arucoTextureCache.get(cacheKey);
  if (cached) return cached;

  const baseCanvas = document.createElement('canvas');
  // Higher resolution helps keep data bits visible when the tag is small in the scene.
  baseCanvas.width = 1024;
  baseCanvas.height = 1024;
  const context = baseCanvas.getContext('2d');
  if (!context) return null;

  context.fillStyle = '#ffffff';
  context.fillRect(0, 0, baseCanvas.width, baseCanvas.height);

  const gridWidth = bits?.width ?? 0;
  const rows = bits?.rows ?? [];
  const safeWidth = Number.isFinite(gridWidth) && gridWidth > 0 && rows.length === gridWidth ? gridWidth : 6;

  const margin = Math.round(baseCanvas.width * 0.08);
  const gridSize = baseCanvas.width - margin * 2;
  const cellSize = gridSize / safeWidth;
  const start = margin;

  // Always draw a strong border + id label so it's obvious the texture is applied,
  // even if the server didn't provide tag bits yet.
  context.save();
  context.strokeStyle = '#0f172a';
  context.lineWidth = 12;
  context.strokeRect(start, start, gridSize, gridSize);
  context.restore();

  if (safeWidth !== gridWidth || rows.length !== gridWidth) {
    // Fallback placeholder pattern (checkerboard) to prove texture mapping works.
    context.fillStyle = '#0f172a';
    for (let y = 0; y < safeWidth; y += 1) {
      for (let x = 0; x < safeWidth; x += 1) {
        if ((x + y) % 2 === 0) {
          context.fillRect(start + x * cellSize, start + y * cellSize, cellSize, cellSize);
        }
      }
    }
  } else {
    context.fillStyle = '#000000';
    for (let y = 0; y < safeWidth; y += 1) {
      const row = rows[y] ?? '';
      for (let x = 0; x < safeWidth; x += 1) {
        const bit = row.charCodeAt(x) === 49 ? 1 : 0;
        if (bit === 0) {
          context.fillRect(start + x * cellSize, start + y * cellSize, cellSize, cellSize);
        }
      }
    }
    context.strokeStyle = '#0f172a';
    context.lineWidth = 2;
    context.strokeRect(start, start, gridSize, gridSize);
  }

  const idLabel =
    typeof marker.tagId === 'number' || typeof marker.tagId === 'string'
      ? String(marker.tagId)
      : typeof tagId === 'string'
        ? tagId
        : '';
  if (idLabel) {
    context.save();
    const fontSize = Math.max(64, Math.round(baseCanvas.width * 0.1));
    context.font = `900 ${fontSize}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace`;
    context.textBaseline = 'alphabetic';
    context.textAlign = 'left';
    const padding = Math.round(baseCanvas.width * 0.03);
    const textWidth = context.measureText(idLabel).width;
    const boxWidth = Math.min(baseCanvas.width - padding * 2, Math.ceil(textWidth + padding * 2));
    const boxHeight = Math.ceil(fontSize + padding * 1.6);
    const x = padding;
    const y = baseCanvas.height - padding;
    context.fillStyle = 'rgba(2, 6, 23, 0.9)';
    context.fillRect(x, y - boxHeight, boxWidth, boxHeight);
    context.fillStyle = '#f8fafc';
    context.fillText(idLabel, x + padding, y - padding);
    context.restore();
  }

  context.fillStyle = '#38bdf8';
  context.fillRect(10, 10, 36, 36);

  const texture = new THREE.CanvasTexture(baseCanvas);
  texture.needsUpdate = true;
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.generateMipmaps = false;
  texture.minFilter = THREE.LinearFilter;
  texture.magFilter = THREE.NearestFilter;
  texture.anisotropy = 8;
  deps.arucoTextureCache.set(cacheKey, texture);
  return texture;
}
