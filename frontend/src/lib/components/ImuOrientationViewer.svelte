<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount } from 'svelte';
  import * as THREE from 'three';
  import { OrbitControls } from 'three-stdlib';

  import { createCameraGroup, setCameraHighlight } from '$lib/3d/rig';
  import { AXIS_COLORS } from '$lib/ui/axisColors';
  import { imuQuaternionToThree, imuVec3ToThree, quatNormalize, type QuaternionWxyz } from '$lib/utils/imuFrames';
  import type { SensorOrientation } from '$lib/types/devices';
  import type { ImuStatus } from '$lib/types/systems';

  type Props = {
    orientation?: SensorOrientation;
    imu?: ImuStatus | null;
    showReferenceControls?: boolean;
    showLegend?: boolean;
    showWorldDecorations?: boolean;
    showGroundPlane?: boolean;
    cameraDistanceScale?: number;
  };

  const {
    orientation = { roll: 0, pitch: 0, yaw: 0 },
    imu = null,
    showReferenceControls = true,
    showLegend = true,
    showWorldDecorations = true,
    showGroundPlane = true,
    cameraDistanceScale = 1
  }: Props = $props();

  let container: HTMLDivElement;
  let canvas: HTMLCanvasElement;

  let scene: THREE.Scene | null = null;
  let perspective: THREE.PerspectiveCamera | null = null;
  let renderer: THREE.WebGLRenderer | null = null;
  let controls: OrbitControls | null = null;
  let cameraGroup: THREE.Group | null = null;
  let cameraPivot: THREE.Group | null = null;
  let worldAxes: THREE.AxesHelper | null = null;
  let groundPlane: THREE.Group | null = null;
  let localAxes: THREE.AxesHelper | null = null;
  let rotationRings: THREE.Group | null = null;
  let accelUpArrow: THREE.ArrowHelper | null = null;
  let worldUpArrow: THREE.ArrowHelper | null = null;
  let animationFrame: number | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let lastViewportWidth = 0;
  let lastViewportHeight = 0;
  let lastPixelRatio = 0;

  const worldUp = new THREE.Vector3(0, 1, 0);

  let fusedQuat = new THREE.Quaternion();
  let referenceFrame = $state<'world' | 'camera'>('world');
  let worldGroup: THREE.Group | null = null;

  onMount(() => {
    if (!browser) return;
    initScene();
    return dispose;
  });

  onDestroy(dispose);

  $effect(() => {
    updateOrientation();
  });

  function initScene() {
    scene = new THREE.Scene();
    scene.background = new THREE.Color(0x020617);

    worldGroup = new THREE.Group();
    scene.add(worldGroup);

    const ambient = new THREE.AmbientLight(0xffffff, 0.6);
    scene.add(ambient);

    const keyLight = new THREE.DirectionalLight(0xffffff, 0.7);
    keyLight.position.set(2.4, 3.4, 2.2);
    scene.add(keyLight);

    const fillLight = new THREE.DirectionalLight(0x70a0ff, 0.4);
    fillLight.position.set(-2.2, 1.5, -2.4);
    scene.add(fillLight);

    if (showGroundPlane) {
      groundPlane = new THREE.Group();

      const groundDisk = new THREE.Mesh(
        new THREE.CircleGeometry(1.18, 48),
        new THREE.MeshStandardMaterial({
          color: 0x070e1a,
          roughness: 0.95,
          metalness: 0.02,
          transparent: true,
          opacity: 0.42,
          depthWrite: false
        })
      );
      groundDisk.rotation.x = -Math.PI / 2;
      groundPlane.add(groundDisk);

      const grid = new THREE.GridHelper(1.9, 16, 0x2f4e73, 0x17283b);
      const gridMaterial = grid.material as THREE.Material & {
        opacity?: number;
        transparent?: boolean;
        depthWrite?: boolean;
      };
      gridMaterial.transparent = true;
      gridMaterial.opacity = 0.78;
      gridMaterial.depthWrite = false;
      grid.position.y = 0.001;
      groundPlane.add(grid);

      groundPlane.position.y = -0.08;
      worldGroup.add(groundPlane);
    }

    if (showWorldDecorations) {
      const grid = new THREE.GridHelper(2, 20, 0x1f2937, 0x0f172a);
      grid.position.y = -0.001;
      worldGroup.add(grid);

      worldAxes = createAxesHelper(0.75, 0.35);
      worldGroup.add(worldAxes);
      worldUpArrow = new THREE.ArrowHelper(
        worldUp.clone().normalize(),
        new THREE.Vector3(0, 0, 0),
        0.65,
        AXIS_COLORS.yHex,
        0.08,
        0.045
      );
      worldGroup.add(worldUpArrow);
    }

    cameraGroup = createCameraGroup({
      id: 'imu-camera',
      bodyColor: 0x2563eb,
      highlightColor: 0x60a5fa,
      forwardDotColor: AXIS_COLORS.accelHex,
      includeAxisGizmo: true,
      axisGizmoOpacity: 0.95
    });
    setCameraHighlight(cameraGroup, true);
    cameraPivot = new THREE.Group();
    cameraPivot.add(cameraGroup);

    localAxes = createAxesHelper(0.32, 0.95);
    cameraPivot.add(localAxes);

    rotationRings = createRotationRings(0.24, 0.008);
    cameraPivot.add(rotationRings);

    recenterCameraMesh();
    (cameraGroup.userData?.modelReady as Promise<void> | undefined)?.then(recenterCameraMesh).catch(recenterCameraMesh);
    scene.add(cameraPivot);
    updateOrientation();

    const distanceScale = Number.isFinite(cameraDistanceScale) ? THREE.MathUtils.clamp(cameraDistanceScale, 0.18, 2.5) : 1;
    perspective = new THREE.PerspectiveCamera(55, 1, 0.05, 10);
    perspective.position.set(0.9 * distanceScale, 0.7 * distanceScale, 1.2 * distanceScale);
    perspective.lookAt(0, 0.2, 0);

    renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
    renderer.outputColorSpace = THREE.SRGBColorSpace;

    controls = new OrbitControls(perspective, canvas);
    controls.enableDamping = true;
    controls.dampingFactor = 0.12;
    controls.enablePan = false;
    controls.minDistance = Math.max(0.12, 0.3 * distanceScale);
    controls.maxDistance = Math.max(1.1, 3.2 * distanceScale);
    controls.maxPolarAngle = Math.PI / 2 - 0.05;

    syncRendererSize();
    resizeObserver = new ResizeObserver(() => {
      syncRendererSize();
    });
    resizeObserver.observe(container);
    animate();
  }

  function animate() {
    if (!renderer || !scene || !perspective) return;
    if (!syncRendererSize()) {
      animationFrame = requestAnimationFrame(animate);
      return;
    }
    updateOrientation();
    controls?.update();
    renderer.render(scene, perspective);
    animationFrame = requestAnimationFrame(animate);
  }

  function syncRendererSize(): boolean {
    if (!container || !renderer || !perspective) return false;
    const ratio = Math.min(window.devicePixelRatio ?? 1, 2);
    const { clientWidth, clientHeight } = container;
    if (!clientWidth || !clientHeight) return false;
    const width = Math.max(1, Math.floor(clientWidth));
    const height = Math.max(1, Math.floor(clientHeight));
    const ratioChanged = Math.abs(lastPixelRatio - ratio) > 0.001;
    const sizeChanged = width !== lastViewportWidth || height !== lastViewportHeight;
    if (!ratioChanged && !sizeChanged) return true;

    lastPixelRatio = ratio;
    lastViewportWidth = width;
    lastViewportHeight = height;
    renderer.setPixelRatio(ratio);
    renderer.setSize(width, height, false);
    renderer.setViewport(0, 0, width, height);
    perspective.aspect = width / height;
    perspective.updateProjectionMatrix();
    return true;
  }

  function updateOrientation() {
    if (!cameraPivot || !worldGroup) return;

    // Prefer backend quaternion (converted to Three.js frame) to avoid gimbal lock.
    const backendQuat = orientation?.quaternion;
    const qThree = backendQuat ? toThreeQuaternion(backendQuat) : null;
    if (qThree) {
      if (referenceFrame === 'world') {
        worldGroup.quaternion.identity();
        cameraPivot.quaternion.copy(qThree).normalize();
      } else {
        cameraPivot.quaternion.identity();
        worldGroup.quaternion.copy(qThree).invert().normalize();
      }
      fusedQuat.copy(qThree);
      if (imu && imu.hasSample) {
        updateAccelUpArrow(imuVec3ToThree(imu.accel));
      } else {
        updateAccelUpArrow(null);
      }
      applyPositionTransform();
      return;
    }

    updateAccelUpArrow(null);

    // Euler fallback (degrees): pitch about +X, yaw about +Y, roll about +Z.
    // Use ZXY so yaw is applied last (camera-local yaw, not world yaw when rolled).
    const fallbackEuler = new THREE.Euler(
      THREE.MathUtils.degToRad(orientation?.pitch ?? 0),
      THREE.MathUtils.degToRad(orientation?.yaw ?? 0),
      THREE.MathUtils.degToRad(orientation?.roll ?? 0),
      'ZXY'
    );
    const fallbackQuat = new THREE.Quaternion().setFromEuler(fallbackEuler).normalize();
    if (referenceFrame === 'world') {
      worldGroup.quaternion.identity();
      cameraPivot.quaternion.copy(fallbackQuat);
    } else {
      cameraPivot.quaternion.identity();
      worldGroup.quaternion.copy(fallbackQuat).invert().normalize();
    }
    fusedQuat.copy(fallbackQuat);
    applyPositionTransform();
  }

  function recenterCameraMesh() {
    if (!cameraGroup || !cameraPivot) return;
    cameraGroup.position.set(0, 0, 0);
    cameraPivot.updateWorldMatrix(true, true);

    const target =
      cameraGroup.getObjectByName('camera-model') ??
      cameraGroup.getObjectByName('camera-placeholder') ??
      cameraGroup;

    const bbox = new THREE.Box3().setFromObject(target);
    const centerWorld = bbox.getCenter(new THREE.Vector3());
    const centerLocal = cameraPivot.worldToLocal(centerWorld.clone());
    cameraGroup.position.sub(centerLocal);
  }

  function toThreeQuaternion(input: { w: number; x: number; y: number; z: number }): THREE.Quaternion | null {
    const qBackend: QuaternionWxyz = { w: input.w, x: input.x, y: input.y, z: input.z };
    const normalized = quatNormalize(qBackend);
    const qThree = imuQuaternionToThree(normalized ?? qBackend);
    if (!Number.isFinite(qThree.w) || !Number.isFinite(qThree.x) || !Number.isFinite(qThree.y) || !Number.isFinite(qThree.z)) return null;
    return new THREE.Quaternion(qThree.x, qThree.y, qThree.z, qThree.w).normalize();
  }

  function currentImuPositionThree(): THREE.Vector3 | null {
    if (!imu?.hasSample || !imu.positionWorld) return null;
    const { x, y, z } = imu.positionWorld;
    if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) return null;
    const pThree = imuVec3ToThree({ x, y, z });
    return new THREE.Vector3(pThree.x, pThree.y, pThree.z);
  }

  function applyPositionTransform() {
    if (!cameraPivot || !worldGroup) return;
    const positionWorld = currentImuPositionThree() ?? new THREE.Vector3(0, 0, 0);

    if (referenceFrame === 'world') {
      cameraPivot.position.copy(positionWorld);
      worldGroup.position.set(0, 0, 0);
      return;
    }

    // Camera frame: keep camera at origin and transform world by inverse pose.
    // x_cam = R^-1 * (x_world - p_world) => t_world_group = -R^-1 * p_world.
    cameraPivot.position.set(0, 0, 0);
    const inv = fusedQuat.clone().invert();
    worldGroup.position.copy(positionWorld.clone().applyQuaternion(inv).negate());
  }

  function updateAccelUpArrow(accelGThree: { x: number; y: number; z: number } | null) {
    if (!scene || !worldGroup) return;
    if (!accelUpArrow) {
      accelUpArrow = new THREE.ArrowHelper(
        new THREE.Vector3(0, 1, 0),
        new THREE.Vector3(0, 0, 0),
        0.55,
        AXIS_COLORS.accelHex,
        0.08,
        0.045
      );
      worldGroup.add(accelUpArrow);
    }

    if (!accelGThree) {
      accelUpArrow.visible = false;
      return;
    }

    const accelUnit = new THREE.Vector3(accelGThree.x, accelGThree.y, accelGThree.z);
    const len = accelUnit.length();
    if (!Number.isFinite(len) || len <= 1e-9) {
      accelUpArrow.visible = false;
      return;
    }
    accelUnit.multiplyScalar(1 / len);

    // Choose sign so the arrow represents "up" (specific force direction) in world space.
    const accelWorld = accelUnit.clone().applyQuaternion(fusedQuat);
    if (accelWorld.dot(worldUp) < 0) accelUnit.negate();
    const accelWorldUp = accelUnit.applyQuaternion(fusedQuat).normalize();

    accelUpArrow.setDirection(accelWorldUp);
    accelUpArrow.visible = true;
  }

  function createAxesHelper(size: number, opacity: number) {
    const axes = new THREE.AxesHelper(size);
    setLineMaterialOverlay(axes.material, opacity);
    axes.renderOrder = 2;
    return axes;
  }

  function createRotationRings(radius: number, tube: number) {
    const group = new THREE.Group();

    const red = createRingMesh(radius, tube, AXIS_COLORS.xHex, 0.18);
    // Pitch axis: +X (right) in Three.js.
    // Default torus is in the XY plane (axis along +Z), so rotate to make its axis +X.
    red.rotation.y = Math.PI / 2;
    group.add(red);

    const green = createRingMesh(radius * 0.97, tube, AXIS_COLORS.yHex, 0.18);
    // Yaw axis: +Y (up) in Three.js.
    green.rotation.x = Math.PI / 2;
    group.add(green);

    const blue = createRingMesh(radius * 0.94, tube, AXIS_COLORS.zHex, 0.18);
    // Roll axis: +Z (forward) in Three.js.
    group.add(blue);

    group.position.set(0, 0.02, 0);
    group.renderOrder = 2;
    return group;
  }

  function createRingMesh(radius: number, tube: number, color: number, opacity: number) {
    const geometry = new THREE.TorusGeometry(radius, tube, 12, 64);
    const material = new THREE.MeshBasicMaterial({ color, transparent: true, opacity, depthTest: false });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.renderOrder = 2;
    return mesh;
  }

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

  function dispose() {
    if (animationFrame != null) {
      cancelAnimationFrame(animationFrame);
      animationFrame = null;
    }
    resizeObserver?.disconnect();
    controls?.dispose();
    renderer?.dispose();
    lastViewportWidth = 0;
    lastViewportHeight = 0;
    lastPixelRatio = 0;
    disposeObject(worldGroup);
    disposeObject(cameraGroup);
    disposeObject(cameraPivot);
    accelUpArrow = null;
    worldUpArrow = null;
    worldAxes = null;
    groundPlane = null;
    localAxes = null;
    rotationRings = null;
    scene?.clear();
    scene = null;
    worldGroup = null;
    perspective = null;
    renderer = null;
    controls = null;
    cameraGroup = null;
    cameraPivot = null;
  }

  function disposeObject(object: THREE.Object3D | null | undefined) {
    if (!object) return;
    object.traverse((child) => {
      if (child instanceof THREE.Mesh) {
        child.geometry.dispose();
        if (Array.isArray(child.material)) {
          child.material.forEach((material) => material.dispose?.());
        } else {
          child.material.dispose?.();
        }
      }
    });
  }
