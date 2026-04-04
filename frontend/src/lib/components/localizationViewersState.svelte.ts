import { browser } from '$app/environment';
import { onDestroy, onMount, type Snippet } from 'svelte';
import * as THREE from 'three';
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
  type ArucoMarker,
  type LocalizationFieldDefinition,
  type LocalizationMarker,
  type LocalizationViewerProps,
  type PolygonMarker
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
  applyMainCameraPov,
  applyRobotFollowPovDelta,
  applyTopCameraCenterBias,
  captureMainCameraState,
  clearMinimapTrail,
  restoreMainCameraState,
  setMinimapOnlyLayer,
  updateMinimapPoseAndTrail
} from './localizationViewersRuntime';
import {
  enableMinimapLayer,
  updateMarkers,
  updateReferenceMarkers,
  updateTagLines
} from './localizationViewersMarkers';
import {
  animate,
  disposeScene,
  initScene,
  observeResizes,
  setRobotGhostVisual,
  stopAnimation,
  updateRendererSizes
} from './localizationViewersScene';
import { buildLocalizationViewerContexts } from './localizationViewersContexts';

type LocalizationViewerStateProps = LocalizationViewerProps & {
  robot?: RobotDimensions;
  cameras?: RigCameraInfo[];
  footerStatus?: Snippet;
  minimapControls?: Snippet;
  metricsActive?: boolean;
  onMetricsToggle?: (() => void) | null;
};

