<script lang="ts">
  import { browser } from '$app/environment';
  import { createEventDispatcher, onDestroy, onMount } from 'svelte';
  import * as THREE from 'three';
  import { OrbitControls } from 'three-stdlib';

  import { applyPose, createCameraGroup, createRobotBaseGroup, DEFAULT_ROBOT_DIMENSIONS, setCameraHighlight } from '$lib/3d/rig';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';

  type Props = {
    robot?: RobotDimensions;
    cameras?: RigCameraInfo[];
    selectedCamera?: string | null;
  };

const {
  robot = DEFAULT_ROBOT_DIMENSIONS,
  cameras = [],
  selectedCamera = null
}: Props = $props();

  const dispatch = createEventDispatcher<{ select: string | null }>();

  let canvas: HTMLCanvasElement;

  let scene: THREE.Scene | null = null;
  let orbitCamera: THREE.PerspectiveCamera | null = null;
  let renderer: THREE.WebGLRenderer | null = null;
  let controls: OrbitControls | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let animationFrame: number | null = null;
  let robotGroup: THREE.Group | null = null;
  let cameraGroup: THREE.Group | null = null;
  const cameraMeshes = new Map<string, THREE.Group>();

let robotDimensions = $state<RobotDimensions>(normalizeRobot(robot));
let cameraEntries = $state<RigCameraInfo[]>(normalizeCameras(cameras));
let selected = $state<string | null>(selectedCamera ?? null);

  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();
  let pointerDown: { x: number; y: number } | null = null;
  let pointerDragged = false;

  onMount(() => {
    initScene();
    return dispose;
  });

  onDestroy(dispose);

  function initScene() {
    if (!browser) return;

    scene = new THREE.Scene();
    scene.background = new THREE.Color(0x05070b);

    const ambient = new THREE.AmbientLight(0xffffff, 0.75);
    scene.add(ambient);

    const dirLight = new THREE.DirectionalLight(0xffffff, 0.7);
    dirLight.position.set(4, 6, 3);
    dirLight.castShadow = true;
    scene.add(dirLight);

    const grid = new THREE.GridHelper(4, 16, 0x1f2937, 0x0f172a);
    grid.position.y = -0.001;
    scene.add(grid);

    robotGroup = new THREE.Group();
    scene.add(robotGroup);
    rebuildRobot();

    cameraGroup = new THREE.Group();
    scene.add(cameraGroup);
    rebuildCameras();

    orbitCamera = new THREE.PerspectiveCamera(55, 1, 0.05, 30);
    orbitCamera.position.set(2.8, 2.1, 2.6);
    orbitCamera.lookAt(0, 0, 0);

    renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
    renderer.outputColorSpace = THREE.SRGBColorSpace;

    controls = new OrbitControls(orbitCamera, canvas);
    controls.enableDamping = true;
    controls.minDistance = 0.3;
    controls.maxDistance = 10;
    controls.maxPolarAngle = Math.PI / 2 - 0.05;
    controls.addEventListener('start', () => {
      pointerDragged = true;
    });
    controls.addEventListener('end', () => {
      pointerDragged = false;
    });

    canvas.addEventListener('pointerdown', handlePointerDown);
    canvas.addEventListener('pointermove', handlePointerMove);
    canvas.addEventListener('pointerup', handlePointerUp);
    canvas.addEventListener('pointerleave', handlePointerCancel);

    updateRendererSize();
    window.addEventListener('resize', updateRendererSize);
    if ('ResizeObserver' in globalThis && canvas) {
      resizeObserver = new ResizeObserver(() => {
        updateRendererSize();
      });
      resizeObserver.observe(canvas);
    }
    animate();
  }

  function animate() {
    if (!renderer || !scene || !orbitCamera) return;
    controls?.update();
    renderer.render(scene, orbitCamera);
    animationFrame = requestAnimationFrame(animate);
  }

  function updateRendererSize() {
    if (!browser || !orbitCamera || !renderer || !canvas) return;
    const ratio = Math.min(window.devicePixelRatio ?? 1, 2);
    const { clientWidth: width, clientHeight: height } = canvas;
    if (!width || !height) return;
    renderer.setPixelRatio(ratio);
    renderer.setSize(width, height, false);
    orbitCamera.aspect = width / height;
    orbitCamera.updateProjectionMatrix();
  }

  function rebuildRobot() {
    if (!scene || !robotGroup) return;
    robotGroup.clear();
    const base = createRobotBaseGroup(robotDimensions);
    robotGroup.add(base);
  }

  function rebuildCameras() {
    if (!cameraGroup) return;
    cameraGroup.clear();
    for (const mesh of cameraMeshes.values()) {
      disposeObject(mesh);
    }
    cameraMeshes.clear();
    for (const entry of cameraEntries) {
      if (!entry.pose) continue;
      const mesh = createCameraGroup({
        id: entry.uid,
        bodyColor: 0x1f2937,
        highlightColor: 0x38bdf8,
        forwardDotColor: 0x38bdf8,
        includeAxisGizmo: true,
        axisGizmoOpacity: 0.65
      });
      mesh.userData.cameraUid = entry.uid;
      setCameraHighlight(mesh, selected === entry.uid);
      applyPose(mesh, entry.pose);
      cameraGroup.add(mesh);
      cameraMeshes.set(entry.uid, mesh);
    }
    updateSelectionHighlight();
  }

  function updateSelectionHighlight() {
    for (const [uid, mesh] of cameraMeshes.entries()) {
      setCameraHighlight(mesh, uid === selected);
    }
  }

  function handlePointerDown(event: PointerEvent) {
    pointerDown = { x: event.clientX, y: event.clientY };
    pointerDragged = false;
  }

  function handlePointerMove(event: PointerEvent) {
    if (!pointerDown) return;
    const dx = event.clientX - pointerDown.x;
    const dy = event.clientY - pointerDown.y;
    if (Math.hypot(dx, dy) > 4) {
      pointerDragged = true;
    }
  }

  function handlePointerUp(event: PointerEvent) {
    if (!pointerDown) return handlePointerCancel();
    if (!pointerDragged) {
      selectFromPointer(event);
    }
    pointerDown = null;
  }

  function handlePointerCancel() {
    pointerDown = null;
  }

  function selectFromPointer(event: PointerEvent) {
    if (!orbitCamera || !cameraGroup || !renderer) return;
    const rect = canvas.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    pointer.x = (x / rect.width) * 2 - 1;
    pointer.y = -(y / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, orbitCamera);
    const intersections = raycaster.intersectObjects(cameraGroup.children, true);
    const hit = intersections.find((intersection) => intersection.object.parent?.userData.cameraUid ?? intersection.object.userData.cameraUid);
    if (!hit) {
      dispatch('select', null);
      return;
    }
    const node = hit.object;
    const uid: string | undefined =
      node.userData.cameraUid ?? node.parent?.userData.cameraUid ?? node.parent?.parent?.userData.cameraUid;
    if (uid) {
      dispatch('select', uid);
    } else {
      dispatch('select', null);
    }
  }