</script>

<div class="viewer" bind:this={container}>
  <canvas bind:this={canvas} aria-label="IMU orientation display"></canvas>
  {#if showReferenceControls}
    <div class="controls" aria-label="IMU viewer reference frame">
      <button type="button" class={`toggle ${referenceFrame === 'world' ? 'active' : ''}`} onclick={() => (referenceFrame = 'world')}>
        World
      </button>
      <button type="button" class={`toggle ${referenceFrame === 'camera' ? 'active' : ''}`} onclick={() => (referenceFrame = 'camera')}>
        Camera
      </button>
    </div>
  {/if}
  {#if showLegend}
    <div class="legend" aria-hidden="true">
      <div class="legend-row">
        <span class="legend-chip"><span class="swatch x"></span><span class="legend-text">X right</span></span>
        <span class="legend-chip"><span class="swatch y"></span><span class="legend-text">Y up</span></span>
        <span class="legend-chip"><span class="swatch z"></span><span class="legend-text">Z forward</span></span>
        <span class="legend-chip"><span class="swatch up"></span><span class="legend-text">World up</span></span>
        <span class="legend-chip"><span class="swatch accel"></span><span class="legend-text">Accel up</span></span>
      </div>
    </div>
  {/if}
</div>

<style>
  .viewer {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 0;
    border-radius: 0.75rem;
    border: 1px solid color-mix(in srgb, var(--color-surface-700) 60%, transparent);
    background: radial-gradient(circle at top, rgba(37, 99, 235, 0.14), rgba(2, 6, 23, 0.85));
    overflow: hidden;
  }

  .controls {
    position: absolute;
    bottom: 0.65rem;
    right: 0.65rem;
    display: flex;
    gap: 0.35rem;
    z-index: 3;
  }

  .toggle {
    pointer-events: auto;
    user-select: none;
    border-radius: 0.5rem;
    border: 1px solid color-mix(in srgb, var(--color-surface-700) 70%, transparent);
    background: color-mix(in srgb, rgba(2, 6, 23, 0.8) 82%, transparent);
    color: color-mix(in srgb, var(--color-surface-200) 92%, white);
    font-size: 0.6rem;
    letter-spacing: 0.22em;
    text-transform: uppercase;
    padding: 0.4rem 0.55rem;
    line-height: 1;
  }

  .toggle.active {
    border-color: color-mix(in srgb, var(--color-primary-500) 75%, transparent);
    background: color-mix(in srgb, rgba(37, 99, 235, 0.22) 50%, rgba(2, 6, 23, 0.75));
    color: color-mix(in srgb, var(--color-primary-100) 85%, white);
  }

  .legend {
    position: absolute;
    top: 0.5rem;
    left: 0.65rem;
    max-width: min(86%, 30rem);
    padding: 0.3rem 0.45rem;
    border-radius: 0.5rem;
    border: 1px solid color-mix(in srgb, var(--color-surface-700) 65%, transparent);
    background: color-mix(in srgb, rgba(2, 6, 23, 0.78) 82%, transparent);
    color: color-mix(in srgb, var(--color-surface-200) 90%, white);
    font-size: 0.6rem;
    line-height: 1;
    box-sizing: border-box;
    user-select: none;
    pointer-events: none;
    text-shadow: 0 1px 0 rgba(0, 0, 0, 0.35);
    overflow: auto;
  }

  .legend-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem 0.5rem;
  }

  .legend-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.28rem;
    white-space: nowrap;
  }

  .legend-text {
    color: color-mix(in srgb, var(--color-surface-200) 88%, white);
  }

  @media (max-width: 520px) {
    .legend {
      left: 0.5rem;
      max-width: min(90%, 23rem);
    }
  }

  .swatch {
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 0.2rem;
    border: 1px solid rgba(255, 255, 255, 0.14);
    background: rgba(148, 163, 184, 0.2);
  }

  .swatch.x {
    background: rgba(239, 68, 68, 0.85);
  }

  .swatch.y {
    background: rgba(34, 197, 94, 0.85);
  }

  .swatch.z {
    background: rgba(59, 130, 246, 0.85);
  }

  .swatch.up {
    background: rgba(34, 197, 94, 0.55);
  }

  .swatch.accel {
    background: rgba(250, 204, 21, 0.75);
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
