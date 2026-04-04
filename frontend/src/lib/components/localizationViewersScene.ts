import * as THREE from 'three';
import { Line2, LineGeometry, LineMaterial, OrbitControls } from 'three-stdlib';
import type { RobotDimensions } from '$lib/types/rig';
import {
  buildEnvironment,
  buildRobotBumpers,
  createAxesHelper,
  disposeObject,
  getMainCameraView,
  getTopCameraBounds,
  rebuildCameraRig
} from '$lib/features/localization/viewers/localizationSceneLifecycle';
import type {
  LocalizationFieldDefinition,
  LocalizationViewerProps
} from '$lib/features/localization/viewers/localizationViewerTypes';
import {
  applyMainCameraPov,
  applyRobotFollowPovDelta,
  applyTopCameraCenterBias,
  setMinimapOnlyLayer,
  updateMinimapPoseAndTrail,
  type LocalizationViewerMinimapContext,
  type LocalizationViewerPovContext
} from './localizationViewersRuntime';

export type LocalizationViewerSceneContext = {
  get animationFrame(): number | null;
  set animationFrame(value: number | null);
  get browser(): boolean;
  get bumperColor(): string | null;
  get cameraGhostActive(): boolean;
  get cameraHighlightColor(): string | null;
  get cameraLayout(): LocalizationViewerProps['cameras'];
  get cameraMeshes(): Map<string, THREE.Group>;
  get cameraRigGroup(): THREE.Group | undefined;
  set cameraRigGroup(value: THREE.Group);
  get cameraTransforms(): LocalizationViewerProps['cameraTransforms'];
  get customField(): LocalizationFieldDefinition | null;
  get environmentGroup(): THREE.Group | undefined;
  set environmentGroup(value: THREE.Group);
  get lastMainHeight(): number;
  set lastMainHeight(value: number);
  get lastMainWidth(): number;
  set lastMainWidth(value: number);
  get lastPixelRatio(): number;
  set lastPixelRatio(value: number);
  get lastTopHeight(): number;
  set lastTopHeight(value: number);
  get lastTopWidth(): number;
  set lastTopWidth(value: number);
  get mainCamera(): THREE.PerspectiveCamera | undefined;
  set mainCamera(value: THREE.PerspectiveCamera);
  get mainCanvas(): HTMLCanvasElement | null;
  get mainContainer(): HTMLDivElement | null;
  get markerGroup(): THREE.Group | undefined;
  set markerGroup(value: THREE.Group);
  get MINIMAP_POSE_DOT_INNER_RADIUS_M(): number;
  get MINIMAP_POSE_DOT_OUTER_RADIUS_M(): number;
  get MINIMAP_TRAIL_LINE_WIDTH_M(): number;
  get minimapContext(): LocalizationViewerMinimapContext;
  get minimapPoseDotGroup(): THREE.Group | undefined;
  set minimapPoseDotGroup(value: THREE.Group);
  get minimapPoseDotInnerMaterial(): THREE.MeshBasicMaterial | null;
  set minimapPoseDotInnerMaterial(value: THREE.MeshBasicMaterial | null);
  get minimapPoseDotOuterMaterial(): THREE.MeshBasicMaterial | null;
  set minimapPoseDotOuterMaterial(value: THREE.MeshBasicMaterial | null);
  get minimapTrailGroup(): THREE.Group | undefined;
  set minimapTrailGroup(value: THREE.Group);
  get minimapTrailLine(): Line2 | null;
  set minimapTrailLine(value: Line2 | null);
  get minimapTrailLineGeometry(): LineGeometry | null;
  set minimapTrailLineGeometry(value: LineGeometry | null);
  get minimapTrailMaterial(): LineMaterial | null;
  set minimapTrailMaterial(value: LineMaterial | null);
  get mode(): LocalizationViewerProps['mode'];
  get originIndicatorGroup(): THREE.Group | undefined;
  set originIndicatorGroup(value: THREE.Group);
  get povContext(): LocalizationViewerPovContext;
  get referenceMarkerGroup(): THREE.Group | undefined;
  set referenceMarkerGroup(value: THREE.Group);
  get resizeObservers(): ResizeObserver[];
  get rendererMain(): THREE.WebGLRenderer | undefined;
  set rendererMain(value: THREE.WebGLRenderer);
  get rendererTop(): THREE.WebGLRenderer | undefined;
  set rendererTop(value: THREE.WebGLRenderer);
  get robotDimensions(): RobotDimensions;
  get robotGhostActive(): boolean;
  get robotGroup(): THREE.Group | undefined;
  set robotGroup(value: THREE.Group);
  get robotFollowUserInteracting(): boolean;
  set robotFollowUserInteracting(value: boolean);
  get robotOverlayGroup(): THREE.Group | undefined;
  set robotOverlayGroup(value: THREE.Group);
  get rootGroup(): THREE.Group | undefined;
  set rootGroup(value: THREE.Group);
  get scene(): THREE.Scene | undefined;
  set scene(value: THREE.Scene);
  get showCameras(): boolean;
  get showFieldImage(): boolean;
  get showRobot(): boolean;
  get tagLineGroup(): THREE.Group | undefined;
  set tagLineGroup(value: THREE.Group);
  get topCamera(): THREE.OrthographicCamera | undefined;
  set topCamera(value: THREE.OrthographicCamera);
  get topCanvas(): HTMLCanvasElement | null;
  get topContainer(): HTMLDivElement | null;
};

