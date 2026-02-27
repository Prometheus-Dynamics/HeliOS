import * as THREE from 'three';
import { applyPose, createCameraGroup, createRobotBaseGroup, setCameraHighlight } from '$lib/3d/rig';
import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
import type {
  LocalizationFieldDefinition,
  LocalizationViewMode,
  RobotOverlay
} from './localizationViewerTypes';
import {
  BUMPER_THICKNESS_M,
  DEFAULT_BUMPER_COLOR,
  DEFAULT_CUSTOM_FIELD,
  DEFAULT_ROBOT_LENGTH_M,
  DEFAULT_ROBOT_WIDTH_M,
  FRC_FIELD_DIMENSIONS,
  GROUND_CLEARANCE_M,
  ROBOT_HEIGHT_M
} from './localizationViewerTypes';

const FIELD_OVERLAY_TEXTURE_CACHE = new Map<string, THREE.Texture>();

export const resolveColorHex = (value: string | null | undefined, fallback: number): number => {
  if (!value) return fallback;
  try {
    return new THREE.Color(value).getHex();
  } catch {
    return fallback;
  }
};

const normalizeLookupKey = (value: string | null | undefined): string | null => {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
};

const lookupKeyVariants = (value: string | null | undefined): string[] => {
  const normalized = normalizeLookupKey(value);
  if (!normalized) return [];
  const out = new Set<string>([normalized]);
  const stripped = normalized.startsWith('device:')
    ? normalized.slice('device:'.length)
    : normalized.startsWith('stream:')
      ? normalized.slice('stream:'.length)
      : normalized;
  if (stripped) {
    out.add(stripped);
    out.add(`device:${stripped}`);
    out.add(`stream:${stripped}`);
  }
  return Array.from(out.values());
};

const cameraLookupKeys = (camera: RigCameraInfo): string[] => {
  const out = new Set<string>();
  for (const value of [
    camera.uid,
    camera.cameraUid ?? null,
    camera.streamId ?? null,
    camera.driverCameraId ?? null,
    camera.hardwareId ?? null,
    camera.streamAlias ?? null
  ]) {
    for (const key of lookupKeyVariants(value)) {
      out.add(key);
    }
  }
  return Array.from(out.values());
};

const resolveCameraTransform = (
  cameraTransforms: Record<
    string,
    { position: [number, number, number]; quaternion?: { x: number; y: number; z: number; w: number } }
  > | null,
  camera: RigCameraInfo
) => {
  if (!cameraTransforms) return null;
  for (const key of cameraLookupKeys(camera)) {
    const match = cameraTransforms[key];
    if (match) return match;
  }
  return null;
};

const recenterCameraGeometry = (group: THREE.Group) => {
  const target =
    group.getObjectByName('camera-model') ??
    group.getObjectByName('camera-placeholder') ??
    group;
  group.updateWorldMatrix(true, true);
  const bounds = new THREE.Box3().setFromObject(target);
  if (bounds.isEmpty()) return;
  const centerWorld = bounds.getCenter(new THREE.Vector3());
  const centerLocal = group.worldToLocal(centerWorld.clone());
  if (![centerLocal.x, centerLocal.y, centerLocal.z].every(Number.isFinite)) return;
  for (const child of group.children) {
    child.position.sub(centerLocal);
  }
  group.updateWorldMatrix(true, true);
};

const scheduleCameraRecentering = (group: THREE.Group) => {
  const ready = group.userData?.modelReady as Promise<void> | undefined;
  if (!ready) {
    recenterCameraGeometry(group);
    return;
  }
  void ready
    .then(() => {
      recenterCameraGeometry(group);
    })
    .catch(() => {
      recenterCameraGeometry(group);
    });
};