export function createLocalizationViewersState(props: LocalizationViewerStateProps) {
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
  } = props;

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
  let robotDimensions = $state<RobotDimensions>(normalizeRobot(null));
  let cameraLayout = $state<RigCameraInfo[]>(normalizeCameras(null));
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
  const { povContext, minimapContext, markerContext, sceneContext } =
    buildLocalizationViewerContexts({
      animationFrame: {
        get: () => animationFrame,
        set: (value) => (animationFrame = value)
      },
      arucoTextureCache: () => arucoTextureCache,
      browser: () => browser,
      bumperColor: () => bumperColor,
      cameraForward: () => cameraForward,
      cameraGhostActive: () => cameraGhostActive,
      cameraHighlightColor: () => cameraHighlightColor,
      cameraLayout: {
        get: () => cameraLayout,
        set: (value) => (cameraLayout = value)
      },
      cameraMeshes: () => cameraMeshes,
      cameraPovApplyFov: () => cameraPovApplyFov,
      cameraPovEnabled: () => cameraPovEnabled,
      cameraPovForwardSign: () => cameraPovForwardSign,
      cameraPovIntrinsics: () => cameraPovIntrinsics,
      cameraPovTransform: () => cameraPovTransform,
      cameraRigGroup: {
        get: () => cameraRigGroup,
        set: (value) => (cameraRigGroup = value)
      },
      cameraTransforms: () => cameraTransforms,
      cameraUp: () => cameraUp,
      controls: {
        get: () => controls,
        set: (value) => (controls = value as OrbitControls)
      },
      customField: () => customField,
      DEFAULT_MAIN_FOV: () => DEFAULT_MAIN_FOV,
      environmentGroup: {
        get: () => environmentGroup,
        set: (value) => (environmentGroup = value)
      },
      lastKnownMinimapPoseDot: {
        get: () => lastKnownMinimapPoseDot,
        set: (value) => (lastKnownMinimapPoseDot = value)
      },
      lastKnownRobotTransform: () => lastKnownRobotTransform,
      lastMainHeight: {
        get: () => lastMainHeight,
        set: (value) => (lastMainHeight = value)
      },
      lastMainWidth: {
        get: () => lastMainWidth,
        set: (value) => (lastMainWidth = value)
      },
      lastPixelRatio: {
        get: () => lastPixelRatio,
        set: (value) => (lastPixelRatio = value)
      },
      lastResolvedCameraPovIntrinsics: {
        get: () => lastResolvedCameraPovIntrinsics,
        set: (value) => (lastResolvedCameraPovIntrinsics = value)
      },
      lastResolvedCameraPovTransform: {
        get: () => lastResolvedCameraPovTransform,
        set: (value) => (lastResolvedCameraPovTransform = value)
      },
      lastRobotFollowWorldPose: {
        get: () => lastRobotFollowWorldPose,
        set: (value) => (lastRobotFollowWorldPose = value)
      },
      lastTopHeight: {
        get: () => lastTopHeight,
        set: (value) => (lastTopHeight = value)
      },
      lastTopWidth: {
        get: () => lastTopWidth,
        set: (value) => (lastTopWidth = value)
      },
      mainCamera: {
        get: () => mainCamera,
        set: (value) => (mainCamera = value)
      },
      mainCanvas: () => mainCanvas,
      mainContainer: () => mainContainer,
      markerColors: () => markerColors,
      markerGroup: {
        get: () => markerGroup,
        set: (value) => (markerGroup = value)
      },
      markerMeshes: () => markerMeshes,
      markers: () => markers,
      metricsActive: () => metricsActive,
      MINIMAP_POSE_DOT_INNER_RADIUS_M: () => MINIMAP_POSE_DOT_INNER_RADIUS_M,
      MINIMAP_POSE_DOT_OUTER_RADIUS_M: () => MINIMAP_POSE_DOT_OUTER_RADIUS_M,
      MINIMAP_TRAIL_DUPLICATE_DISTANCE_M: () => MINIMAP_TRAIL_DUPLICATE_DISTANCE_M,
      MINIMAP_TRAIL_LINE_WIDTH_M: () => MINIMAP_TRAIL_LINE_WIDTH_M,
      MINIMAP_TRAIL_MAX_POINTS: () => MINIMAP_TRAIL_MAX_POINTS,
      MINIMAP_TRAIL_WINDOW_MS: () => MINIMAP_TRAIL_WINDOW_MS,
      minimapControls: () => minimapControls,
      minimapExpanded: {
        get: () => minimapExpanded,
        set: (value) => (minimapExpanded = value)
      },
      minimapPoseDot: () => minimapPoseDot,
      minimapPoseDotGroup: {
        get: () => minimapPoseDotGroup,
        set: (value) => (minimapPoseDotGroup = value)
      },
      minimapPoseDotInnerMaterial: {
        get: () => minimapPoseDotInnerMaterial,
        set: (value) => (minimapPoseDotInnerMaterial = value)
      },
      minimapPoseDotOuterMaterial: {
        get: () => minimapPoseDotOuterMaterial,
        set: (value) => (minimapPoseDotOuterMaterial = value)
      },
      minimapTrailGroup: {
        get: () => minimapTrailGroup,
        set: (value) => (minimapTrailGroup = value)
      },
      minimapTrailLine: {
        get: () => minimapTrailLine,
        set: (value) => (minimapTrailLine = value)
      },
      minimapTrailLineGeometry: {
        get: () => minimapTrailLineGeometry,
        set: (value) => (minimapTrailLineGeometry = value)
      },
      minimapTrailMaterial: {
        get: () => minimapTrailMaterial,
        set: (value) => (minimapTrailMaterial = value)
      },
      minimapTrailPoints: () => minimapTrailPoints,
      minimapTrailSegmentCount: {
        get: () => minimapTrailSegmentCount,
        set: (value) => (minimapTrailSegmentCount = value)
      },
      mode: () => mode,
      onMetricsToggle: () => onMetricsToggle,
      originIndicatorGroup: {
        get: () => originIndicatorGroup,
        set: (value) => (originIndicatorGroup = value)
      },
      referenceMarkerGroup: {
        get: () => referenceMarkerGroup,
        set: (value) => (referenceMarkerGroup = value)
      },
      referenceMarkerMeshes: () => referenceMarkerMeshes,
      referenceMarkers: () => referenceMarkers,
      rendererMain: {
        get: () => rendererMain,
        set: (value) => (rendererMain = value)
      },
      rendererTop: {
        get: () => rendererTop,
        set: (value) => (rendererTop = value)
      },
      resizeObservers: () => resizeObservers,
      robotDimensions: () => robotDimensions,
      robotFollowPovEnabled: () => robotFollowPovEnabled,
      robotFollowUserInteracting: {
        get: () => robotFollowUserInteracting,
        set: (value) => (robotFollowUserInteracting = value)
      },
      ROBOT_FOLLOW_CLOSE_DISTANCE_M: () => ROBOT_FOLLOW_CLOSE_DISTANCE_M,
      ROBOT_FOLLOW_CLOSE_HEIGHT_M: () => ROBOT_FOLLOW_CLOSE_HEIGHT_M,
      ROBOT_FOLLOW_CLOSE_SIDE_M: () => ROBOT_FOLLOW_CLOSE_SIDE_M,
      ROBOT_FOLLOW_TARGET_UP_M: () => ROBOT_FOLLOW_TARGET_UP_M,
      robotGhostActive: () => robotGhostActive,
      robotGroup: {
        get: () => robotGroup,
        set: (value) => (robotGroup = value)
      },
      robotOverlayGroup: {
        get: () => robotOverlayGroup,
        set: (value) => (robotOverlayGroup = value)
      },
      rootGroup: {
        get: () => rootGroup,
        set: (value) => (rootGroup = value)
      },
      robotTransform: () => robotTransform,
      savedMainCameraState: {
        get: () => savedMainCameraState,
        set: (value) => (savedMainCameraState = value)
      },
      scene: {
        get: () => scene,
        set: (value) => (scene = value)
      },
      showCameras: () => showCameras,
      showFieldImage: () => showFieldImage,
      showMinimapTrail: () => showMinimapTrail,
      showRobot: () => showRobot,
      showTagLines: () => showTagLines,
      tagLineGroup: {
        get: () => tagLineGroup,
        set: (value) => (tagLineGroup = value)
      },
      tagLineMarkers: () => tagLineMarkers,
      tagLineMeshes: () => tagLineMeshes,
      topCamera: {
        get: () => topCamera,
        set: (value) => (topCamera = value)
      },
      topCanvas: () => topCanvas,
      topContainer: () => topContainer
    });

  onMount(() => {
    initScene(sceneContext);
    observeResizes(sceneContext);
    updateMarkers(markerContext);
    animate(sceneContext);

    return () => {
      disposeScene(sceneContext);
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
      minimapTrailSegmentCount = 0;
      minimapTrailPoints.length = 0;
      lastKnownMinimapPoseDot = null;
      lastKnownRobotTransform = null;
      lastRobotFollowWorldPose = null;
      robotFollowWasActive = false;
      robotFollowUserInteracting = false;
      robotGhostActive = false;
    };
  });

  onDestroy(() => {
    stopAnimation(sceneContext);
  });

  $effect(() => {
    buildEnvironment({ environmentGroup, viewMode: mode, customField, showFieldImage });
    if (controls) {
      const mainCameraView = getMainCameraView(mode, customField);
      controls.minDistance = mainCameraView.minDistance;
      controls.maxDistance = mainCameraView.maxDistance;
    }
    updateRendererSizes(sceneContext);
  });

  $effect(() => {
    if (!controls || !mainCamera) return;
    const povActive = cameraPovEnabled;
    if (povActive && !povWasActive) {
      captureMainCameraState(povContext);
    } else if (!povActive && povWasActive) {
      restoreMainCameraState(povContext);
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
      applyRobotFollowPovDelta(povContext);
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
    applyMainCameraPov(povContext);
  });

  $effect(() => {
    updateMarkers(markerContext);
  });

  $effect(() => {
    void markers;
    void tagLineMarkers;
    void showTagLines;
    void cameraTransforms;
    void cameraLayout;
    updateTagLines(markerContext);
  });

  $effect(() => {
    updateReferenceMarkers(markerContext);
  });

  $effect(() => {
    void bumperNumber;
    if (!robotGroup) return;
    buildRobotBumpers({ browser, robotGroup, robotDimensions, bumperColor, showRobot });
    setRobotGhostVisual(sceneContext, robotGhostActive);
  });

  $effect(() => {
    void bumperColor;
    if (!robotGroup) return;
    buildRobotBumpers({ browser, robotGroup, robotDimensions, bumperColor, showRobot });
    setRobotGhostVisual(sceneContext, robotGhostActive);
  });

  $effect(() => {
    if (!rootGroup) return;
    const transform = sceneTransform;
    if (!transform) {
      rootGroup.position.set(0, 0, 0);
      rootGroup.quaternion.identity();
      applyTopCameraCenterBias(minimapContext);
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
    applyTopCameraCenterBias(minimapContext);
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
        setRobotGhostVisual(sceneContext, false);
        return;
      }

      const ghost = lastKnownRobotTransform;
      if (!ghost) {
        robotGroup.position.set(0, 0, 0);
        robotGroup.quaternion.identity();
        setRobotGhostVisual(sceneContext, false);
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
      setRobotGhostVisual(sceneContext, true);
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
      clearMinimapTrail(minimapContext);
    }
    if (minimapTrailGroup) {
      minimapTrailGroup.visible = false;
    }
  });

  $effect(() => {
    const next = normalizeRobot(robot);
    if (!dimensionsEqual(robotDimensions, next)) {
      robotDimensions = next;
      buildRobotBumpers({ browser, robotGroup, robotDimensions, bumperColor, showRobot });
      setRobotGhostVisual(sceneContext, robotGhostActive);
    }
  });

  $effect(() => {
    void showRobot;
    buildRobotBumpers({ browser, robotGroup, robotDimensions, bumperColor, showRobot });
    setRobotGhostVisual(sceneContext, robotGhostActive);
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

  return {
    get activeField() {
      return activeField;
    },
    get footerStatus() {
      return footerStatus;
    },
    get mainCanvas() {
      return mainCanvas;
    },
    set mainCanvas(value: HTMLCanvasElement | null) {
      mainCanvas = value;
    },
    get mainContainer() {
      return mainContainer;
    },
    set mainContainer(value: HTMLDivElement | null) {
      mainContainer = value;
    },
    get metricsActive() {
      return metricsActive;
    },
    get minimapControls() {
      return minimapControls;
    },
    get minimapExpanded() {
      return minimapExpanded;
    },
    set minimapExpanded(value: boolean) {
      minimapExpanded = value;
    },
    get onMetricsToggle() {
      return onMetricsToggle;
    },
    get topCanvas() {
      return topCanvas;
    },
    set topCanvas(value: HTMLCanvasElement | null) {
      topCanvas = value;
    },
    get topContainer() {
      return topContainer;
    },
    set topContainer(value: HTMLDivElement | null) {
      topContainer = value;
    }
  };
}