export function setRobotGhostVisual(ctx: LocalizationViewerSceneContext, ghost: boolean) {
  if (!ctx.robotGroup) return;
  ctx.robotGroup.traverse((node) => {
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

export function initScene(ctx: LocalizationViewerSceneContext) {
  if (!ctx.mainCanvas || !ctx.topCanvas) return;
  ctx.scene = new THREE.Scene();
  ctx.scene.background = new THREE.Color(0x04070f);

  const hemi = new THREE.HemisphereLight(0x6c8cff, 0x060a14, 0.8);
  ctx.scene.add(hemi);

  const dirLight = new THREE.DirectionalLight(0xfff1c1, 0.65);
  dirLight.position.set(12, 18, 8);
  dirLight.castShadow = true;
  dirLight.shadow.mapSize.set(1024, 1024);
  ctx.scene.add(dirLight);

  ctx.originIndicatorGroup = new THREE.Group();
  ctx.originIndicatorGroup.visible = false;
  const originAxes = createAxesHelper(5.5);
  const originAxesMaterial = originAxes.material as THREE.LineBasicMaterial;
  originAxesMaterial.opacity = 0.95;
  originAxes.position.y = 0.03;
  ctx.originIndicatorGroup.add(originAxes);
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
  ctx.originIndicatorGroup.add(originCore);
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
  ctx.originIndicatorGroup.add(originHalo);
  ctx.scene.add(ctx.originIndicatorGroup);

  ctx.rootGroup = new THREE.Group();
  ctx.scene.add(ctx.rootGroup);

  ctx.environmentGroup = new THREE.Group();
  ctx.rootGroup.add(ctx.environmentGroup);
  buildEnvironment({
    environmentGroup: ctx.environmentGroup,
    viewMode: ctx.mode,
    customField: ctx.customField,
    showFieldImage: ctx.showFieldImage
  });

  ctx.referenceMarkerGroup = new THREE.Group();
  ctx.rootGroup.add(ctx.referenceMarkerGroup);

  ctx.markerGroup = new THREE.Group();
  ctx.rootGroup.add(ctx.markerGroup);
  ctx.tagLineGroup = new THREE.Group();
  ctx.rootGroup.add(ctx.tagLineGroup);
  ctx.robotGroup = new THREE.Group();
  ctx.rootGroup.add(ctx.robotGroup);
  ctx.robotOverlayGroup = new THREE.Group();
  ctx.rootGroup.add(ctx.robotOverlayGroup);
  buildRobotBumpers({
    browser: ctx.browser,
    robotGroup: ctx.robotGroup,
    robotDimensions: ctx.robotDimensions,
    bumperColor: ctx.bumperColor,
    showRobot: ctx.showRobot
  });
  ctx.cameraRigGroup = new THREE.Group();
  ctx.rootGroup.add(ctx.cameraRigGroup);
  rebuildCameraRig({
    browser: ctx.browser,
    cameraRigGroup: ctx.cameraRigGroup,
    cameraMeshes: ctx.cameraMeshes,
    cameraLayout: ctx.cameraLayout ?? [],
    showCameras: ctx.showCameras,
    cameraTransforms: ctx.cameraTransforms,
    cameraHighlightColor: ctx.cameraHighlightColor,
    cameraGhostActive: ctx.cameraGhostActive
  });

  ctx.minimapTrailGroup = new THREE.Group();
  ctx.minimapTrailGroup.visible = false;
  const minimapTrailGeometry = new LineGeometry();
  minimapTrailGeometry.setPositions([0, 0, 0, 0, 0, 0]);
  const minimapTrailLineMaterial = new LineMaterial({
    color: 0x38bdf8,
    transparent: true,
    opacity: 0.95,
    depthTest: false,
    depthWrite: false,
    worldUnits: true,
    linewidth: ctx.MINIMAP_TRAIL_LINE_WIDTH_M
  });
  minimapTrailLineMaterial.resolution.set(1, 1);
  const minimapTrail = new Line2(minimapTrailGeometry, minimapTrailLineMaterial);
  minimapTrail.frustumCulled = false;
  minimapTrail.renderOrder = 998;
  ctx.minimapTrailGroup.add(minimapTrail);
  setMinimapOnlyLayer(ctx.minimapTrailGroup);
  ctx.rootGroup.add(ctx.minimapTrailGroup);
  ctx.minimapTrailMaterial = minimapTrailLineMaterial;
  ctx.minimapTrailLineGeometry = minimapTrailGeometry;
  ctx.minimapTrailLine = minimapTrail;

  ctx.minimapPoseDotGroup = new THREE.Group();
  ctx.minimapPoseDotGroup.visible = false;
  const outerMaterial = new THREE.MeshBasicMaterial({
    color: 0x020617,
    transparent: true,
    opacity: 0.95,
    depthTest: false,
    depthWrite: false
  });
  const outerDot = new THREE.Mesh(
    new THREE.CircleGeometry(ctx.MINIMAP_POSE_DOT_OUTER_RADIUS_M, 40),
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
    new THREE.CircleGeometry(ctx.MINIMAP_POSE_DOT_INNER_RADIUS_M, 40),
    innerMaterial
  );
  outerDot.rotation.x = -Math.PI / 2;
  innerDot.rotation.x = -Math.PI / 2;
  innerDot.position.y = 0.001;
  outerDot.renderOrder = 1000;
  innerDot.renderOrder = 1001;
  ctx.minimapPoseDotGroup.add(outerDot, innerDot);
  setMinimapOnlyLayer(ctx.minimapPoseDotGroup);
  ctx.rootGroup.add(ctx.minimapPoseDotGroup);
  ctx.minimapPoseDotOuterMaterial = outerMaterial;
  ctx.minimapPoseDotInnerMaterial = innerMaterial;

  const mainCameraView = getMainCameraView(ctx.mode, ctx.customField);
  ctx.mainCamera = new THREE.PerspectiveCamera(55, 1, 0.1, 200);
  ctx.mainCamera.position.set(...mainCameraView.position);
  ctx.mainCamera.lookAt(...mainCameraView.target);
  ctx.mainCamera.layers.enable(0);
  ctx.mainCamera.layers.disable(1);

  ctx.topCamera = new THREE.OrthographicCamera(-20, 20, 20, -20, 0.1, 200);
  ctx.topCamera.position.set(0, 40, 0);
  ctx.topCamera.up.set(0, 0, -1);
  ctx.topCamera.lookAt(0, 0, 0);
  ctx.topCamera.layers.enable(1);
  applyTopCameraCenterBias(ctx.minimapContext);

  ctx.rendererMain = new THREE.WebGLRenderer({
    canvas: ctx.mainCanvas,
    antialias: true
  });
  ctx.rendererMain.shadowMap.enabled = true;
  ctx.rendererMain.outputColorSpace = THREE.SRGBColorSpace;

  ctx.rendererTop = new THREE.WebGLRenderer({
    canvas: ctx.topCanvas,
    antialias: true
  });
  ctx.rendererTop.outputColorSpace = THREE.SRGBColorSpace;
  ctx.rendererTop.setClearColor(new THREE.Color(0x03060c), 1);

  const controls = new OrbitControls(ctx.mainCamera, ctx.mainCanvas);
  controls.enableDamping = true;
  controls.minDistance = mainCameraView.minDistance;
  controls.maxDistance = mainCameraView.maxDistance;
  controls.minPolarAngle = 0.01;
  controls.maxPolarAngle = Math.PI - 0.01;
  controls.target.set(...mainCameraView.target);
  controls.update();
  controls.addEventListener('start', () => {
    ctx.robotFollowUserInteracting = true;
  });
  controls.addEventListener('end', () => {
    ctx.robotFollowUserInteracting = false;
  });
  ctx.povContext.controls?.dispose();
  ctx.povContext.controls = controls;
  updateRendererSizes(ctx, true);
}

export function updateRendererSizes(
  ctx: LocalizationViewerSceneContext,
  force = false
) {
  if (!ctx.browser || !ctx.rendererMain || !ctx.rendererTop || !ctx.mainCamera || !ctx.topCamera) {
    return;
  }
  const ratio = Math.min(window.devicePixelRatio ?? 1, 2);
  const ratioChanged = ratio !== ctx.lastPixelRatio;

  const mainWidth = ctx.mainContainer?.clientWidth ?? 0;
  const mainHeight = ctx.mainContainer?.clientHeight ?? 0;
  const mainChanged =
    force || ratioChanged || mainWidth !== ctx.lastMainWidth || mainHeight !== ctx.lastMainHeight;
  if (mainWidth > 0 && mainHeight > 0 && mainChanged) {
    ctx.rendererMain.setPixelRatio(ratio);
    ctx.rendererMain.setSize(mainWidth, mainHeight, false);
    ctx.mainCamera.aspect = mainWidth / mainHeight;
    ctx.mainCamera.updateProjectionMatrix();
    ctx.lastMainWidth = mainWidth;
    ctx.lastMainHeight = mainHeight;
  }

  const topWidth = ctx.topContainer?.clientWidth ?? 0;
  const topHeight = ctx.topContainer?.clientHeight ?? 0;
  const topChanged =
    force || ratioChanged || topWidth !== ctx.lastTopWidth || topHeight !== ctx.lastTopHeight;
  if (topWidth > 0 && topHeight > 0 && topChanged) {
    ctx.rendererTop.setPixelRatio(ratio);
    ctx.rendererTop.setSize(topWidth, topHeight, false);
    const { halfWidth, halfHeight } = getTopCameraBounds(ctx.mode, ctx.customField);
    ctx.topCamera.left = -halfWidth;
    ctx.topCamera.right = halfWidth;
    ctx.topCamera.top = halfHeight;
    ctx.topCamera.bottom = -halfHeight;
    ctx.topCamera.updateProjectionMatrix();
    if (ctx.minimapTrailMaterial) {
      ctx.minimapTrailMaterial.resolution.set(topWidth * ratio, topHeight * ratio);
    }
    ctx.lastTopWidth = topWidth;
    ctx.lastTopHeight = topHeight;
  }

  if (mainChanged || topChanged || ratioChanged) {
    ctx.lastPixelRatio = ratio;
  }
}

export function observeResizes(ctx: LocalizationViewerSceneContext) {
  const elements = [ctx.mainContainer, ctx.topContainer];
  elements.forEach((element) => {
    if (!element) return;
    const observer = new ResizeObserver(() => updateRendererSizes(ctx));
    observer.observe(element);
    ctx.resizeObservers.push(observer);
  });
}

export function animate(ctx: LocalizationViewerSceneContext) {
  if (!ctx.browser || !ctx.scene || !ctx.mainCamera || !ctx.topCamera) return;
  ctx.animationFrame = requestAnimationFrame(() => animate(ctx));
  updateRendererSizes(ctx);
  updateMinimapPoseAndTrail(ctx.minimapContext);
  if (ctx.povContext.cameraPovEnabled) {
    applyMainCameraPov(ctx.povContext);
  } else {
    applyRobotFollowPovDelta(ctx.povContext);
    ctx.povContext.controls?.update();
  }
  ctx.rendererMain?.render(ctx.scene, ctx.mainCamera);
  ctx.rendererTop?.render(ctx.scene, ctx.topCamera);
}

export function stopAnimation(ctx: LocalizationViewerSceneContext) {
  if (!ctx.browser || ctx.animationFrame === null) return;
  cancelAnimationFrame(ctx.animationFrame);
  ctx.animationFrame = null;
}

export function disposeScene(ctx: LocalizationViewerSceneContext) {
  stopAnimation(ctx);
  ctx.povContext.controls?.dispose();
  ctx.rendererMain?.dispose();
  ctx.rendererTop?.dispose();
  for (const mesh of ctx.cameraMeshes.values()) {
    disposeObject(mesh);
  }
  ctx.cameraMeshes.clear();
  if (ctx.minimapTrailGroup) {
    disposeObject(ctx.minimapTrailGroup);
  }
  ctx.minimapTrailMaterial = null;
  ctx.minimapTrailLineGeometry = null;
  ctx.minimapTrailLine = null;
  if (ctx.minimapPoseDotGroup) {
    disposeObject(ctx.minimapPoseDotGroup);
  }
  ctx.minimapPoseDotOuterMaterial = null;
  ctx.minimapPoseDotInnerMaterial = null;
  ctx.resizeObservers.forEach((observer) => observer.disconnect());
}