const setCameraGhostVisual = (group: THREE.Group, ghost: boolean) => {
  group.traverse((node) => {
    const mesh = node as THREE.Mesh;
    if (!mesh?.isMesh) return;
    const materials = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
    for (const material of materials) {
      if (!material) continue;
      const mat = material as THREE.Material & {
        opacity?: number;
        transparent?: boolean;
        depthWrite?: boolean;
        userData?: Record<string, unknown>;
      };
      const userData = (mat.userData ??= {});
      const baseOpacity =
        typeof userData.__baseOpacity === 'number'
          ? (userData.__baseOpacity as number)
          : typeof mat.opacity === 'number'
            ? mat.opacity
            : 1;
      userData.__baseOpacity = baseOpacity;
      if (typeof mat.opacity === 'number') {
        mat.opacity = ghost ? Math.max(0.25, baseOpacity * 0.62) : baseOpacity;
      }
      mat.transparent = ghost || Boolean(mat.transparent);
      mat.depthWrite = !ghost;
      mat.needsUpdate = true;
    }
  });
};

export const createBumperMaterial = (color?: string | null): THREE.MeshStandardMaterial => {
  const baseColor = new THREE.Color(resolveColorHex(color ?? null, DEFAULT_BUMPER_COLOR));
  const emissive = baseColor.clone().multiplyScalar(0.25);
  return new THREE.MeshStandardMaterial({
    color: baseColor,
    emissive,
    emissiveIntensity: 0.2,
    roughness: 0.55,
    metalness: 0.2
  });
};

export const toNumber = (value: unknown, fallback: number): number => {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return fallback;
};

export const normalizeRobot = (value: unknown): RobotDimensions => {
  if (value && typeof value === 'object') {
    const input = value as Partial<RobotDimensions>;
    return {
      width: toNumber(input.width, DEFAULT_ROBOT_WIDTH_M),
      length: toNumber(input.length, DEFAULT_ROBOT_LENGTH_M),
      bumperHeight: toNumber(input.bumperHeight, ROBOT_HEIGHT_M),
      bumperThickness: toNumber(input.bumperThickness, BUMPER_THICKNESS_M),
      groundClearance: toNumber(input.groundClearance, GROUND_CLEARANCE_M)
    };
  }
  return {
    width: DEFAULT_ROBOT_WIDTH_M,
    length: DEFAULT_ROBOT_LENGTH_M,
    bumperHeight: ROBOT_HEIGHT_M,
    bumperThickness: BUMPER_THICKNESS_M,
    groundClearance: GROUND_CLEARANCE_M
  };
};

export const normalizeCameras = (value: unknown): RigCameraInfo[] => {
  if (!Array.isArray(value)) {
    return [];
  }
  return value
    .filter((entry): entry is RigCameraInfo => !!entry && typeof entry === 'object')
    .map((entry) => ({
      ...entry,
      pose: entry.pose
        ? {
            ...entry.pose,
            translation: entry.pose.translation ? { ...entry.pose.translation } : entry.pose.translation,
            rotation: entry.pose.rotation ? { ...entry.pose.rotation } : entry.pose.rotation
          }
        : entry.pose
    }));
};

export const camerasEqual = (a: RigCameraInfo[], b: RigCameraInfo[]): boolean => {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const left = a[i];
    const right = b[i];
    if (left.uid !== right.uid) return false;
    const lp = left.pose;
    const rp = right.pose;
    if (!lp && !rp) continue;
    if (!lp || !rp) return false;
    if (
      Math.abs((lp.translation?.x ?? 0) - (rp.translation?.x ?? 0)) > 1e-6 ||
      Math.abs((lp.translation?.y ?? 0) - (rp.translation?.y ?? 0)) > 1e-6 ||
      Math.abs((lp.translation?.z ?? 0) - (rp.translation?.z ?? 0)) > 1e-6 ||
      Math.abs((lp.rotation?.roll ?? 0) - (rp.rotation?.roll ?? 0)) > 1e-6 ||
      Math.abs((lp.rotation?.pitch ?? 0) - (rp.rotation?.pitch ?? 0)) > 1e-6 ||
      Math.abs((lp.rotation?.yaw ?? 0) - (rp.rotation?.yaw ?? 0)) > 1e-6
    ) {
      return false;
    }
  }
  return true;
};

