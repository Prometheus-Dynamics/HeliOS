import * as THREE from 'three';
import { FontLoader, TextGeometry } from 'three/examples/jsm/Addons.js';
import type { Font } from 'three/examples/jsm/Addons.js';
import helvetiker from 'three/examples/fonts/helvetiker_regular.typeface.json';

import type { PoseRotation, PoseVector, RigPose, RobotDimensions } from '$lib/types/rig';

export const DEFAULT_ROBOT_DIMENSIONS: RobotDimensions = {
  width: 0.6,
  length: 0.6,
  bumperHeight: 0.127,
  bumperThickness: 0.0508,
  groundClearance: 0
};

export const CAMERA_BODY_SIZE = {
  width: 0.06,
  height: 0.045,
  depth: 0.032
};

type CameraMeshOptions = {
  id?: string;
  bodyColor?: number;
  highlightColor?: number;
  forwardDotColor?: number;
  size?: typeof CAMERA_BODY_SIZE;
  modelUrl?: string | null;
  includeAxisGizmo?: boolean;
  axisGizmoOpacity?: number;
};

type RobotMaterialOptions = {
  bumperMaterial?: THREE.Material;
  labelMaterial?: THREE.Material;
  includeOrientationLabels?: boolean;
  includeDirectionIndicator?: boolean;
};

const DEFAULT_CAMERA_BODY_COLOR = 0x1f2937;
const DEFAULT_CAMERA_HIGHLIGHT_COLOR = 0x38bdf8;
const DEFAULT_CAMERA_FORWARD_COLOR = 0xfacc15;
const DEFAULT_CAMERA_MODEL_URL: string | null = null;
const DEFAULT_CAMERA_AXIS_GIZMO = false;
const DEFAULT_CAMERA_AXIS_GIZMO_OPACITY = 0.85;

function setLineMaterialOverlay(material: THREE.Material | THREE.Material[], opacity: number) {
  const apply = (mat: THREE.Material) => {
    const line = mat as THREE.Material & { depthTest?: boolean; transparent?: boolean; opacity?: number };
    line.depthTest = false;
    line.transparent = true;
    line.opacity = opacity;
  };
  if (Array.isArray(material)) {
    material.forEach(apply);
  } else {
    apply(material);
  }
}

function createAxesHelper(size: number, opacity: number): THREE.AxesHelper {
  const axes = new THREE.AxesHelper(size);
  setLineMaterialOverlay(axes.material, opacity);
  axes.renderOrder = 2;
  return axes;
}

type CachedModel = {
  url: string;
  prototype: THREE.Object3D;
};

let cameraModelCache: CachedModel | null = null;
let cameraModelLoadPromise: Promise<CachedModel> | null = null;
let cameraModelLoadUrl: string | null = null;

function isGzipPayload(bytes: Uint8Array): boolean {
  return bytes.length >= 2 && bytes[0] === 0x1f && bytes[1] === 0x8b;
}

async function arrayBufferToUtf8Text(buffer: ArrayBuffer): Promise<string> {
  const bytes = new Uint8Array(buffer);
  if (isGzipPayload(bytes)) {
    const DecompressionStreamCtor = (globalThis as unknown as { DecompressionStream?: new (format: string) => TransformStream })
      .DecompressionStream;
    if (!DecompressionStreamCtor) {
      try {
        const { gunzipSync, strFromU8 } = await import('fflate');
        return strFromU8(gunzipSync(bytes));
      } catch (err) {
        throw new Error(`VRML model is gzipped but decompression is unavailable (${(err as Error)?.message ?? 'unknown error'})`);
      }
    }
    const decompressedResponse = new Response(new Blob([bytes]).stream().pipeThrough(new DecompressionStreamCtor('gzip')));
    return await decompressedResponse.text();
  }
  return new TextDecoder('utf-8', { fatal: false }).decode(bytes);
}

async function loadVrmlFromUrl(url: string): Promise<THREE.Object3D> {
  const [{ VRMLLoader }, response] = await Promise.all([import('three/examples/jsm/loaders/VRMLLoader.js'), fetch(url)]);
  if (!response.ok) {
    throw new Error(`Failed to fetch VRML model (${response.status})`);
  }
  const buffer = await response.arrayBuffer();
  const text = await arrayBufferToUtf8Text(buffer);
  const absoluteUrl = new URL(url, window.location.href).href;
  const baseUrl = new URL('.', absoluteUrl).href;
  const loader = new VRMLLoader();
  return loader.parse(text, baseUrl);
}