function dispose() {
  if (animationFrame) {
    cancelAnimationFrame(animationFrame);
    animationFrame = null;
  }
  if (browser) {
    window.removeEventListener('resize', updateRendererSize);
  }
  resizeObserver?.disconnect();
  resizeObserver = null;
  canvas?.removeEventListener('pointerdown', handlePointerDown);
  canvas?.removeEventListener('pointermove', handlePointerMove);
  canvas?.removeEventListener('pointerup', handlePointerUp);
  canvas?.removeEventListener('pointerleave', handlePointerCancel);
  controls?.dispose();
    renderer?.dispose();
    for (const mesh of cameraMeshes.values()) {
      disposeObject(mesh);
    }
    cameraMeshes.clear();
    robotGroup = null;
    cameraGroup = null;
    scene = null;
    orbitCamera = null;
    renderer = null;
    controls = null;
  }

  function disposeObject(object: THREE.Object3D | null | undefined) {
    if (!object) return;
    object.traverse((node) => {
      if (!(node instanceof THREE.Mesh)) return;
      node.geometry?.dispose?.();
      if (Array.isArray(node.material)) {
        node.material.forEach((material) => material.dispose());
      } else {
        node.material?.dispose?.();
      }
      const texture = node.userData?.texture as THREE.Texture | undefined;
      texture?.dispose?.();
    });
  }

  function robotEqual(a: RobotDimensions, b: RobotDimensions): boolean {
    return (
      Math.abs(a.width - b.width) < 1e-6 &&
      Math.abs(a.length - b.length) < 1e-6 &&
      Math.abs(a.bumperHeight - b.bumperHeight) < 1e-6 &&
      Math.abs(a.bumperThickness - b.bumperThickness) < 1e-6 &&
      Math.abs(a.groundClearance - b.groundClearance) < 1e-6
    );
  }

  function camerasEqual(a: RigCameraInfo[], b: RigCameraInfo[]): boolean {
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
  }

  function normalizeRobot(value: unknown): RobotDimensions {
    if (value && typeof value === 'object') {
      const input = value as Partial<RobotDimensions>;
      return {
        width: toNumber(input.width, DEFAULT_ROBOT_DIMENSIONS.width),
        length: toNumber(input.length, DEFAULT_ROBOT_DIMENSIONS.length),
        bumperHeight: toNumber(input.bumperHeight, DEFAULT_ROBOT_DIMENSIONS.bumperHeight),
        bumperThickness: toNumber(input.bumperThickness, DEFAULT_ROBOT_DIMENSIONS.bumperThickness),
        groundClearance: toNumber(input.groundClearance, DEFAULT_ROBOT_DIMENSIONS.groundClearance)
      };
    }
    return { ...DEFAULT_ROBOT_DIMENSIONS };
  }

  function normalizeCameras(value: unknown): RigCameraInfo[] {
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
  }

  function toNumber(value: unknown, fallback: number): number {
    if (typeof value === 'number' && Number.isFinite(value)) return value;
    if (typeof value === 'string') {
      const parsed = Number(value);
      if (Number.isFinite(parsed)) return parsed;
    }
    return fallback;
  }

  $effect(() => {
    const next = normalizeRobot(robot);
    if (!robotEqual(robotDimensions, next)) {
      robotDimensions = next;
      rebuildRobot();
    }
  });

  $effect(() => {
    const next = normalizeCameras(cameras);
    if (!camerasEqual(cameraEntries, next)) {
      cameraEntries = next;
      rebuildCameras();
    }
  });

  $effect(() => {
    const nextSelected = selectedCamera ?? null;
    if (selected !== nextSelected) {
      selected = nextSelected;
      updateSelectionHighlight();
    }
  });

</script>

<canvas
  class="block h-full w-full min-h-0 overflow-hidden rounded-xl border-0 bg-transparent"
  bind:this={canvas}
  aria-label="Camera layout preview"
></canvas>