export const dimensionsEqual = (a: RobotDimensions, b: RobotDimensions) =>
  Math.abs(a.width - b.width) < 1e-6 &&
  Math.abs(a.length - b.length) < 1e-6 &&
  Math.abs(a.bumperHeight - b.bumperHeight) < 1e-6 &&
  Math.abs(a.bumperThickness - b.bumperThickness) < 1e-6 &&
  Math.abs(a.groundClearance - b.groundClearance) < 1e-6;

export const buildEnvironment = (params: {
  environmentGroup: THREE.Group | null;
  viewMode: LocalizationViewMode;
  customField: LocalizationFieldDefinition | null;
  showFieldImage?: boolean;
}) => {
  const { environmentGroup, viewMode, customField, showFieldImage = true } = params;
  if (!environmentGroup) return;
  environmentGroup.children.forEach((child) => disposeObject(child));
  environmentGroup.clear();

  if (viewMode === 'frc-field') {
    const width = customField?.width && customField.width > 0 ? customField.width : FRC_FIELD_DIMENSIONS.width;
    const depth = customField?.depth && customField.depth > 0 ? customField.depth : FRC_FIELD_DIMENSIONS.depth;
    addFieldEnvironment(environmentGroup, {
      name: 'FRC field',
      width,
      depth,
      overlay: customField?.overlay ?? null
    }, showFieldImage);
    return;
  }
  if (viewMode === 'custom-field') {
    addFieldEnvironment(
      environmentGroup,
      customField && customField.width > 0 && customField.depth > 0
        ? customField
        : { ...DEFAULT_CUSTOM_FIELD, overlay: null },
      showFieldImage
    );
    return;
  }
  addIsolatedEnvironment(environmentGroup);
};

const addIsolatedEnvironment = (environmentGroup: THREE.Group) => {
  const floor = new THREE.Mesh(
    new THREE.CircleGeometry(38, 60),
    new THREE.MeshStandardMaterial({
      color: 0x0b101c,
      roughness: 0.85,
      metalness: 0.05,
      transparent: true,
      opacity: 0.16,
      depthWrite: false
    })
  );
  floor.rotation.x = -Math.PI / 2;
  floor.receiveShadow = true;
  environmentGroup.add(floor);

  const grid = new THREE.GridHelper(60, 30, 0x1c2c3f, 0x0d1928);
  tintGrid(grid, 0.35);
  grid.position.y = 0.01;
  environmentGroup.add(grid);
};