async function loadCameraModelPrototype(url: string): Promise<CachedModel> {
  if (cameraModelCache && cameraModelCache.url === url) return cameraModelCache;
  if (cameraModelLoadPromise && cameraModelLoadUrl === url) return cameraModelLoadPromise;

  cameraModelLoadUrl = url;
  cameraModelLoadPromise = loadVrmlFromUrl(url).then((object) => {
    const cached = { url, prototype: object };
    cameraModelCache = cached;
    return cached;
  });

  return cameraModelLoadPromise;
}

function cloneModelUnique(object: THREE.Object3D): THREE.Object3D {
  const clone = object.clone(true);
  clone.traverse((node) => {
    if (!(node instanceof THREE.Mesh)) return;
    node.geometry = node.geometry.clone();
    if (Array.isArray(node.material)) {
      node.material = node.material.map((material) => material.clone());
    } else {
      node.material = node.material.clone();
    }
  });
  return clone;
}

function fitModelToCameraBounds(model: THREE.Object3D, target: typeof CAMERA_BODY_SIZE): void {
  const bbox = new THREE.Box3().setFromObject(model);
  const size = bbox.getSize(new THREE.Vector3());
  const safeSize = new THREE.Vector3(Math.max(size.x, 1e-6), Math.max(size.y, 1e-6), Math.max(size.z, 1e-6));
  const scale = Math.min(target.width / safeSize.x, target.height / safeSize.y, target.depth / safeSize.z);
  model.scale.multiplyScalar(scale);

  const scaledBox = new THREE.Box3().setFromObject(model);
  const center = scaledBox.getCenter(new THREE.Vector3());
  const minY = scaledBox.min.y;
  model.position.sub(center);
  model.position.y -= minY;
}

function applyCameraModelMaterials(model: THREE.Object3D, baseColor: THREE.Color, highlighted: boolean, highlightColor: THREE.Color): THREE.Material[] {
  const materials: THREE.Material[] = [];
  const activeColor = highlighted ? highlightColor : baseColor;
  const idleEmissive = baseColor.clone().multiplyScalar(0.18);
  model.traverse((node) => {
    if (!(node instanceof THREE.Mesh)) return;
    node.castShadow = true;
    node.receiveShadow = true;
    const list = Array.isArray(node.material) ? node.material : [node.material];
    for (const material of list) {
      materials.push(material);
      const anyMaterial = material as unknown as { color?: THREE.Color; emissive?: THREE.Color; emissiveIntensity?: number };
      if (anyMaterial.color) {
        anyMaterial.color.copy(activeColor);
      }
      if (anyMaterial.emissive) {
        anyMaterial.emissive.copy(highlighted ? activeColor : idleEmissive);
      }
      if (typeof anyMaterial.emissiveIntensity === 'number') {
        anyMaterial.emissiveIntensity = highlighted ? 0.28 : 0.12;
      }
      material.needsUpdate = true;
    }
  });
  return materials;
}

function addSilhouetteOutline(mesh: THREE.Mesh, color: number, scale = 1.06): void {
  const outlineMaterial = new THREE.MeshBasicMaterial({
    color,
    side: THREE.BackSide,
    transparent: true,
    opacity: 0.95,
    depthWrite: false
  });
  outlineMaterial.polygonOffset = true;
  outlineMaterial.polygonOffsetFactor = 1;
  outlineMaterial.polygonOffsetUnits = 1;
  const outline = new THREE.Mesh(mesh.geometry, outlineMaterial);
  outline.name = `${mesh.name || 'mesh'}-outline`;
  outline.castShadow = false;
  outline.receiveShadow = false;
  outline.scale.setScalar(scale);
  outline.renderOrder = 1;
  mesh.add(outline);
}

