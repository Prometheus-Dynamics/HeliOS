import * as THREE from 'three';
import { Line2, LineGeometry, LineMaterial } from 'three-stdlib';
import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
import type {
  LocalizationFieldDefinition,
  LocalizationMarker,
  LocalizationViewerProps
} from '$lib/features/localization/viewers/localizationViewerTypes';
import type {
  LocalizationViewerMinimapContext,
  LocalizationViewerPovContext
} from './localizationViewersRuntime';
import type { LocalizationViewerMarkerContext } from './localizationViewersMarkers';
import type { LocalizationViewerSceneContext } from './localizationViewersScene';

type ReadRef<T> = () => T;
type WriteRef<T> = {
  get: () => T;
  set: (value: T) => void;
};

type LocalizationViewerContextRefs = {
  animationFrame: WriteRef<number | null>;
  arucoTextureCache: ReadRef<Map<string, THREE.Texture>>;
  browser: ReadRef<boolean>;
  bumperColor: ReadRef<string | null>;
  cameraForward: ReadRef<THREE.Vector3>;
  cameraGhostActive: ReadRef<boolean>;
  cameraHighlightColor: ReadRef<string | null>;
  cameraLayout: WriteRef<RigCameraInfo[]>;
  cameraMeshes: ReadRef<Map<string, THREE.Group>>;
  cameraPovApplyFov: ReadRef<boolean>;
  cameraPovEnabled: ReadRef<boolean>;
  cameraPovForwardSign: ReadRef<number>;
  cameraPovIntrinsics: ReadRef<LocalizationViewerProps['cameraPovIntrinsics']>;
  cameraPovTransform: ReadRef<LocalizationViewerProps['cameraPovTransform']>;
  cameraRigGroup: WriteRef<THREE.Group>;
  cameraTransforms: ReadRef<LocalizationViewerProps['cameraTransforms']>;
  cameraUp: ReadRef<THREE.Vector3>;
  controls: WriteRef<unknown>;
  customField: ReadRef<LocalizationFieldDefinition | null>;
  DEFAULT_MAIN_FOV: ReadRef<number>;
  environmentGroup: WriteRef<THREE.Group>;
  lastKnownMinimapPoseDot: WriteRef<{
    position: [number, number, number];
    color: string;
  } | null>;
  lastKnownRobotTransform: ReadRef<LocalizationViewerProps['robotTransform']>;
  lastMainHeight: WriteRef<number>;
  lastMainWidth: WriteRef<number>;
  lastPixelRatio: WriteRef<number>;
  lastResolvedCameraPovIntrinsics: WriteRef<LocalizationViewerProps['cameraPovIntrinsics']>;
  lastResolvedCameraPovTransform: WriteRef<LocalizationViewerProps['cameraPovTransform']>;
  lastRobotFollowWorldPose: WriteRef<{
    position: THREE.Vector3;
    quaternion: THREE.Quaternion;
  } | null>;
  lastTopHeight: WriteRef<number>;
  lastTopWidth: WriteRef<number>;
  mainCamera: WriteRef<THREE.PerspectiveCamera>;
  mainCanvas: ReadRef<HTMLCanvasElement | null>;
  mainContainer: ReadRef<HTMLDivElement | null>;
  markerColors: ReadRef<{ tracking: string; idle: string; offline: string }>;
  markerGroup: WriteRef<THREE.Group>;
  markerMeshes: ReadRef<Map<string, THREE.Group>>;
  markers: ReadRef<LocalizationMarker[]>;
  metricsActive: ReadRef<boolean>;
  MINIMAP_POSE_DOT_INNER_RADIUS_M: ReadRef<number>;
  MINIMAP_POSE_DOT_OUTER_RADIUS_M: ReadRef<number>;
  MINIMAP_TRAIL_DUPLICATE_DISTANCE_M: ReadRef<number>;
  MINIMAP_TRAIL_LINE_WIDTH_M: ReadRef<number>;
  MINIMAP_TRAIL_MAX_POINTS: ReadRef<number>;
  MINIMAP_TRAIL_WINDOW_MS: ReadRef<number>;
  minimapControls: ReadRef<unknown>;
  minimapExpanded: WriteRef<boolean>;
  minimapPoseDot: ReadRef<LocalizationViewerProps['minimapPoseDot']>;
  minimapPoseDotGroup: WriteRef<THREE.Group>;
  minimapPoseDotInnerMaterial: WriteRef<THREE.MeshBasicMaterial | null>;
  minimapPoseDotOuterMaterial: WriteRef<THREE.MeshBasicMaterial | null>;
  minimapTrailGroup: WriteRef<THREE.Group>;
  minimapTrailLine: WriteRef<Line2 | null>;
  minimapTrailLineGeometry: WriteRef<LineGeometry | null>;
  minimapTrailMaterial: WriteRef<LineMaterial | null>;
  minimapTrailPoints: ReadRef<Array<{ point: THREE.Vector3; timestampMs: number }>>;
  minimapTrailSegmentCount: WriteRef<number>;
  mode: ReadRef<LocalizationViewerProps['mode']>;
  onMetricsToggle: ReadRef<(() => void) | null | undefined>;
  originIndicatorGroup: WriteRef<THREE.Group>;
  referenceMarkerGroup: WriteRef<THREE.Group>;
  referenceMarkerMeshes: ReadRef<Map<string, THREE.Group>>;
  referenceMarkers: ReadRef<LocalizationMarker[]>;
  rendererMain: WriteRef<THREE.WebGLRenderer>;
  rendererTop: WriteRef<THREE.WebGLRenderer>;
  resizeObservers: ReadRef<ResizeObserver[]>;
  robotDimensions: ReadRef<RobotDimensions>;
  robotFollowPovEnabled: ReadRef<boolean>;
  robotFollowUserInteracting: WriteRef<boolean>;
  ROBOT_FOLLOW_CLOSE_DISTANCE_M: ReadRef<number>;
  ROBOT_FOLLOW_CLOSE_HEIGHT_M: ReadRef<number>;
  ROBOT_FOLLOW_CLOSE_SIDE_M: ReadRef<number>;
  ROBOT_FOLLOW_TARGET_UP_M: ReadRef<number>;
  robotGhostActive: ReadRef<boolean>;
  robotGroup: WriteRef<THREE.Group>;
  robotOverlayGroup: WriteRef<THREE.Group>;
  rootGroup: WriteRef<THREE.Group>;
  robotTransform: ReadRef<LocalizationViewerProps['robotTransform']>;
  savedMainCameraState: WriteRef<{
    position: THREE.Vector3;
    quaternion: THREE.Quaternion;
    up: THREE.Vector3;
    target: THREE.Vector3;
    fov: number;
  } | null>;
  scene: WriteRef<THREE.Scene>;
  showCameras: ReadRef<boolean>;
  showFieldImage: ReadRef<boolean>;
  showMinimapTrail: ReadRef<boolean>;
  showRobot: ReadRef<boolean>;
  showTagLines: ReadRef<boolean>;
  tagLineGroup: WriteRef<THREE.Group>;
  tagLineMarkers: ReadRef<LocalizationMarker[]>;
  tagLineMeshes: ReadRef<Map<string, THREE.Line>>;
  topCamera: WriteRef<THREE.OrthographicCamera>;
  topCanvas: ReadRef<HTMLCanvasElement | null>;
  topContainer: ReadRef<HTMLDivElement | null>;
};