const addFieldEnvironment = (
  environmentGroup: THREE.Group,
  field: LocalizationFieldDefinition,
  showFieldImage: boolean
) => {
  const { width, depth, overlay } = field;

  const floor = new THREE.Mesh(
    new THREE.PlaneGeometry(width, depth),
    new THREE.MeshStandardMaterial({
      color: 0x07101a,
      roughness: 0.9,
      metalness: 0.08,
      transparent: true,
      opacity: 0.16,
      depthWrite: false
    })
  );
  floor.rotation.x = -Math.PI / 2;
  floor.receiveShadow = true;
  environmentGroup.add(floor);

  if (showFieldImage && overlay?.dataUrl) {
    const overlayWidth = overlay.widthM && overlay.widthM > 0 ? overlay.widthM : width;
    const overlayDepth = overlay.depthM && overlay.depthM > 0 ? overlay.depthM : depth;
    const overlayRotationDeg = typeof overlay.rotationDeg === 'number' && Number.isFinite(overlay.rotationDeg) ? overlay.rotationDeg : 0;
    const overlayCacheKey = `${overlay.dataUrl}|rot=${overlayRotationDeg}`;
    const overlayMaterial = new THREE.MeshBasicMaterial({
      color: 0xffffff,
      transparent: true,
      opacity: typeof overlay.opacity === 'number' ? overlay.opacity : 0.7,
      depthWrite: false,
      // Keep depth testing enabled so the overlay does not "wash out" objects above the floor.
      // Use polygon offset + a small Y lift on the mesh to reduce z-fighting with the floor.
      depthTest: true,
      polygonOffset: true,
      polygonOffsetFactor: -1,
      polygonOffsetUnits: -1
    });
    const overlayMesh = new THREE.Mesh(new THREE.PlaneGeometry(overlayWidth, overlayDepth), overlayMaterial);
    overlayMesh.rotation.x = -Math.PI / 2;
    overlayMesh.position.set(overlay.offsetXM ?? 0, 0.012, overlay.offsetZM ?? 0);
    overlayMesh.renderOrder = 10;

    const cached = FIELD_OVERLAY_TEXTURE_CACHE.get(overlayCacheKey) ?? null;
    if (cached) {
      overlayMaterial.map = cached;
      overlayMaterial.needsUpdate = true;
      overlayMesh.userData.texture = cached;
    } else {
      const loader = new THREE.TextureLoader();
      loader.load(overlay.dataUrl, (texture) => {
        texture.colorSpace = THREE.SRGBColorSpace;
        texture.wrapS = THREE.ClampToEdgeWrapping;
        texture.wrapT = THREE.ClampToEdgeWrapping;
        // Rotate the image in UV space; this fixes the common Limelight fmap orientation mismatch
        // without rotating the actual world-space plane.
        texture.center.set(0.5, 0.5);
        // Treat `rotationDeg` as clockwise degrees (matches typical UI expectations).
        texture.rotation = -THREE.MathUtils.degToRad(overlayRotationDeg);
        FIELD_OVERLAY_TEXTURE_CACHE.set(overlayCacheKey, texture);
        overlayMaterial.map = texture;
        overlayMaterial.needsUpdate = true;
        overlayMesh.userData.texture = texture;
      });
    }

    environmentGroup.add(overlayMesh);
  }

  const border = new THREE.LineSegments(
    new THREE.EdgesGeometry(new THREE.BoxGeometry(width, 0.2, depth)),
    new THREE.LineBasicMaterial({ color: 0x38bdf8 })
  );
  border.position.y = 0.12;
  environmentGroup.add(border);

  const grid = new THREE.GridHelper(width, width, 0x124364, 0x0a1c2b);
  grid.scale.z = depth / width;
  grid.position.y = 0.02;
  tintGrid(grid, 0.45);
  environmentGroup.add(grid);

  const beaconMaterial = new THREE.MeshStandardMaterial({
    color: 0x38bdf8,
    emissive: 0x082032,
    emissiveIntensity: 0.6,
    metalness: 0.4
  });
  const beaconGeometry = new THREE.CylinderGeometry(0.08, 0.08, 1.4, 12);
  const halfWidth = width / 2;
  const halfDepth = depth / 2;
  const posts = [
    new THREE.Vector3(halfWidth, 0.7, halfDepth),
    new THREE.Vector3(halfWidth, 0.7, -halfDepth),
    new THREE.Vector3(-halfWidth, 0.7, halfDepth),
    new THREE.Vector3(-halfWidth, 0.7, -halfDepth)
  ];
  posts.forEach((position) => {
    const pillar = new THREE.Mesh(beaconGeometry, beaconMaterial);
    pillar.position.copy(position);
    environmentGroup.add(pillar);
  });
};

export const tintGrid = (grid: THREE.GridHelper, opacity: number) => {
  const material = grid.material as THREE.Material & { opacity?: number; transparent?: boolean };
  material.opacity = opacity;
  material.transparent = true;
};

export const createAxesHelper = (size: number) => {
  const axes = new THREE.AxesHelper(size);
  const axesMaterial = axes.material as THREE.LineBasicMaterial;
  axesMaterial.depthTest = false;
  axesMaterial.transparent = true;
  axesMaterial.opacity = 0.4;
  axes.renderOrder = 2;
  return axes;
};