export function createCameraGroup(options: CameraMeshOptions = {}): THREE.Group {
  const {
    id,
    bodyColor = DEFAULT_CAMERA_BODY_COLOR,
    highlightColor = DEFAULT_CAMERA_HIGHLIGHT_COLOR,
    forwardDotColor = DEFAULT_CAMERA_FORWARD_COLOR,
    size = CAMERA_BODY_SIZE,
    modelUrl = DEFAULT_CAMERA_MODEL_URL,
    includeAxisGizmo = DEFAULT_CAMERA_AXIS_GIZMO,
    axisGizmoOpacity = DEFAULT_CAMERA_AXIS_GIZMO_OPACITY
  } = options;

  const group = new THREE.Group();
  if (id) {
    group.name = id;
  }

  group.userData.baseColor = new THREE.Color(bodyColor);
  group.userData.highlightColor = new THREE.Color(highlightColor);
  group.userData.highlighted = false;
  group.userData.highlightMaterials = [] as THREE.Material[];
  let resolveModelReady: (() => void) | null = null;
  group.userData.modelReady = new Promise<void>((resolve) => {
    resolveModelReady = resolve;
  });

  const placeholder = new THREE.Group();
  placeholder.name = 'camera-placeholder';

  const bodyMaterial = new THREE.MeshStandardMaterial({
    color: bodyColor,
    metalness: 0.35,
    roughness: 0.38
  });
  const bodyGeometry = new THREE.BoxGeometry(size.width, size.height, size.depth);
  const body = new THREE.Mesh(bodyGeometry, bodyMaterial);
  body.castShadow = true;
  body.receiveShadow = true;
  body.position.y = size.height / 2;
  addSilhouetteOutline(body, 0xf8fafc, 1.07);
  placeholder.add(body);

  const forwardDotMaterial = new THREE.MeshStandardMaterial({
    color: forwardDotColor,
    emissive: forwardDotColor,
    emissiveIntensity: 0.35,
    roughness: 0.2,
    metalness: 0.1
  });
  const forwardDot = new THREE.Mesh(
    new THREE.SphereGeometry(Math.min(size.height, size.width) * 0.12, 20, 14),
    forwardDotMaterial
  );
  // Localization viewer uses +Z as forward (robot front is +Z).
  forwardDot.position.set(0, size.height * 0.65, size.depth / 2 + 0.002);
  group.add(forwardDot);

  if (includeAxisGizmo) {
    const gizmoSize = Math.max(size.width, size.height, size.depth) * 1.25;
    const axes = createAxesHelper(gizmoSize, axisGizmoOpacity);
    axes.position.set(0, size.height / 2, 0);
    group.add(axes);
  }

  const mountGeometry = new THREE.BoxGeometry(size.width * 0.7, size.height * 0.12, size.depth * 0.6);
  const mountMaterial = bodyMaterial.clone();
  const mountBaseColor = new THREE.Color(bodyColor).multiplyScalar(0.85);
  mountMaterial.color.copy(mountBaseColor);
  const mount = new THREE.Mesh(mountGeometry, mountMaterial);
  mount.position.y = mountGeometry.parameters.height / 2;
  mount.castShadow = true;
  mount.receiveShadow = true;
  addSilhouetteOutline(mount, 0xf8fafc, 1.05);
  placeholder.add(mount);

  group.userData.bodyMaterial = bodyMaterial;
  group.userData.mountMaterial = mountMaterial;
  group.userData.mountBaseColor = mountBaseColor;
  group.userData.highlightMaterials = [bodyMaterial, mountMaterial];

  group.add(placeholder);

  if (modelUrl && typeof window !== 'undefined') {
    void loadCameraModelPrototype(modelUrl)
      .then(({ prototype }) => {
        const model = cloneModelUnique(prototype);
        model.name = 'camera-model';
        fitModelToCameraBounds(model, size);

        placeholder.visible = false;
        placeholder.traverse((node) => {
          if (!(node instanceof THREE.Mesh)) return;
          node.geometry?.dispose?.();
          if (Array.isArray(node.material)) {
            node.material.forEach((material) => material.dispose());
          } else {
            node.material?.dispose?.();
          }
        });
        placeholder.clear();

        const highlighted = Boolean(group.userData.highlighted);
        const base = group.userData.baseColor as THREE.Color;
        const highlight = group.userData.highlightColor as THREE.Color;
        group.userData.highlightMaterials = applyCameraModelMaterials(model, base, highlighted, highlight);

        group.add(model);
        resolveModelReady?.();
        resolveModelReady = null;
      })
      .catch((error) => {
        console.warn('Failed to load camera model', error);
        // Fall back to placeholder shapes.
        resolveModelReady?.();
        resolveModelReady = null;
      });
  } else {
    resolveModelReady?.();
    resolveModelReady = null;
  }

  return group;
}

