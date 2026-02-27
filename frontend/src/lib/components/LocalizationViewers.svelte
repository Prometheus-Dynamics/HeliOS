<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount, type Snippet } from 'svelte';
  import * as THREE from 'three';
  import LocalizationMinimap from '$lib/features/localization/viewers/LocalizationMinimap.svelte';
  import LocalizationScene from '$lib/features/localization/viewers/LocalizationScene.svelte';
  import {
    Line2,
    LineGeometry,
    LineMaterial,
    OrbitControls
  } from 'three-stdlib';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
  import {
    BUMPER_THICKNESS_M,
    DEFAULT_ROBOT_LENGTH_M,
    DEFAULT_ROBOT_WIDTH_M,
    GROUND_CLEARANCE_M,
    ROBOT_HEIGHT_M,
    type LocalizationFieldDefinition,
    type LocalizationMarker,
    type LocalizationViewerProps
  } from '$lib/features/localization/viewers/localizationViewerTypes';
  import {
    buildEnvironment,
    buildRobotBumpers,
    camerasEqual,
    createAxesHelper,
    dimensionsEqual,
    disposeObject,
    getActiveField,
    getMainCameraView,
    getTopCameraBounds,
    normalizeCameras,
    normalizeRobot,
    rebuildCameraRig,
    syncRobotOverlays
  } from '$lib/features/localization/viewers/localizationSceneLifecycle';
  import {
    buildMarker,
    updateMarkerTransform
  } from '$lib/features/localization/viewers/localizationViewerMarkers';

  const {
    markers = [],
    tagLineMarkers = [],
    referenceMarkers = [],
    mode = 'isolated',
    showOriginAxes = true,
    showTagLines = false,
    showFieldImage = true,
    showMinimapTrail = true,
    bumperNumber = '0000',
    bumperColor = null,
    robotOverlays = [],
    robot = {
      width: DEFAULT_ROBOT_WIDTH_M,
      length: DEFAULT_ROBOT_LENGTH_M,
      bumperThickness: BUMPER_THICKNESS_M,
      bumperHeight: ROBOT_HEIGHT_M,
      groundClearance: GROUND_CLEARANCE_M
    },
    cameras = [],
    customField = null,
    showRobot = true,
    showCameras = true,
    cameraGhostActive = false,
    sceneTransform = null,
    robotTransform = null,
    cameraTransforms = null,
    cameraHighlightColor = null,
    minimapPoseDot = null,
    cameraPovEnabled = false,
    cameraPovTransform = null,
    cameraPovIntrinsics = null,
    cameraPovApplyFov = true,
    cameraPovForwardSign = 1,
    robotFollowPovEnabled = false,
    footerStatus,
    minimapControls,
    metricsActive = false,
    onMetricsToggle
  }: LocalizationViewerProps & {
    robot?: RobotDimensions;
    cameras?: RigCameraInfo[];
    footerStatus?: Snippet;
    minimapControls?: Snippet;
    metricsActive?: boolean;
    onMetricsToggle?: (() => void) | null;
  } = $props();

  let mainCanvas = $state<HTMLCanvasElement | null>(null);
  let topCanvas = $state<HTMLCanvasElement | null>(null);
  let mainContainer = $state<HTMLDivElement | null>(null);
  let topContainer = $state<HTMLDivElement | null>(null);

  let scene: THREE.Scene;
  let mainCamera: THREE.PerspectiveCamera;
  let topCamera: THREE.OrthographicCamera;
  let rendererMain: THREE.WebGLRenderer;
  let rendererTop: THREE.WebGLRenderer;
  let controls: OrbitControls;
  let rootGroup: THREE.Group;
  let originIndicatorGroup: THREE.Group;
  let markerGroup: THREE.Group;
  let tagLineGroup: THREE.Group;
  let referenceMarkerGroup: THREE.Group;
  let environmentGroup: THREE.Group;
  let robotGroup: THREE.Group;
  let robotOverlayGroup: THREE.Group;
  let cameraRigGroup: THREE.Group;
  let minimapTrailGroup: THREE.Group;
  let minimapTrailMaterial: LineMaterial | null = null;
  let minimapTrailLineGeometry: LineGeometry | null = null;
  let minimapTrailLine: Line2 | null = null;
  let minimapTrailSegmentCount = 0;
  let minimapPoseDotGroup: THREE.Group;
  let minimapPoseDotOuterMaterial: THREE.MeshBasicMaterial | null = null;
  let minimapPoseDotInnerMaterial: THREE.MeshBasicMaterial | null = null;
  let animationFrame: number | null = null;
  let minimapExpanded = $state(false);
  let robotDimensions = $state<RobotDimensions>(normalizeRobot(robot));
  let cameraLayout = $state<RigCameraInfo[]>(normalizeCameras(cameras));
  const cameraMeshes = new Map<string, THREE.Group>();
  const robotOverlayMeshes = new Map<string, THREE.Group>();
  const markerMeshes = new Map<string, THREE.Group>();
  const referenceMarkerMeshes = new Map<string, THREE.Group>();
  const tagLineMeshes = new Map<string, THREE.Line>();
  const activeField = $derived.by<LocalizationFieldDefinition | null>(() => getActiveField(mode, customField));

  const resizeObservers: ResizeObserver[] = [];
  let lastMainWidth = 0;
  let lastMainHeight = 0;
  let lastTopWidth = 0;
  let lastTopHeight = 0;
  let lastPixelRatio = 0;

  const markerColors = {
    tracking: '#38bdf8',
    idle: '#f97316',
    offline: '#f43f5e'
  } as const;
  const MINIMAP_POSE_DOT_OUTER_RADIUS_M = 0.24;
  const MINIMAP_POSE_DOT_INNER_RADIUS_M = 0.16;
  const MINIMAP_TRAIL_LINE_WIDTH_M = MINIMAP_POSE_DOT_INNER_RADIUS_M;
  const MINIMAP_TRAIL_WINDOW_MS = 5000;
  const MINIMAP_TRAIL_MAX_POINTS = 1200;
  const MINIMAP_TRAIL_DUPLICATE_DISTANCE_M = 0.0001;
  const ROBOT_FOLLOW_CLOSE_DISTANCE_M = 0.92;
  const ROBOT_FOLLOW_CLOSE_HEIGHT_M = 0.52;
  const ROBOT_FOLLOW_CLOSE_SIDE_M = 0.16;
  const ROBOT_FOLLOW_TARGET_UP_M = 0.2;
  const minimapTrailPoints: Array<{ point: THREE.Vector3; timestampMs: number }> = [];
  let lastKnownMinimapPoseDot: { position: [number, number, number]; color: string } | null = null;
  let lastKnownRobotTransform: LocalizationViewerProps['robotTransform'] = null;
  let robotGhostActive = false;
  const arucoTextureCache = new Map<string, THREE.Texture>();
  const cameraForward = new THREE.Vector3(0, 0, 1);
  const cameraUp = new THREE.Vector3(0, 1, 0);
  const DEFAULT_MAIN_FOV = 55;
  let povWasActive = false;
  let robotFollowWasActive = false;
  let robotFollowUserInteracting = false;
  let lastResolvedCameraPovTransform: LocalizationViewerProps['cameraPovTransform'] = null;
  let lastResolvedCameraPovIntrinsics: LocalizationViewerProps['cameraPovIntrinsics'] = null;
  let lastRobotFollowWorldPose: { position: THREE.Vector3; quaternion: THREE.Quaternion } | null = null;
  let savedMainCameraState:
    | {
        position: THREE.Vector3;
        quaternion: THREE.Quaternion;
        up: THREE.Vector3;
        target: THREE.Vector3;
        fov: number;
      }
    | null = null;

  function captureMainCameraState() {
    if (!mainCamera || !controls) return;
    savedMainCameraState = {
      position: mainCamera.position.clone(),
      quaternion: mainCamera.quaternion.clone(),
      up: mainCamera.up.clone(),
      target: controls.target.clone(),
      fov: mainCamera.fov
    };
  }

  function restoreMainCameraState() {
    if (!mainCamera || !controls || !savedMainCameraState) return;
    mainCamera.position.copy(savedMainCameraState.position);
    mainCamera.quaternion.copy(savedMainCameraState.quaternion);
    mainCamera.up.copy(savedMainCameraState.up);
    mainCamera.fov = savedMainCameraState.fov;
    mainCamera.clearViewOffset();
    mainCamera.updateProjectionMatrix();
    controls.target.copy(savedMainCameraState.target);
    controls.update();
    savedMainCameraState = null;
  }

  function applyMainCameraPov() {
    if (!mainCamera) return;
    const transform = cameraPovTransform ?? lastResolvedCameraPovTransform;
    if (!transform) {
      if (!cameraPovEnabled) {
        mainCamera.clearViewOffset();
        if (Math.abs(mainCamera.fov - DEFAULT_MAIN_FOV) > 1e-6) {
          mainCamera.fov = DEFAULT_MAIN_FOV;
          mainCamera.updateProjectionMatrix();
        }
      }
      return;
    }

    const localPos = new THREE.Vector3(...transform.position);
    const worldPos = rootGroup ? rootGroup.localToWorld(localPos.clone()) : localPos;
    const localQuat = transform.quaternion
      ? new THREE.Quaternion(
          transform.quaternion.x,
          transform.quaternion.y,
          transform.quaternion.z,
          transform.quaternion.w
        ).normalize()
      : new THREE.Quaternion();
    const worldQuat = localQuat.clone();
    if (rootGroup) {
      const rootQuat = new THREE.Quaternion();
      rootGroup.getWorldQuaternion(rootQuat);
      worldQuat.premultiply(rootQuat).normalize();
    }

    const forwardSign = cameraPovForwardSign === -1 ? -1 : 1;
    const forward = cameraForward.clone().multiplyScalar(forwardSign).applyQuaternion(worldQuat).normalize();
    const up = cameraUp.clone().applyQuaternion(worldQuat).normalize();
    const lookAt = worldPos.clone().add(forward);

    mainCamera.position.copy(worldPos);
    mainCamera.up.copy(up);
    mainCamera.lookAt(lookAt);

    if (!cameraPovApplyFov) {
      mainCamera.clearViewOffset();
    } else {
      const intrinsics = cameraPovIntrinsics ?? lastResolvedCameraPovIntrinsics;
      if (
        intrinsics &&
        Number.isFinite(intrinsics.fx) &&
        Number.isFinite(intrinsics.fy) &&
        intrinsics.fx > 0 &&
        intrinsics.fy > 0 &&
        Number.isFinite(intrinsics.width) &&
        Number.isFinite(intrinsics.height) &&
        intrinsics.width > 0 &&
        intrinsics.height > 0
      ) {
        const vfov = 2 * Math.atan(intrinsics.height / (2 * intrinsics.fy)) * (180 / Math.PI);
        if (Number.isFinite(vfov) && vfov > 1 && vfov < 179) {
          mainCamera.fov = vfov;
        }
        const fullWidth = Math.max(1, Math.round(intrinsics.width));
        const fullHeight = Math.max(1, Math.round(intrinsics.height));
        const cxOffsetPx = intrinsics.cx - intrinsics.width * 0.5;
        const cyOffsetPx = intrinsics.cy - intrinsics.height * 0.5;
        const maxOffsetX = fullWidth * 0.45;
        const maxOffsetY = fullHeight * 0.45;
        const offsetX = Math.round(THREE.MathUtils.clamp(cxOffsetPx, -maxOffsetX, maxOffsetX));
        const offsetY = Math.round(THREE.MathUtils.clamp(cyOffsetPx, -maxOffsetY, maxOffsetY));
        if (offsetX !== 0 || offsetY !== 0) {
          mainCamera.setViewOffset(fullWidth, fullHeight, -offsetX, -offsetY, fullWidth, fullHeight);
        } else {
          mainCamera.clearViewOffset();
        }
        mainCamera.updateProjectionMatrix();
      } else {
        mainCamera.clearViewOffset();
        if (Math.abs(mainCamera.fov - DEFAULT_MAIN_FOV) > 1e-6) {
          mainCamera.fov = DEFAULT_MAIN_FOV;
          mainCamera.updateProjectionMatrix();
        }
      }
    }

    if (controls) {
      controls.target.copy(lookAt);
    }
  }

  function resolveWorldRobotPose(
    transform: LocalizationViewerProps['robotTransform'] | null | undefined
  ): { position: THREE.Vector3; quaternion: THREE.Quaternion } | null {
    if (!transform) return null;
    const localPos = new THREE.Vector3(...transform.position);
    const localQuat = transform.quaternion
      ? new THREE.Quaternion(
          transform.quaternion.x,
          transform.quaternion.y,
          transform.quaternion.z,
          transform.quaternion.w
        ).normalize()
      : new THREE.Quaternion();

    const worldPos = rootGroup ? rootGroup.localToWorld(localPos.clone()) : localPos;
    const worldQuat = localQuat.clone();
    if (rootGroup) {
      const rootQuat = new THREE.Quaternion();
      rootGroup.getWorldQuaternion(rootQuat);
      worldQuat.premultiply(rootQuat).normalize();
    }
    return { position: worldPos, quaternion: worldQuat };
  }

  function applyRobotFollowPovDelta() {
    if (!mainCamera || !controls) return;
    if (cameraPovEnabled || !robotFollowPovEnabled) {
      lastRobotFollowWorldPose = null;
      return;
    }

    const current = resolveWorldRobotPose(robotTransform ?? lastKnownRobotTransform);
    if (!current) {
      lastRobotFollowWorldPose = null;
      return;
    }

    const previous = lastRobotFollowWorldPose;
    if (!previous) {
      const distance = Math.max(ROBOT_FOLLOW_CLOSE_DISTANCE_M, robotDimensions.length * 0.5);
      const height = Math.max(ROBOT_FOLLOW_CLOSE_HEIGHT_M, robotDimensions.bumperHeight * 0.9);
      const side = Math.max(ROBOT_FOLLOW_CLOSE_SIDE_M, robotDimensions.width * 0.1);
      const targetLift = Math.max(ROBOT_FOLLOW_TARGET_UP_M, robotDimensions.bumperHeight * 0.35);
      const forward = new THREE.Vector3(0, 0, 1).applyQuaternion(current.quaternion).normalize();
      const up = new THREE.Vector3(0, 1, 0).applyQuaternion(current.quaternion).normalize();
      const right = new THREE.Vector3(1, 0, 0).applyQuaternion(current.quaternion).normalize();
      const lookAhead = Math.max(0.4, distance * 0.8);
      const cameraPos = current.position
        .clone()
        .add(right.multiplyScalar(side))
        .add(up.clone().multiplyScalar(height))
        .add(forward.clone().multiplyScalar(-distance));
      const targetPos = current.position
        .clone()
        .add(up.multiplyScalar(targetLift))
        .add(forward.multiplyScalar(lookAhead));
      controls.target.copy(targetPos);
      mainCamera.position.copy(cameraPos);
      lastRobotFollowWorldPose = {
        position: current.position.clone(),
        quaternion: current.quaternion.clone()
      };
      return;
    }

    const translationDelta = current.position.clone().sub(previous.position);
    if (robotFollowUserInteracting) {
      mainCamera.position.add(translationDelta);
      controls.target.add(translationDelta);
      lastRobotFollowWorldPose = {
        position: current.position.clone(),
        quaternion: current.quaternion.clone()
      };
      return;
    }

    const previousInverseQuat = previous.quaternion.clone().invert();
    const mapPoint = (point: THREE.Vector3): THREE.Vector3 => {
      const local = point.clone().sub(previous.position).applyQuaternion(previousInverseQuat);
      return local.applyQuaternion(current.quaternion).add(current.position);
    };

    mainCamera.position.copy(mapPoint(mainCamera.position));
    controls.target.copy(mapPoint(controls.target));
    lastRobotFollowWorldPose = {
      position: current.position.clone(),
      quaternion: current.quaternion.clone()
    };
  }

  function clearMinimapTrail() {
    minimapTrailPoints.length = 0;
    minimapTrailSegmentCount = 0;
    syncMinimapTrailGeometry();
  }

  function pruneMinimapTrail(nowMs: number) {
    const cutoff = nowMs - MINIMAP_TRAIL_WINDOW_MS;
    let removeCount = 0;
    for (const entry of minimapTrailPoints) {
      if (entry.timestampMs >= cutoff) break;
      removeCount += 1;
    }
    if (removeCount > 0) {
      minimapTrailPoints.splice(0, removeCount);
    }
    if (minimapTrailPoints.length > MINIMAP_TRAIL_MAX_POINTS) {
      minimapTrailPoints.splice(0, minimapTrailPoints.length - MINIMAP_TRAIL_MAX_POINTS);
    }
  }

  function syncMinimapTrailGeometry() {
    if (!minimapTrailLineGeometry || !minimapTrailLine) return;
    const count = Math.min(minimapTrailPoints.length, MINIMAP_TRAIL_MAX_POINTS);
    if (count < 2) {
      minimapTrailSegmentCount = 0;
      minimapTrailLineGeometry.setPositions([0, 0, 0, 0, 0, 0]);
      return;
    }
    const positions: number[] = [];
    for (let i = 0; i < count; i += 1) {
      const entry = minimapTrailPoints[i];
      if (!entry) continue;
      positions.push(entry.point.x, entry.point.y, entry.point.z);
    }
    if (positions.length < 6) {
      minimapTrailSegmentCount = 0;
      minimapTrailLineGeometry.setPositions([0, 0, 0, 0, 0, 0]);
      return;
    }
    minimapTrailSegmentCount = Math.max(0, positions.length / 3 - 1);
    minimapTrailLineGeometry.setPositions(positions);
    minimapTrailLineGeometry.computeBoundingSphere();
  }

  function pushMinimapTrailPoint(point: THREE.Vector3, nowMs = performance.now()) {
    pruneMinimapTrail(nowMs);
    const last = minimapTrailPoints[minimapTrailPoints.length - 1] ?? null;
    if (last) {
      const dist2 = last.point.distanceToSquared(point);
      const duplicateDist2 = MINIMAP_TRAIL_DUPLICATE_DISTANCE_M * MINIMAP_TRAIL_DUPLICATE_DISTANCE_M;
      if (dist2 <= duplicateDist2) {
        // Treat numerically identical poses as the same sample.
        return;
      }
    }

    minimapTrailPoints.push({ point: point.clone(), timestampMs: nowMs });
    pruneMinimapTrail(nowMs);

    syncMinimapTrailGeometry();
  }

  function setRobotGhostVisual(ghost: boolean) {
    if (!robotGroup) return;
    robotGhostActive = ghost;
    robotGroup.traverse((node) => {
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
          mat.opacity = ghost ? Math.max(0.24, baseOpacity * 0.6) : baseOpacity;
        }
        mat.transparent = ghost || Boolean(mat.transparent);
        mat.depthWrite = !ghost;
        mat.needsUpdate = true;
      }
    });
  }

  onMount(() => {
    initScene();
    observeResizes();
    updateMarkers();
    animate();

    return () => {
      stopAnimation();
      controls?.dispose();
      rendererMain?.dispose();
      rendererTop?.dispose();
      for (const mesh of cameraMeshes.values()) {
        disposeObject(mesh);
      }
      cameraMeshes.clear();
      for (const overlay of robotOverlayMeshes.values()) {
        disposeObject(overlay);
      }
      robotOverlayMeshes.clear();
      for (const marker of markerMeshes.values()) {
        disposeObject(marker);
      }
      markerMeshes.clear();
      for (const marker of referenceMarkerMeshes.values()) {
        disposeObject(marker);
      }
      referenceMarkerMeshes.clear();
      for (const line of tagLineMeshes.values()) {
        disposeObject(line);
      }
      tagLineMeshes.clear();
      if (minimapTrailGroup) {
        disposeObject(minimapTrailGroup);
      }
      minimapTrailMaterial = null;
      minimapTrailLineGeometry = null;
      minimapTrailLine = null;
      minimapTrailSegmentCount = 0;
      minimapTrailPoints.length = 0;
      lastKnownMinimapPoseDot = null;
      lastKnownRobotTransform = null;
      lastRobotFollowWorldPose = null;
      robotFollowWasActive = false;
      robotFollowUserInteracting = false;
      robotGhostActive = false;
      if (minimapPoseDotGroup) {
        disposeObject(minimapPoseDotGroup);
      }
      minimapPoseDotOuterMaterial = null;
      minimapPoseDotInnerMaterial = null;
      resizeObservers.forEach((observer) => observer.disconnect());
    };
  });

  onDestroy(() => {
    stopAnimation();
  });

  $effect(() => {
    buildEnvironment({ environmentGroup, viewMode: mode, customField, showFieldImage });
    if (controls) {
      const mainCameraView = getMainCameraView(mode, customField);
      controls.minDistance = mainCameraView.minDistance;
      controls.maxDistance = mainCameraView.maxDistance;
    }
    updateRendererSizes();
  });

  $effect(() => {
    if (!controls || !mainCamera) return;
    const povActive = cameraPovEnabled;
    if (povActive && !povWasActive) {
      captureMainCameraState();
    } else if (!povActive && povWasActive) {
      restoreMainCameraState();
    }
    controls.enabled = !povActive;
    if (!povActive) {
      mainCamera.clearViewOffset();
    }
    povWasActive = povActive;
  });

  $effect(() => {
    if (!controls || !mainCamera) return;
    const followActive = robotFollowPovEnabled && !cameraPovEnabled;
    if (followActive && !robotFollowWasActive) {
      lastRobotFollowWorldPose = null;
      applyRobotFollowPovDelta();
      controls.update();
    } else if (!followActive && robotFollowWasActive) {
      lastRobotFollowWorldPose = null;
    }
    robotFollowWasActive = followActive;
  });

  $effect(() => {
    if (!cameraPovEnabled) {
      lastResolvedCameraPovTransform = null;
      lastResolvedCameraPovIntrinsics = null;
      return;
    }
    if (cameraPovTransform) {
      lastResolvedCameraPovTransform = cameraPovTransform;
    }
    if (cameraPovApplyFov && cameraPovIntrinsics) {
      lastResolvedCameraPovIntrinsics = cameraPovIntrinsics;
    } else if (!cameraPovApplyFov) {
      lastResolvedCameraPovIntrinsics = null;
    }
  });

  $effect(() => {
    void cameraPovEnabled;
    void cameraPovTransform;
    void cameraPovIntrinsics;
    void cameraPovApplyFov;
    void cameraPovForwardSign;
    applyMainCameraPov();
  });

  $effect(() => {
    updateMarkers();
  });

  $effect(() => {
    void markers;
    void tagLineMarkers;
    void showTagLines;
    void cameraTransforms;
    void cameraLayout;
    updateTagLines();
  });

  $effect(() => {
    updateReferenceMarkers();
  });

  $effect(() => {
    void bumperNumber;
    if (!robotGroup) return;
    buildRobotBumpers({ browser, robotGroup, robotDimensions, bumperColor, showRobot });
    setRobotGhostVisual(robotGhostActive);
  });

  $effect(() => {
    void bumperColor;
    if (!robotGroup) return;
    buildRobotBumpers({ browser, robotGroup, robotDimensions, bumperColor, showRobot });
    setRobotGhostVisual(robotGhostActive);
  });

  $effect(() => {
    if (!rootGroup) return;
    const transform = sceneTransform;
    if (!transform) {
      rootGroup.position.set(0, 0, 0);
      rootGroup.quaternion.identity();
      applyTopCameraCenterBias();
      return;
    }
    rootGroup.position.set(...transform.position);
    if (transform.quaternion) {
      rootGroup.quaternion.set(
        transform.quaternion.x,
        transform.quaternion.y,
        transform.quaternion.z,
        transform.quaternion.w
      );
      rootGroup.quaternion.normalize();
    } else {
      rootGroup.quaternion.identity();
    }
    applyTopCameraCenterBias();
  });

  $effect(() => {
    if (!originIndicatorGroup) return;
    originIndicatorGroup.visible = Boolean(sceneTransform) && showOriginAxes;
  });

	  $effect(() => {
	    if (!robotGroup) return;
	    const transform = robotTransform;
      if (transform) {
        lastKnownRobotTransform = transform;
        robotGroup.position.set(...transform.position);
        if (transform.quaternion) {
          robotGroup.quaternion.set(
            transform.quaternion.x,
            transform.quaternion.y,
            transform.quaternion.z,
            transform.quaternion.w
          );
          robotGroup.quaternion.normalize();
        } else {
          robotGroup.quaternion.identity();
        }
        setRobotGhostVisual(false);
        return;
      }

      const ghost = lastKnownRobotTransform;
      if (!ghost) {
        robotGroup.position.set(0, 0, 0);
        robotGroup.quaternion.identity();
        setRobotGhostVisual(false);
        return;
      }
      robotGroup.position.set(...ghost.position);
      if (ghost.quaternion) {
	      robotGroup.quaternion.set(
	        ghost.quaternion.x,
	        ghost.quaternion.y,
	        ghost.quaternion.z,
	        ghost.quaternion.w
	      );
	      robotGroup.quaternion.normalize();
	    } else {
	      robotGroup.quaternion.identity();
	    }
      setRobotGhostVisual(true);
	  });

  $effect(() => {
    const overlays = Array.isArray(robotOverlays) ? robotOverlays : [];
    if (robotGroup) {
      robotGroup.visible = robotGhostActive || (showRobot && overlays.length === 0);
    }
  });

  $effect(() => {
    if (showMinimapTrail) return;
    if (minimapTrailPoints.length > 0) {
      clearMinimapTrail();
    }
    if (minimapTrailGroup) {
      minimapTrailGroup.visible = false;
    }
  });

  function updateMinimapPoseAndTrail() {
    if (
      !minimapPoseDotGroup ||
      !minimapPoseDotOuterMaterial ||
      !minimapPoseDotInnerMaterial ||
      !minimapTrailGroup
    ) {
      return;
    }
    const dot = minimapPoseDot;
    const trailEnabled = showMinimapTrail;
    const nowMs = performance.now();
    pruneMinimapTrail(nowMs);
    syncMinimapTrailGeometry();

    if (!dot) {
      const ghost = lastKnownMinimapPoseDot;
      if (ghost) {
        minimapPoseDotGroup.visible = true;
        minimapPoseDotGroup.position.set(...ghost.position);
        minimapPoseDotOuterMaterial.opacity = 0.55;
        minimapPoseDotInnerMaterial.opacity = 0.42;
        minimapPoseDotInnerMaterial.color.set(ghost.color);
        if (trailEnabled) {
          if (minimapTrailMaterial) {
            minimapTrailMaterial.color.set(ghost.color);
            minimapTrailMaterial.opacity = 1;
          }
        }
      } else {
        minimapPoseDotGroup.visible = false;
      }
      minimapTrailGroup.visible = trailEnabled && minimapTrailSegmentCount > 0;
      return;
    }

    const [x, y, z] = dot.position;
    if (![x, y, z].every((value) => Number.isFinite(value))) {
      const ghost = lastKnownMinimapPoseDot;
      if (ghost) {
        minimapPoseDotGroup.visible = true;
        minimapPoseDotGroup.position.set(...ghost.position);
        minimapPoseDotOuterMaterial.opacity = 0.55;
        minimapPoseDotInnerMaterial.opacity = 0.42;
        minimapPoseDotInnerMaterial.color.set(ghost.color);
        if (trailEnabled) {
          if (minimapTrailMaterial) {
            minimapTrailMaterial.color.set(ghost.color);
            minimapTrailMaterial.opacity = 1;
          }
        }
      } else {
        minimapPoseDotGroup.visible = false;
      }
      minimapTrailGroup.visible = trailEnabled && minimapTrailSegmentCount > 0;
      return;
    }

    minimapPoseDotGroup.visible = true;
    minimapPoseDotGroup.position.set(x, y, z);
    const color = dot.color ?? '#38bdf8';
    lastKnownMinimapPoseDot = { position: [x, y, z], color };
    minimapPoseDotOuterMaterial.opacity = 0.95;
    minimapPoseDotInnerMaterial.opacity = 1;
    minimapPoseDotInnerMaterial.color.set(color);

    if (trailEnabled) {
      pushMinimapTrailPoint(new THREE.Vector3(x, y, z), nowMs);
      if (minimapTrailMaterial) {
        minimapTrailMaterial.color.set(color);
        minimapTrailMaterial.opacity = 1;
      }
      minimapTrailGroup.visible = minimapTrailSegmentCount > 0;
    } else {
      minimapTrailGroup.visible = false;
    }
  }

  function applyTopCameraCenterBias() {
    if (!topCamera || !rootGroup) return;
    const centerX = rootGroup.position.x;
    const centerY = rootGroup.position.y;
    const centerZ = rootGroup.position.z;
    topCamera.position.set(centerX, topCamera.position.y, centerZ);
    topCamera.lookAt(centerX, centerY, centerZ);
  }

  function initScene() {
    if (!mainCanvas || !topCanvas) return;
    scene = new THREE.Scene();
    scene.background = new THREE.Color(0x04070f);

    const hemi = new THREE.HemisphereLight(0x6c8cff, 0x060a14, 0.8);
    scene.add(hemi);

    const dirLight = new THREE.DirectionalLight(0xfff1c1, 0.65);
    dirLight.position.set(12, 18, 8);
    dirLight.castShadow = true;
    dirLight.shadow.mapSize.set(1024, 1024);
    scene.add(dirLight);

    originIndicatorGroup = new THREE.Group();
    originIndicatorGroup.visible = false;
    const originAxes = createAxesHelper(5.5);
    const originAxesMaterial = originAxes.material as THREE.LineBasicMaterial;
    originAxesMaterial.opacity = 0.95;
    originAxes.position.y = 0.03;
    originIndicatorGroup.add(originAxes);
    const originCore = new THREE.Mesh(
      new THREE.SphereGeometry(0.14, 24, 16),
      new THREE.MeshBasicMaterial({
        color: 0xffffff,
        transparent: true,
        opacity: 0.95,
        depthTest: false,
        depthWrite: false
      })
    );
    originCore.position.y = 0.04;
    originCore.renderOrder = 3;
    originIndicatorGroup.add(originCore);
    const originHalo = new THREE.Mesh(
      new THREE.TorusGeometry(0.34, 0.03, 10, 52),
      new THREE.MeshBasicMaterial({
        color: 0xffffff,
        transparent: true,
        opacity: 0.78,
        depthTest: false,
        depthWrite: false
      })
    );
    originHalo.rotation.x = Math.PI / 2;
    originHalo.position.y = 0.02;
    originHalo.renderOrder = 3;
    originIndicatorGroup.add(originHalo);
    scene.add(originIndicatorGroup);

    rootGroup = new THREE.Group();
    scene.add(rootGroup);

    environmentGroup = new THREE.Group();
    rootGroup.add(environmentGroup);
    buildEnvironment({ environmentGroup, viewMode: mode, customField, showFieldImage });

    referenceMarkerGroup = new THREE.Group();
    rootGroup.add(referenceMarkerGroup);

    markerGroup = new THREE.Group();
    rootGroup.add(markerGroup);
    tagLineGroup = new THREE.Group();
    rootGroup.add(tagLineGroup);
    robotGroup = new THREE.Group();
    rootGroup.add(robotGroup);
    robotOverlayGroup = new THREE.Group();
    rootGroup.add(robotOverlayGroup);
    buildRobotBumpers({ browser, robotGroup, robotDimensions, bumperColor, showRobot });
    cameraRigGroup = new THREE.Group();
    rootGroup.add(cameraRigGroup);
    rebuildCameraRig({
      browser,
      cameraRigGroup,
      cameraMeshes,
      cameraLayout,
      showCameras,
      cameraTransforms,
      cameraHighlightColor,
      cameraGhostActive
    });

    minimapTrailGroup = new THREE.Group();
    minimapTrailGroup.visible = false;
    const minimapTrailGeometry = new LineGeometry();
    minimapTrailGeometry.setPositions([0, 0, 0, 0, 0, 0]);
    const minimapTrailLineMaterial = new LineMaterial({
      color: 0x38bdf8,
      transparent: true,
      opacity: 0.95,
      depthTest: false,
      depthWrite: false,
      worldUnits: true,
      linewidth: MINIMAP_TRAIL_LINE_WIDTH_M
    });
    minimapTrailLineMaterial.resolution.set(1, 1);
    const minimapTrail = new Line2(minimapTrailGeometry, minimapTrailLineMaterial);
    minimapTrail.frustumCulled = false;
    minimapTrail.renderOrder = 998;
    minimapTrailGroup.add(minimapTrail);
    setMinimapOnlyLayer(minimapTrailGroup);
    rootGroup.add(minimapTrailGroup);
    minimapTrailMaterial = minimapTrailLineMaterial;
    minimapTrailLineGeometry = minimapTrailGeometry;
    minimapTrailLine = minimapTrail;

    minimapPoseDotGroup = new THREE.Group();
    minimapPoseDotGroup.visible = false;
    const outerMaterial = new THREE.MeshBasicMaterial({
      color: 0x020617,
      transparent: true,
      opacity: 0.95,
      depthTest: false,
      depthWrite: false
    });
    const outerDot = new THREE.Mesh(
      new THREE.CircleGeometry(MINIMAP_POSE_DOT_OUTER_RADIUS_M, 40),
      outerMaterial
    );
    const innerMaterial = new THREE.MeshBasicMaterial({
      color: 0x38bdf8,
      transparent: true,
      opacity: 1,
      depthTest: false,
      depthWrite: false
    });
    const innerDot = new THREE.Mesh(
      new THREE.CircleGeometry(MINIMAP_POSE_DOT_INNER_RADIUS_M, 40),
      innerMaterial
    );
    outerDot.rotation.x = -Math.PI / 2;
    innerDot.rotation.x = -Math.PI / 2;
    innerDot.position.y = 0.001;
    outerDot.renderOrder = 1000;
    innerDot.renderOrder = 1001;
    minimapPoseDotGroup.add(outerDot, innerDot);
    setMinimapOnlyLayer(minimapPoseDotGroup);
    rootGroup.add(minimapPoseDotGroup);
    minimapPoseDotOuterMaterial = outerMaterial;
    minimapPoseDotInnerMaterial = innerMaterial;

    const mainCameraView = getMainCameraView(mode, customField);
    mainCamera = new THREE.PerspectiveCamera(55, 1, 0.1, 200);
    mainCamera.position.set(...mainCameraView.position);
    mainCamera.lookAt(...mainCameraView.target);
    mainCamera.layers.enable(0);
    mainCamera.layers.disable(1);

    topCamera = new THREE.OrthographicCamera(-20, 20, 20, -20, 0.1, 200);
    topCamera.position.set(0, 40, 0);
    topCamera.up.set(0, 0, -1);
    topCamera.lookAt(0, 0, 0);
    topCamera.layers.enable(1);
    applyTopCameraCenterBias();

    rendererMain = new THREE.WebGLRenderer({ canvas: mainCanvas, antialias: true });
    rendererMain.shadowMap.enabled = true;
    rendererMain.outputColorSpace = THREE.SRGBColorSpace;

    rendererTop = new THREE.WebGLRenderer({ canvas: topCanvas, antialias: true });
    rendererTop.outputColorSpace = THREE.SRGBColorSpace;
    rendererTop.setClearColor(new THREE.Color(0x03060c), 1);

    controls = new OrbitControls(mainCamera, mainCanvas);
    controls.enableDamping = true;
    controls.minDistance = mainCameraView.minDistance;
    controls.maxDistance = mainCameraView.maxDistance;
    controls.minPolarAngle = 0.01;
    controls.maxPolarAngle = Math.PI - 0.01;
    controls.target.set(...mainCameraView.target);
    controls.update();
    controls.addEventListener('start', () => {
      robotFollowUserInteracting = true;
    });
    controls.addEventListener('end', () => {
      robotFollowUserInteracting = false;
    });

    updateRendererSizes(true);
  }

  function updateRendererSizes(force = false) {
    if (!browser || !rendererMain || !rendererTop) return;
    const ratio = Math.min(window.devicePixelRatio ?? 1, 2);
    const ratioChanged = ratio !== lastPixelRatio;

    const mainWidth = mainContainer?.clientWidth ?? 0;
    const mainHeight = mainContainer?.clientHeight ?? 0;
    const mainChanged = force || ratioChanged || mainWidth !== lastMainWidth || mainHeight !== lastMainHeight;
    if (mainWidth > 0 && mainHeight > 0 && mainChanged) {
      rendererMain.setPixelRatio(ratio);
      rendererMain.setSize(mainWidth, mainHeight, false);
      mainCamera.aspect = mainWidth / mainHeight;
      mainCamera.updateProjectionMatrix();
      lastMainWidth = mainWidth;
      lastMainHeight = mainHeight;
    }

    const topWidth = topContainer?.clientWidth ?? 0;
    const topHeight = topContainer?.clientHeight ?? 0;
    const topChanged = force || ratioChanged || topWidth !== lastTopWidth || topHeight !== lastTopHeight;
    if (topWidth > 0 && topHeight > 0 && topChanged) {
      rendererTop.setPixelRatio(ratio);
      rendererTop.setSize(topWidth, topHeight, false);
      const { halfWidth, halfHeight } = getTopCameraBounds(mode, customField);
      topCamera.left = -halfWidth;
      topCamera.right = halfWidth;
      topCamera.top = halfHeight;
      topCamera.bottom = -halfHeight;
      topCamera.updateProjectionMatrix();
      if (minimapTrailMaterial) {
        minimapTrailMaterial.resolution.set(topWidth * ratio, topHeight * ratio);
      }
      lastTopWidth = topWidth;
      lastTopHeight = topHeight;
    }

    if (mainChanged || topChanged || ratioChanged) {
      lastPixelRatio = ratio;
    }
  }

  function observeResizes() {
    const elements = [mainContainer, topContainer];
    elements.forEach((element) => {
      if (!element) return;
      const observer = new ResizeObserver(() => updateRendererSizes());
      observer.observe(element);
      resizeObservers.push(observer);
    });
  }

  function updateMarkers() {
    if (!markerGroup || !markers) return;

    const nextIds = new Set<string>();
    for (const marker of markers) {
      nextIds.add(marker.id);
    }

    // Remove stale marker meshes.
    for (const [id, mesh] of markerMeshes) {
      if (nextIds.has(id)) continue;
      markerGroup.remove(mesh);
      disposeObject(mesh);
      markerMeshes.delete(id);
    }

    // Upsert markers.
    for (const marker of markers) {
      const existing = markerMeshes.get(marker.id);
      const visualKey = markerVisualKey(marker);
      if (!existing) {
        const built = buildMarker(marker, { browser, markerColors, arucoTextureCache });
        if (!built) continue;
        built.name = marker.id;
        built.userData.__visualKey = visualKey;
        markerGroup.add(built);
        markerMeshes.set(marker.id, built);
        continue;
      }
      const existingVisualKey = String(existing.userData?.__visualKey ?? '');
      if (existingVisualKey !== visualKey) {
        markerGroup.remove(existing);
        disposeObject(existing);
        const rebuilt = buildMarker(marker, { browser, markerColors, arucoTextureCache });
        if (!rebuilt) {
          markerMeshes.delete(marker.id);
          continue;
        }
        rebuilt.name = marker.id;
        rebuilt.userData.__visualKey = visualKey;
        markerGroup.add(rebuilt);
        markerMeshes.set(marker.id, rebuilt);
        continue;
      }

      updateMarkerTransform(existing, marker);
    }
  }

  function markerVisualKey(marker: LocalizationMarker): string {
    const common = [
      marker.targetType ?? 'aruco',
      marker.color ?? '',
      marker.status ?? '',
      String((marker as any).tagId ?? ''),
      String((marker as any).tagSize ?? ''),
      String((marker as any).tagHeight ?? ''),
      String((marker as any).tagBorderRatio ?? ''),
      String((marker as any).codeRotation ?? '')
    ];

    const bits = (marker as any).tagBits as { width: number; border: number; rows: string[] } | undefined;
    if (bits && Number.isFinite(bits.width) && Array.isArray(bits.rows)) {
      common.push(`bits:${bits.width}:${bits.border}:${bits.rows.join('')}`);
    } else {
      common.push('bits:');
    }

    if (marker.targetType === 'polygon') {
      const polygon = marker as any;
      const outline = Array.isArray(polygon.outline)
        ? polygon.outline
            .map((pair: [number, number]) =>
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

  function updateReferenceMarkers() {
    if (!referenceMarkerGroup || !referenceMarkers) return;

    const nextIds = new Set<string>();
    for (const marker of referenceMarkers) {
      nextIds.add(marker.id);
    }

    for (const [id, mesh] of referenceMarkerMeshes) {
      if (nextIds.has(id)) continue;
      referenceMarkerGroup.remove(mesh);
      disposeObject(mesh);
      referenceMarkerMeshes.delete(id);
    }

    for (const marker of referenceMarkers) {
      const existing = referenceMarkerMeshes.get(marker.id);
      const visualKey = markerVisualKey(marker);
      if (!existing) {
        const built = buildMarker(marker, { browser, markerColors, arucoTextureCache });
        if (!built) continue;
        built.name = marker.id;
        built.userData.__visualKey = visualKey;
        enableMinimapLayer(built);
        referenceMarkerGroup.add(built);
        referenceMarkerMeshes.set(marker.id, built);
        continue;
      }
      const existingVisualKey = String(existing.userData?.__visualKey ?? '');
      if (existingVisualKey !== visualKey) {
        referenceMarkerGroup.remove(existing);
        disposeObject(existing);
        const rebuilt = buildMarker(marker, { browser, markerColors, arucoTextureCache });
        if (!rebuilt) {
          referenceMarkerMeshes.delete(marker.id);
          continue;
        }
        rebuilt.name = marker.id;
        rebuilt.userData.__visualKey = visualKey;
        enableMinimapLayer(rebuilt);
        referenceMarkerGroup.add(rebuilt);
        referenceMarkerMeshes.set(marker.id, rebuilt);
        continue;
      }

      updateMarkerTransform(existing, marker);
    }
  }

  function cameraKeyVariants(value: string | null | undefined): string[] {
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

  function cameraLayoutKeys(camera: RigCameraInfo): string[] {
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

  function viewerPositionFromRigTranslation(
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

  function isFinitePosition(value: unknown): value is [number, number, number] {
    return (
      Array.isArray(value) &&
      value.length === 3 &&
      Number.isFinite(value[0]) &&
      Number.isFinite(value[1]) &&
      Number.isFinite(value[2])
    );
  }

  function resolveSourceCameraPosition(marker: LocalizationMarker): [number, number, number] | null {
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
      const transform = cameraTransforms?.[key];
      if (transform && isFinitePosition(transform.position)) {
        return transform.position;
      }
    }

    const matchedCamera = cameraLayout.find((camera) =>
      cameraLayoutKeys(camera).some((key) => sourceKeys.has(key))
    );
    if (matchedCamera) {
      for (const key of cameraLayoutKeys(matchedCamera)) {
        const transform = cameraTransforms?.[key];
        if (transform && isFinitePosition(transform.position)) {
          return transform.position;
        }
      }

      const fallback = viewerPositionFromRigTranslation(matchedCamera.pose?.translation);
      if (fallback) return fallback;
    }

    return null;
  }

  function updateTagLines() {
    if (!tagLineGroup) return;
    const activeMarkers = Array.isArray(tagLineMarkers) ? tagLineMarkers : markers;
    const lineMarkers = Array.isArray(activeMarkers) ? activeMarkers : [];
    const lineTargets: Array<{ id: string; marker: LocalizationMarker }> = lineMarkers.map((marker) => ({
      id: marker.id,
      marker
    }));

    if (!showTagLines || lineTargets.length === 0) {
      for (const line of tagLineMeshes.values()) {
        tagLineGroup.remove(line);
        disposeObject(line);
      }
      tagLineMeshes.clear();
      tagLineGroup.visible = false;
      return;
    }

    const nextIds = new Set<string>();
    for (const target of lineTargets) {
      const marker = target.marker;
      const cameraPosition = resolveSourceCameraPosition(marker);
      if (!cameraPosition) continue;
      const lineId = target.id;
      nextIds.add(lineId);

      const points = [
        new THREE.Vector3(...cameraPosition),
        new THREE.Vector3(...marker.position)
      ];
      const color = marker.color ?? '#38bdf8';
      const existing = tagLineMeshes.get(lineId);
      if (!existing) {
        const geometry = new THREE.BufferGeometry().setFromPoints(points);
        const material = new THREE.LineBasicMaterial({
          color,
          transparent: true,
          opacity: 0.6
        });
        const line = new THREE.Line(geometry, material);
        line.renderOrder = 3;
        tagLineGroup.add(line);
        tagLineMeshes.set(lineId, line);
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

    for (const [id, line] of tagLineMeshes.entries()) {
      if (nextIds.has(id)) continue;
      tagLineGroup.remove(line);
      disposeObject(line);
      tagLineMeshes.delete(id);
    }

    tagLineGroup.visible = true;
  }

  function enableMinimapLayer(object: THREE.Object3D) {
    object.traverse((node) => {
      node.layers.enable(1);
    });
  }

  function setMinimapOnlyLayer(object: THREE.Object3D) {
    object.traverse((node) => {
      node.layers.set(1);
    });
  }


  function animate() {
    if (!browser) return;
    animationFrame = requestAnimationFrame(animate);
    updateRendererSizes();
    updateMinimapPoseAndTrail();
    if (cameraPovEnabled) {
      applyMainCameraPov();
    } else {
      applyRobotFollowPovDelta();
      controls?.update();
    }
    rendererMain?.render(scene, mainCamera);
    rendererTop?.render(scene, topCamera);
  }

  function stopAnimation() {
    if (!browser || animationFrame === null) return;
    cancelAnimationFrame(animationFrame);
    animationFrame = null;
  }

  $effect(() => {
    const next = normalizeRobot(robot);
    if (!dimensionsEqual(robotDimensions, next)) {
      robotDimensions = next;
      buildRobotBumpers({ browser, robotGroup, robotDimensions, bumperColor, showRobot });
      setRobotGhostVisual(robotGhostActive);
    }
  });

  $effect(() => {
    void showRobot;
    buildRobotBumpers({ browser, robotGroup, robotDimensions, bumperColor, showRobot });
    setRobotGhostVisual(robotGhostActive);
  });

  $effect(() => {
    void robotOverlays;
    void showRobot;
    void robotDimensions;
    syncRobotOverlays({
      browser,
      robotOverlayGroup,
      robotOverlayMeshes,
      robotOverlays,
      showRobot,
      robotDimensions
    });
  });

  $effect(() => {
    const next = normalizeCameras(cameras);
    if (!camerasEqual(cameraLayout, next)) {
      cameraLayout = next;
      rebuildCameraRig({
        browser,
        cameraRigGroup,
        cameraMeshes,
        cameraLayout,
        showCameras,
        cameraTransforms,
        cameraHighlightColor,
        cameraGhostActive
      });
    }
  });

  $effect(() => {
    void showCameras;
    rebuildCameraRig({
      browser,
      cameraRigGroup,
      cameraMeshes,
      cameraLayout,
      showCameras,
      cameraTransforms,
      cameraHighlightColor,
      cameraGhostActive
    });
  });

  $effect(() => {
    void cameraTransforms;
    rebuildCameraRig({
      browser,
      cameraRigGroup,
      cameraMeshes,
      cameraLayout,
      showCameras,
      cameraTransforms,
      cameraHighlightColor,
      cameraGhostActive
    });
  });

  $effect(() => {
    void cameraHighlightColor;
    rebuildCameraRig({
      browser,
      cameraRigGroup,
      cameraMeshes,
      cameraLayout,
      showCameras,
      cameraTransforms,
      cameraHighlightColor,
      cameraGhostActive
    });
  });

  $effect(() => {
    void cameraGhostActive;
    rebuildCameraRig({
      browser,
      cameraRigGroup,
      cameraMeshes,
      cameraLayout,
      showCameras,
      cameraTransforms,
      cameraHighlightColor,
      cameraGhostActive
    });
  });
	</script>

<div class="relative h-full min-h-0 w-full overflow-hidden">
  <LocalizationScene bind:mainContainer bind:mainCanvas {activeField} {footerStatus} />
  <LocalizationMinimap
    bind:minimapExpanded
    bind:topContainer
    bind:topCanvas
    {activeField}
    {metricsActive}
    {onMetricsToggle}
    {minimapControls}
  />
</div>