export const buildRobotBumpers = (params: {
  browser: boolean;
  robotGroup: THREE.Group | null;
  robotDimensions: RobotDimensions;
  bumperColor: string | null;
  showRobot: boolean;
}) => {
  const { browser, robotGroup, robotDimensions, bumperColor, showRobot } = params;
  if (!browser || !robotGroup) return;
  if (!showRobot) {
    robotGroup.clear();
    return;
  }
  robotGroup.clear();
  const base = createRobotBaseGroup(robotDimensions, { bumperMaterial: createBumperMaterial(bumperColor) });
  robotGroup.add(base);
};

export const syncRobotOverlays = (params: {
  browser: boolean;
  robotOverlayGroup: THREE.Group | null;
  robotOverlayMeshes: Map<string, THREE.Group>;
  robotOverlays: RobotOverlay[];
  showRobot: boolean;
  robotDimensions: RobotDimensions;
}) => {
  const { browser, robotOverlayGroup, robotOverlayMeshes, robotOverlays, showRobot, robotDimensions } = params;
  if (!browser || !robotOverlayGroup) return;
  const overlays = Array.isArray(robotOverlays) ? robotOverlays : [];
  const visible = showRobot && overlays.length > 0;
  robotOverlayGroup.visible = visible;
  const dimsKey = `${robotDimensions.width}:${robotDimensions.length}:${robotDimensions.bumperHeight}:${robotDimensions.bumperThickness}:${robotDimensions.groundClearance}`;
  const activeIds = new Set<string>();

  for (const overlay of overlays) {
    if (!overlay || !overlay.id) continue;
    activeIds.add(overlay.id);
    const color = overlay.color ?? null;
    let group = robotOverlayMeshes.get(overlay.id);
    const needsRebuild =
      !group ||
      group.userData.color !== color ||
      group.userData.dimsKey !== dimsKey;
    if (needsRebuild) {
      if (group) {
        disposeObject(group);
        robotOverlayGroup.remove(group);
      }
      group = new THREE.Group();
      group.userData.color = color;
      group.userData.dimsKey = dimsKey;
      const base = createRobotBaseGroup(robotDimensions, { bumperMaterial: createBumperMaterial(color) });
      group.add(base);
      robotOverlayGroup.add(group);
      robotOverlayMeshes.set(overlay.id, group);
    }

    const transform = overlay.transform;
    if (!transform) {
      group.visible = false;
      continue;
    }
    group.visible = true;
    group.position.set(...transform.position);
    if (transform.quaternion) {
      group.quaternion.set(
        transform.quaternion.x,
        transform.quaternion.y,
        transform.quaternion.z,
        transform.quaternion.w
      );
      group.quaternion.normalize();
    } else {
      group.quaternion.identity();
    }
  }

  for (const [id, group] of robotOverlayMeshes.entries()) {
    if (activeIds.has(id)) continue;
    disposeObject(group);
    robotOverlayGroup.remove(group);
    robotOverlayMeshes.delete(id);
  }
};

export const rebuildCameraRig = (params: {
  browser: boolean;
  cameraRigGroup: THREE.Group | null;
  cameraMeshes: Map<string, THREE.Group>;
  cameraLayout: RigCameraInfo[];
  showCameras: boolean;
  cameraGhostActive?: boolean;
  cameraTransforms: Record<string, { position: [number, number, number]; quaternion?: { x: number; y: number; z: number; w: number } }> | null;
  cameraHighlightColor: string | null;
}) => {
  const {
    browser,
    cameraRigGroup,
    cameraMeshes,
    cameraLayout,
    showCameras,
    cameraGhostActive,
    cameraTransforms,
    cameraHighlightColor
  } = params;
  if (!browser || !cameraRigGroup) return;
  cameraRigGroup.clear();
  for (const mesh of cameraMeshes.values()) {
    disposeObject(mesh);
  }
  cameraMeshes.clear();
  if (!showCameras) {
    return;
  }

  const highlight = resolveColorHex(cameraHighlightColor ?? null, 0xf8fafc);
  for (const entry of cameraLayout) {
    if (!entry.pose) continue;
    const mesh = createCameraGroup({
      id: entry.uid,
      bodyColor: 0x94a3b8,
      highlightColor: highlight,
      forwardDotColor: 0x38bdf8,
      includeAxisGizmo: true,
      axisGizmoOpacity: 0.55
    });
    scheduleCameraRecentering(mesh);
    setCameraHighlight(mesh, false);
    setCameraGhostVisual(mesh, Boolean(cameraGhostActive));
    const override = resolveCameraTransform(cameraTransforms, entry);
    if (override) {
      mesh.position.set(...override.position);
      if (override.quaternion) {
        mesh.quaternion.set(
          override.quaternion.x,
          override.quaternion.y,
          override.quaternion.z,
          override.quaternion.w
        );
        mesh.quaternion.normalize();
      } else {
        mesh.quaternion.identity();
      }
    } else {
      applyPose(mesh, entry.pose);
    }
    cameraRigGroup.add(mesh);
    cameraMeshes.set(entry.uid, mesh);
  }
};