export function setCameraHighlight(group: THREE.Group, highlighted: boolean): void {
  group.userData.highlighted = highlighted;

  const baseColor: THREE.Color = group.userData.baseColor ?? new THREE.Color(DEFAULT_CAMERA_BODY_COLOR);
  const highlightColor: THREE.Color = group.userData.highlightColor ?? new THREE.Color(DEFAULT_CAMERA_HIGHLIGHT_COLOR);
  const materials: THREE.Material[] = Array.isArray(group.userData.highlightMaterials) ? group.userData.highlightMaterials : [];
  const activeColor = highlighted ? highlightColor : baseColor;
  const idleEmissive = baseColor.clone().multiplyScalar(0.18);
  for (const material of materials) {
    const anyMaterial = material as unknown as { color?: THREE.Color; emissive?: THREE.Color; emissiveIntensity?: number };
    if (anyMaterial.color) {
      anyMaterial.color.copy(activeColor);
    }
    if (anyMaterial.emissive) {
      anyMaterial.emissive.copy(highlighted ? activeColor : idleEmissive);
    }
    if (typeof anyMaterial.emissiveIntensity === 'number') {
      anyMaterial.emissiveIntensity = highlighted ? 0.32 : 0.12;
    }
    material.needsUpdate = true;
  }

  const mountMaterial = group.userData.mountMaterial as THREE.MeshStandardMaterial | undefined;
  const mountBaseColor: THREE.Color | undefined = group.userData.mountBaseColor;
  if (mountMaterial && mountBaseColor) {
    const mountHighlight = highlightColor.clone().multiplyScalar(0.85);
    const mountIdleEmissive = mountBaseColor.clone().multiplyScalar(0.18);
    mountMaterial.color.copy(highlighted ? mountHighlight : mountBaseColor);
    mountMaterial.emissive.copy(highlighted ? mountHighlight : mountIdleEmissive);
    mountMaterial.emissiveIntensity = highlighted ? 0.22 : 0.1;
    mountMaterial.needsUpdate = true;
  }
}

const DEFAULT_BUMPER_COLOR = 0x991b1b;
const DEFAULT_BUMPER_EMISSIVE = 0x330b0b;
const DEFAULT_LABEL_COLOR = 0xf8fafc;

type FontJson = Parameters<FontLoader['parse']>[0];
const orientationFontSource = helvetiker as FontJson;
let orientationFont: Font | null = null;

function ensureOrientationFont(): Font | null {
  if (orientationFont) return orientationFont;
  try {
    orientationFont = new FontLoader().parse(orientationFontSource);
  } catch {
    orientationFont = null;
  }
  return orientationFont;
}

export function createRobotBaseGroup(dimensions: RobotDimensions, materials: RobotMaterialOptions = {}): THREE.Group {
  const dims: RobotDimensions = {
    width: dimensions.width ?? DEFAULT_ROBOT_DIMENSIONS.width,
    length: dimensions.length ?? DEFAULT_ROBOT_DIMENSIONS.length,
    bumperHeight: dimensions.bumperHeight ?? DEFAULT_ROBOT_DIMENSIONS.bumperHeight,
    bumperThickness: dimensions.bumperThickness ?? DEFAULT_ROBOT_DIMENSIONS.bumperThickness,
    groundClearance: dimensions.groundClearance ?? DEFAULT_ROBOT_DIMENSIONS.groundClearance
  };

  const group = new THREE.Group();
  const bumperMaterial =
    materials.bumperMaterial ??
    new THREE.MeshStandardMaterial({
      color: DEFAULT_BUMPER_COLOR,
      emissive: DEFAULT_BUMPER_EMISSIVE,
      emissiveIntensity: 0.2,
      roughness: 0.55,
      metalness: 0.2
    });

  const bumperGeometry = createBumperGeometry(dims);
  bumperGeometry.computeBoundingBox();
  const bumperCenterY =
    ((bumperGeometry.boundingBox?.min.y ?? 0) + (bumperGeometry.boundingBox?.max.y ?? 0)) / 2;

  const bumper = new THREE.Mesh(bumperGeometry, bumperMaterial);
  bumper.position.y = dims.groundClearance - (bumperGeometry.boundingBox?.min.y ?? 0);
  bumper.castShadow = true;
  bumper.receiveShadow = true;
  group.add(bumper);

  if (materials.includeOrientationLabels !== false) {
    const font = ensureOrientationFont();
    if (font) {
      const labelMaterial: THREE.Material =
        materials.labelMaterial ??
        new THREE.MeshStandardMaterial({
          color: DEFAULT_LABEL_COLOR,
          emissive: 0x0f172a,
          emissiveIntensity: 0.25,
          roughness: 0.35,
          metalness: 0.2
        });
      addOrientationLabels(group, font, labelMaterial, dims, bumperCenterY + bumper.position.y);
    }
  }

  if (materials.includeDirectionIndicator !== false) {
    addDirectionIndicator(group, dims, bumper.position.y + bumperCenterY);
  }

  return group;
}