export function buildLocalizationViewerContexts(refs: LocalizationViewerContextRefs): {
  povContext: LocalizationViewerPovContext;
  minimapContext: LocalizationViewerMinimapContext;
  markerContext: LocalizationViewerMarkerContext;
  sceneContext: LocalizationViewerSceneContext;
} {
  const povContext: LocalizationViewerPovContext = {
    get cameraForward() {
      return refs.cameraForward();
    },
    get cameraPovApplyFov() {
      return refs.cameraPovApplyFov();
    },
    get cameraPovEnabled() {
      return refs.cameraPovEnabled();
    },
    get cameraPovForwardSign() {
      return refs.cameraPovForwardSign();
    },
    get cameraPovIntrinsics() {
      return refs.cameraPovIntrinsics();
    },
    get cameraPovTransform() {
      return refs.cameraPovTransform();
    },
    get cameraUp() {
      return refs.cameraUp();
    },
    get controls() {
      return refs.controls.get() as LocalizationViewerPovContext['controls'];
    },
    set controls(value) {
      refs.controls.set(value);
    },
    get DEFAULT_MAIN_FOV() {
      return refs.DEFAULT_MAIN_FOV();
    },
    get lastKnownRobotTransform() {
      return refs.lastKnownRobotTransform();
    },
    get lastResolvedCameraPovIntrinsics() {
      return refs.lastResolvedCameraPovIntrinsics.get();
    },
    set lastResolvedCameraPovIntrinsics(value) {
      refs.lastResolvedCameraPovIntrinsics.set(value);
    },
    get lastResolvedCameraPovTransform() {
      return refs.lastResolvedCameraPovTransform.get();
    },
    set lastResolvedCameraPovTransform(value) {
      refs.lastResolvedCameraPovTransform.set(value);
    },
    get lastRobotFollowWorldPose() {
      return refs.lastRobotFollowWorldPose.get();
    },
    set lastRobotFollowWorldPose(value) {
      refs.lastRobotFollowWorldPose.set(value);
    },
    get mainCamera() {
      return refs.mainCamera.get();
    },
    get rootGroup() {
      return refs.rootGroup.get();
    },
    get robotDimensions() {
      return refs.robotDimensions();
    },
    get robotFollowPovEnabled() {
      return refs.robotFollowPovEnabled();
    },
    get robotFollowUserInteracting() {
      return refs.robotFollowUserInteracting.get();
    },
    get ROBOT_FOLLOW_CLOSE_DISTANCE_M() {
      return refs.ROBOT_FOLLOW_CLOSE_DISTANCE_M();
    },
    get ROBOT_FOLLOW_CLOSE_HEIGHT_M() {
      return refs.ROBOT_FOLLOW_CLOSE_HEIGHT_M();
    },
    get ROBOT_FOLLOW_CLOSE_SIDE_M() {
      return refs.ROBOT_FOLLOW_CLOSE_SIDE_M();
    },
    get ROBOT_FOLLOW_TARGET_UP_M() {
      return refs.ROBOT_FOLLOW_TARGET_UP_M();
    },
    get robotTransform() {
      return refs.robotTransform();
    },
    get savedMainCameraState() {
      return refs.savedMainCameraState.get();
    },
    set savedMainCameraState(value) {
      refs.savedMainCameraState.set(value);
    }
  };

  const minimapContext: LocalizationViewerMinimapContext = {
    get lastKnownMinimapPoseDot() {
      return refs.lastKnownMinimapPoseDot.get();
    },
    set lastKnownMinimapPoseDot(value) {
      refs.lastKnownMinimapPoseDot.set(value);
    },
    get minimapPoseDot() {
      return refs.minimapPoseDot();
    },
    get minimapPoseDotGroup() {
      return refs.minimapPoseDotGroup.get();
    },
    get minimapPoseDotInnerMaterial() {
      return refs.minimapPoseDotInnerMaterial.get();
    },
    get minimapPoseDotOuterMaterial() {
      return refs.minimapPoseDotOuterMaterial.get();
    },
    get minimapTrailGroup() {
      return refs.minimapTrailGroup.get();
    },
    get minimapTrailLine() {
      return refs.minimapTrailLine.get() as LocalizationViewerMinimapContext['minimapTrailLine'];
    },
    get minimapTrailLineGeometry() {
      return refs.minimapTrailLineGeometry.get() as LocalizationViewerMinimapContext['minimapTrailLineGeometry'];
    },
    get minimapTrailMaterial() {
      return refs.minimapTrailMaterial.get() as LocalizationViewerMinimapContext['minimapTrailMaterial'];
    },
    get minimapTrailPoints() {
      return refs.minimapTrailPoints();
    },
    get minimapTrailSegmentCount() {
      return refs.minimapTrailSegmentCount.get();
    },
    set minimapTrailSegmentCount(value) {
      refs.minimapTrailSegmentCount.set(value);
    },
    get MINIMAP_TRAIL_DUPLICATE_DISTANCE_M() {
      return refs.MINIMAP_TRAIL_DUPLICATE_DISTANCE_M();
    },
    get MINIMAP_TRAIL_MAX_POINTS() {
      return refs.MINIMAP_TRAIL_MAX_POINTS();
    },
    get MINIMAP_TRAIL_WINDOW_MS() {
      return refs.MINIMAP_TRAIL_WINDOW_MS();
    },
    get rootGroup() {
      return refs.rootGroup.get();
    },
    get showMinimapTrail() {
      return refs.showMinimapTrail();
    },
    get topCamera() {
      return refs.topCamera.get();
    }
  };

  const markerContext: LocalizationViewerMarkerContext = {
    get arucoTextureCache() {
      return refs.arucoTextureCache();
    },
    get browser() {
      return refs.browser();
    },
    get cameraLayout() {
      return refs.cameraLayout.get();
    },
    get cameraTransforms() {
      return refs.cameraTransforms();
    },
    get markerColors() {
      return refs.markerColors();
    },
    get markers() {
      return refs.markers();
    },
    get markerGroup() {
      return refs.markerGroup.get();
    },
    get markerMeshes() {
      return refs.markerMeshes();
    },
    get referenceMarkerGroup() {
      return refs.referenceMarkerGroup.get();
    },
    get referenceMarkerMeshes() {
      return refs.referenceMarkerMeshes();
    },
    get referenceMarkers() {
      return refs.referenceMarkers();
    },
    get showTagLines() {
      return refs.showTagLines();
    },
    get tagLineGroup() {
      return refs.tagLineGroup.get();
    },
    get tagLineMarkers() {
      return refs.tagLineMarkers();
    },
    get tagLineMeshes() {
      return refs.tagLineMeshes();
    }
  };

  const sceneContext: LocalizationViewerSceneContext = {
    get animationFrame() {
      return refs.animationFrame.get();
    },
    set animationFrame(value) {
      refs.animationFrame.set(value);
    },
    get browser() {
      return refs.browser();
    },
    get bumperColor() {
      return refs.bumperColor();
    },
    get cameraGhostActive() {
      return refs.cameraGhostActive();
    },
    get cameraHighlightColor() {
      return refs.cameraHighlightColor();
    },
    get cameraLayout() {
      return refs.cameraLayout.get();
    },
    get cameraMeshes() {
      return refs.cameraMeshes();
    },
    get cameraRigGroup() {
      return refs.cameraRigGroup.get();
    },
    set cameraRigGroup(value) {
      refs.cameraRigGroup.set(value);
    },
    get cameraTransforms() {
      return refs.cameraTransforms();
    },
    get customField() {
      return refs.customField();
    },
    get environmentGroup() {
      return refs.environmentGroup.get();
    },
    set environmentGroup(value) {
      refs.environmentGroup.set(value);
    },
    get lastMainHeight() {
      return refs.lastMainHeight.get();
    },
    set lastMainHeight(value) {
      refs.lastMainHeight.set(value);
    },
    get lastMainWidth() {
      return refs.lastMainWidth.get();
    },
    set lastMainWidth(value) {
      refs.lastMainWidth.set(value);
    },
    get lastPixelRatio() {
      return refs.lastPixelRatio.get();
    },
    set lastPixelRatio(value) {
      refs.lastPixelRatio.set(value);
    },
    get lastTopHeight() {
      return refs.lastTopHeight.get();
    },
    set lastTopHeight(value) {
      refs.lastTopHeight.set(value);
    },
    get lastTopWidth() {
      return refs.lastTopWidth.get();
    },
    set lastTopWidth(value) {
      refs.lastTopWidth.set(value);
    },
    get mainCamera() {
      return refs.mainCamera.get();
    },
    set mainCamera(value) {
      refs.mainCamera.set(value);
    },
    get mainCanvas() {
      return refs.mainCanvas();
    },
    get mainContainer() {
      return refs.mainContainer();
    },
    get markerGroup() {
      return refs.markerGroup.get();
    },
    set markerGroup(value) {
      refs.markerGroup.set(value);
    },
    get MINIMAP_POSE_DOT_INNER_RADIUS_M() {
      return refs.MINIMAP_POSE_DOT_INNER_RADIUS_M();
    },
    get MINIMAP_POSE_DOT_OUTER_RADIUS_M() {
      return refs.MINIMAP_POSE_DOT_OUTER_RADIUS_M();
    },
    get MINIMAP_TRAIL_LINE_WIDTH_M() {
      return refs.MINIMAP_TRAIL_LINE_WIDTH_M();
    },
    get minimapContext() {
      return minimapContext;
    },
    get minimapPoseDotGroup() {
      return refs.minimapPoseDotGroup.get();
    },
    set minimapPoseDotGroup(value) {
      refs.minimapPoseDotGroup.set(value);
    },
    get minimapPoseDotInnerMaterial() {
      return refs.minimapPoseDotInnerMaterial.get();
    },
    set minimapPoseDotInnerMaterial(value) {
      refs.minimapPoseDotInnerMaterial.set(value);
    },
    get minimapPoseDotOuterMaterial() {
      return refs.minimapPoseDotOuterMaterial.get();
    },
    set minimapPoseDotOuterMaterial(value) {
      refs.minimapPoseDotOuterMaterial.set(value);
    },
    get minimapTrailGroup() {
      return refs.minimapTrailGroup.get();
    },
    set minimapTrailGroup(value) {
      refs.minimapTrailGroup.set(value);
    },
    get minimapTrailLine() {
      return refs.minimapTrailLine.get() as LocalizationViewerSceneContext['minimapTrailLine'];
    },
    set minimapTrailLine(value) {
      refs.minimapTrailLine.set(value);
    },
    get minimapTrailLineGeometry() {
      return refs.minimapTrailLineGeometry.get() as LocalizationViewerSceneContext['minimapTrailLineGeometry'];
    },
    set minimapTrailLineGeometry(value) {
      refs.minimapTrailLineGeometry.set(value);
    },
    get minimapTrailMaterial() {
      return refs.minimapTrailMaterial.get() as LocalizationViewerSceneContext['minimapTrailMaterial'];
    },
    set minimapTrailMaterial(value) {
      refs.minimapTrailMaterial.set(value);
    },
    get mode() {
      return refs.mode();
    },
    get originIndicatorGroup() {
      return refs.originIndicatorGroup.get();
    },
    set originIndicatorGroup(value) {
      refs.originIndicatorGroup.set(value);
    },
    get povContext() {
      return povContext;
    },
    get referenceMarkerGroup() {
      return refs.referenceMarkerGroup.get();
    },
    set referenceMarkerGroup(value) {
      refs.referenceMarkerGroup.set(value);
    },
    get resizeObservers() {
      return refs.resizeObservers();
    },
    get rendererMain() {
      return refs.rendererMain.get();
    },
    set rendererMain(value) {
      refs.rendererMain.set(value);
    },
    get rendererTop() {
      return refs.rendererTop.get();
    },
    set rendererTop(value) {
      refs.rendererTop.set(value);
    },
    get robotDimensions() {
      return refs.robotDimensions();
    },
    get robotGhostActive() {
      return refs.robotGhostActive();
    },
    get robotGroup() {
      return refs.robotGroup.get();
    },
    set robotGroup(value) {
      refs.robotGroup.set(value);
    },
    get robotFollowUserInteracting() {
      return refs.robotFollowUserInteracting.get();
    },
    set robotFollowUserInteracting(value) {
      refs.robotFollowUserInteracting.set(value);
    },
    get robotOverlayGroup() {
      return refs.robotOverlayGroup.get();
    },
    set robotOverlayGroup(value) {
      refs.robotOverlayGroup.set(value);
    },
    get rootGroup() {
      return refs.rootGroup.get();
    },
    set rootGroup(value) {
      refs.rootGroup.set(value);
    },
    get scene() {
      return refs.scene.get();
    },
    set scene(value) {
      refs.scene.set(value);
    },
    get showCameras() {
      return refs.showCameras();
    },
    get showFieldImage() {
      return refs.showFieldImage();
    },
    get showRobot() {
      return refs.showRobot();
    },
    get tagLineGroup() {
      return refs.tagLineGroup.get();
    },
    set tagLineGroup(value) {
      refs.tagLineGroup.set(value);
    },
    get topCamera() {
      return refs.topCamera.get();
    },
    set topCamera(value) {
      refs.topCamera.set(value);
    },
    get topCanvas() {
      return refs.topCanvas();
    },
    get topContainer() {
      return refs.topContainer();
    }
  };

  return {
    povContext,
    minimapContext,
    markerContext,
    sceneContext
  };
}