export const disposeObject = (object: THREE.Object3D | null | undefined) => {
  if (!object) return;
  object.traverse((node) => {
    const anyNode = node as unknown as {
      geometry?: { dispose?: () => void };
      material?: THREE.Material | THREE.Material[];
      userData?: Record<string, unknown>;
    };

    anyNode.geometry?.dispose?.();
    const material = anyNode.material;
    if (Array.isArray(material)) {
      material.forEach((mat) => mat.dispose?.());
    } else {
      material?.dispose?.();
    }

    const texture = (anyNode.userData as { texture?: THREE.Texture } | undefined)?.texture;
    texture?.dispose?.();
  });
};

export const getActiveField = (
  viewMode: LocalizationViewMode,
  customField: LocalizationFieldDefinition | null
): LocalizationFieldDefinition | null => {
  if (viewMode === 'frc-field') {
    return { name: 'FRC field', width: FRC_FIELD_DIMENSIONS.width, depth: FRC_FIELD_DIMENSIONS.depth };
  }
  if (viewMode === 'custom-field') {
    if (
      customField &&
      Number.isFinite(customField.width) &&
      Number.isFinite(customField.depth) &&
      customField.width > 0 &&
      customField.depth > 0
    ) {
      return customField;
    }
    return DEFAULT_CUSTOM_FIELD;
  }
  return null;
};

export const getTopCameraBounds = (
  viewMode: LocalizationViewMode,
  customField: LocalizationFieldDefinition | null
) => {
  const field = getActiveField(viewMode, customField);
  if (field) {
    const padding = 1.2;
    return {
      halfWidth: field.width / 2 + padding,
      halfHeight: field.depth / 2 + padding
    };
  }
  const span = 25;
  return { halfWidth: span, halfHeight: span };
};

export type MainCameraView = {
  position: [number, number, number];
  target: [number, number, number];
  minDistance: number;
  maxDistance: number;
};

export const getMainCameraView = (
  viewMode: LocalizationViewMode,
  customField: LocalizationFieldDefinition | null
): MainCameraView => {
  const field = getActiveField(viewMode, customField);
  if (field) {
    const halfWidth = Math.max(0.5, field.width / 2);
    const halfDepth = Math.max(0.5, field.depth / 2);
    const radius = Math.sqrt(halfWidth * halfWidth + halfDepth * halfDepth);
    // Bias toward a tighter center-workspace view instead of fitting the full field.
    const zDistance = Math.max(4.8, radius * 0.6);
    const xOffset = Math.max(2.8, zDistance * 0.5);
    const yHeight = Math.max(3.1, zDistance * 0.56);
    return {
      position: [xOffset, yHeight, zDistance],
      target: [0, 0, 0],
      minDistance: 0.8,
      maxDistance: Math.max(55, zDistance * 4.2)
    };
  }

  return {
    position: [2.6, 2.3, 3.2],
    target: [0, 0, 0],
    minDistance: 0.45,
    maxDistance: 32
  };
};