function addOrientationLabels(
  group: THREE.Group,
  font: Font,
  material: THREE.Material,
  dims: RobotDimensions,
  verticalCenter: number
) {
  const textDepth = 0.012;
  const targetHeight = dims.bumperHeight * 0.65;
  const frontBackWidth = Math.max(dims.width - dims.bumperThickness * 1.4, 0.05);
  const sideWidth = Math.max(dims.length - dims.bumperThickness * 1.4, 0.05);
  const epsilon = 0.0005;

  const frontFace = dims.length / 2 + dims.bumperThickness / 2;
  const sideFace = dims.width / 2 + dims.bumperThickness / 2;
  const frontPosition = frontFace - textDepth / 2 - epsilon;
  const backPosition = -frontFace + textDepth / 2 + epsilon;
  const leftPosition = -sideFace + textDepth / 2 + epsilon;
  const rightPosition = sideFace - textDepth / 2 - epsilon;

  const placements = [
    { text: 'FRONT', maxWidth: frontBackWidth, position: new THREE.Vector3(0, verticalCenter, frontPosition), rotationY: 0 },
    { text: 'BACK', maxWidth: frontBackWidth, position: new THREE.Vector3(0, verticalCenter, backPosition), rotationY: Math.PI },
    { text: 'RIGHT', maxWidth: sideWidth, position: new THREE.Vector3(leftPosition, verticalCenter, 0), rotationY: -Math.PI / 2 },
    { text: 'LEFT', maxWidth: sideWidth, position: new THREE.Vector3(rightPosition, verticalCenter, 0), rotationY: Math.PI / 2 }
  ];

  placements.forEach(({ text, maxWidth, position, rotationY }) => {
    const geometry = createCenteredTextGeometry(font, text, textDepth, maxWidth, targetHeight);
    const mesh = new THREE.Mesh(geometry, material.clone());
    mesh.position.copy(position);
    mesh.rotation.set(0, rotationY, 0);
    mesh.castShadow = true;
    group.add(mesh);
  });
}

function createCenteredTextGeometry(font: Font, text: string, depth: number, maxWidth: number, maxHeight: number) {
  const geometry = new TextGeometry(text, {
    font,
    size: 0.25,
    depth,
    curveSegments: 6,
    bevelEnabled: false
  });

  geometry.computeBoundingBox();
  const bbox = geometry.boundingBox ?? new THREE.Box3();
  const rawWidth = bbox.max.x - bbox.min.x || 1;
  const rawHeight = bbox.max.y - bbox.min.y || 1;
  const scale = Math.min(maxWidth / rawWidth, maxHeight / rawHeight);
  geometry.scale(scale, scale, 1);
  geometry.computeBoundingBox();
  const centeredBox = geometry.boundingBox!;
  const center = centeredBox.getCenter(new THREE.Vector3());
  geometry.translate(-center.x, -center.y, -center.z);

  return geometry;
}

function addDirectionIndicator(group: THREE.Group, dims: RobotDimensions, verticalCenter: number) {
  const frontFace = dims.length / 2 + dims.bumperThickness / 2;
  const arrowHeight = dims.bumperHeight * 0.82;
  const arrowWidth = dims.width * 0.24;
  const shaftRatio = 0.55;
  const arrowThickness = 0.015;
  const inset = 0.001;

  const halfWidth = arrowWidth / 2;
  const shaftHeight = arrowHeight * shaftRatio;

  const shape = new THREE.Shape();
  shape.moveTo(-halfWidth, -arrowHeight / 2);
  shape.lineTo(halfWidth, -arrowHeight / 2);
  shape.lineTo(halfWidth, -arrowHeight / 2 + shaftHeight);
  shape.lineTo(halfWidth * 0.35, -arrowHeight / 2 + shaftHeight);
  shape.lineTo(0, arrowHeight / 2);
  shape.lineTo(-halfWidth * 0.35, -arrowHeight / 2 + shaftHeight);
  shape.lineTo(-halfWidth, -arrowHeight / 2 + shaftHeight);
  shape.lineTo(-halfWidth, -arrowHeight / 2);
  shape.closePath();

  const geometry = new THREE.ExtrudeGeometry(shape, {
    depth: arrowThickness,
    bevelEnabled: false
  });
  geometry.translate(0, 0, -arrowThickness / 2);

  const material = new THREE.MeshStandardMaterial({
    color: 0xfbbf24,
    emissive: 0x7c2d12,
    emissiveIntensity: 0.55,
    roughness: 0.3,
    metalness: 0.2
  });

  const arrow = new THREE.Mesh(geometry, material);
  arrow.position.set(0, verticalCenter, frontFace + arrowThickness / 2 - inset);
  arrow.layers.set(1);
  arrow.castShadow = true;
  group.add(arrow);
}

export function createBumperGeometry(dimensions: RobotDimensions): THREE.ExtrudeGeometry {
  const bumperThickness = Math.max(dimensions.bumperThickness, 0.001);
  const innerWidth = Math.max(dimensions.width, 0.05);
  const innerLength = Math.max(dimensions.length, 0.05);
  const outerWidth = innerWidth + bumperThickness * 2;
  const outerLength = innerLength + bumperThickness * 2;
  const outerRadius = Math.max(bumperThickness / 2, 0.02);

  const shape = new THREE.Shape();
  roundedRect(shape, -outerWidth / 2, -outerLength / 2, outerWidth, outerLength, outerRadius);

  const hole = new THREE.Path();
  roundedRect(
    hole,
    -innerWidth / 2,
    -innerLength / 2,
    innerWidth,
    innerLength,
    Math.max(0, outerRadius - bumperThickness / 2)
  );
  shape.holes.push(hole);

  const geometry = new THREE.ExtrudeGeometry(shape, {
    depth: dimensions.bumperHeight,
    bevelEnabled: true,
    bevelThickness: 0.01,
    bevelSize: 0.01,
    bevelSegments: 2
  });
  geometry.rotateX(-Math.PI / 2);
  return geometry;
}

function roundedRect(shape: THREE.Shape | THREE.Path, x: number, y: number, width: number, height: number, radius: number) {
  const r = Math.min(radius, width / 2, height / 2);
  shape.moveTo(x + r, y);
  shape.lineTo(x + width - r, y);
  shape.quadraticCurveTo(x + width, y, x + width, y + r);
  shape.lineTo(x + width, y + height - r);
  shape.quadraticCurveTo(x + width, y + height, x + width - r, y + height);
  shape.lineTo(x + r, y + height);
  shape.quadraticCurveTo(x, y + height, x, y + height - r);
  shape.lineTo(x, y + r);
  shape.quadraticCurveTo(x, y, x + r, y);
}

export function applyPose(group: THREE.Group, pose: PoseVector | RigPose, rotation?: PoseRotation) {
  const translation = 'translation' in pose ? pose.translation : pose;
  const rotationSrc = 'rotation' in pose ? pose.rotation : rotation;
  group.position.set(translation.y, translation.z, translation.x);
  if (rotationSrc) {
    const euler = new THREE.Euler(
      THREE.MathUtils.degToRad(-rotationSrc.pitch),
      THREE.MathUtils.degToRad(rotationSrc.yaw),
      THREE.MathUtils.degToRad(rotationSrc.roll),
      'YXZ'
    );
    group.quaternion.setFromEuler(euler).normalize();
  }
}
