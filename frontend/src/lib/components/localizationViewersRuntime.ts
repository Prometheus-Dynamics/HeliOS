import * as THREE from 'three';
import { Line2, LineGeometry, LineMaterial, OrbitControls } from 'three-stdlib';
import type { LocalizationViewerProps } from '$lib/features/localization/viewers/localizationViewerTypes';

type SavedMainCameraState = {
  position: THREE.Vector3;
  quaternion: THREE.Quaternion;
  up: THREE.Vector3;
  target: THREE.Vector3;
  fov: number;
};

type WorldRobotPose = {
  position: THREE.Vector3;
  quaternion: THREE.Quaternion;
};

type MinimapPoseDotSnapshot = {
  position: [number, number, number];
  color: string;
};

export type LocalizationViewerPovContext = {
  get cameraForward(): THREE.Vector3;
  get cameraPovApplyFov(): boolean;
  get cameraPovEnabled(): boolean;
  get cameraPovForwardSign(): number;
  get cameraPovIntrinsics(): LocalizationViewerProps['cameraPovIntrinsics'];
  get cameraPovTransform(): LocalizationViewerProps['cameraPovTransform'];
  get cameraUp(): THREE.Vector3;
  get controls(): OrbitControls | undefined;
  set controls(value: OrbitControls | undefined);
  get DEFAULT_MAIN_FOV(): number;
  get lastKnownRobotTransform(): LocalizationViewerProps['robotTransform'];
  get lastResolvedCameraPovIntrinsics(): LocalizationViewerProps['cameraPovIntrinsics'];
  set lastResolvedCameraPovIntrinsics(
    value: LocalizationViewerProps['cameraPovIntrinsics']
  );
  get lastResolvedCameraPovTransform(): LocalizationViewerProps['cameraPovTransform'];
  set lastResolvedCameraPovTransform(value: LocalizationViewerProps['cameraPovTransform']);
  get lastRobotFollowWorldPose(): WorldRobotPose | null;
  set lastRobotFollowWorldPose(value: WorldRobotPose | null);
  get mainCamera(): THREE.PerspectiveCamera | undefined;
  get rootGroup(): THREE.Group | undefined;
  get robotDimensions(): { length: number; width: number; bumperHeight: number };
  get robotFollowPovEnabled(): boolean;
  get robotFollowUserInteracting(): boolean;
  get ROBOT_FOLLOW_CLOSE_DISTANCE_M(): number;
  get ROBOT_FOLLOW_CLOSE_HEIGHT_M(): number;
  get ROBOT_FOLLOW_CLOSE_SIDE_M(): number;
  get ROBOT_FOLLOW_TARGET_UP_M(): number;
  get robotTransform(): LocalizationViewerProps['robotTransform'];
  get savedMainCameraState(): SavedMainCameraState | null;
  set savedMainCameraState(value: SavedMainCameraState | null);
};

export function captureMainCameraState(ctx: LocalizationViewerPovContext) {
  if (!ctx.mainCamera || !ctx.controls) return;
  ctx.savedMainCameraState = {
    position: ctx.mainCamera.position.clone(),
    quaternion: ctx.mainCamera.quaternion.clone(),
    up: ctx.mainCamera.up.clone(),
    target: ctx.controls.target.clone(),
    fov: ctx.mainCamera.fov
  };
}

export function restoreMainCameraState(ctx: LocalizationViewerPovContext) {
  if (!ctx.mainCamera || !ctx.controls || !ctx.savedMainCameraState) return;
  ctx.mainCamera.position.copy(ctx.savedMainCameraState.position);
  ctx.mainCamera.quaternion.copy(ctx.savedMainCameraState.quaternion);
  ctx.mainCamera.up.copy(ctx.savedMainCameraState.up);
  ctx.mainCamera.fov = ctx.savedMainCameraState.fov;
  ctx.mainCamera.clearViewOffset();
  ctx.mainCamera.updateProjectionMatrix();
  ctx.controls.target.copy(ctx.savedMainCameraState.target);
  ctx.controls.update();
  ctx.savedMainCameraState = null;
}

export function applyMainCameraPov(ctx: LocalizationViewerPovContext) {
  if (!ctx.mainCamera) return;
  const transform = ctx.cameraPovTransform ?? ctx.lastResolvedCameraPovTransform;
  if (!transform) {
    if (!ctx.cameraPovEnabled) {
      ctx.mainCamera.clearViewOffset();
      if (Math.abs(ctx.mainCamera.fov - ctx.DEFAULT_MAIN_FOV) > 1e-6) {
        ctx.mainCamera.fov = ctx.DEFAULT_MAIN_FOV;
        ctx.mainCamera.updateProjectionMatrix();
      }
    }
    return;
  }

  const localPos = new THREE.Vector3(...transform.position);
  const worldPos = ctx.rootGroup ? ctx.rootGroup.localToWorld(localPos.clone()) : localPos;
  const localQuat = transform.quaternion
    ? new THREE.Quaternion(
        transform.quaternion.x,
        transform.quaternion.y,
        transform.quaternion.z,
        transform.quaternion.w
      ).normalize()
    : new THREE.Quaternion();
  const worldQuat = localQuat.clone();
  if (ctx.rootGroup) {
    const rootQuat = new THREE.Quaternion();
    ctx.rootGroup.getWorldQuaternion(rootQuat);
    worldQuat.premultiply(rootQuat).normalize();
  }

  const forwardSign = ctx.cameraPovForwardSign === -1 ? -1 : 1;
  const forward = ctx.cameraForward
    .clone()
    .multiplyScalar(forwardSign)
    .applyQuaternion(worldQuat)
    .normalize();
  const up = ctx.cameraUp.clone().applyQuaternion(worldQuat).normalize();
  const lookAt = worldPos.clone().add(forward);

  ctx.mainCamera.position.copy(worldPos);
  ctx.mainCamera.up.copy(up);
  ctx.mainCamera.lookAt(lookAt);

  if (!ctx.cameraPovApplyFov) {
    ctx.mainCamera.clearViewOffset();
  } else {
    const intrinsics = ctx.cameraPovIntrinsics ?? ctx.lastResolvedCameraPovIntrinsics;
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
      const vfov =
        2 * Math.atan(intrinsics.height / (2 * intrinsics.fy)) * (180 / Math.PI);
      if (Number.isFinite(vfov) && vfov > 1 && vfov < 179) {
        ctx.mainCamera.fov = vfov;
      }
      const fullWidth = Math.max(1, Math.round(intrinsics.width));
      const fullHeight = Math.max(1, Math.round(intrinsics.height));
      const cxOffsetPx = intrinsics.cx - intrinsics.width * 0.5;
      const cyOffsetPx = intrinsics.cy - intrinsics.height * 0.5;
      const maxOffsetX = fullWidth * 0.45;
      const maxOffsetY = fullHeight * 0.45;
      const offsetX = Math.round(
        THREE.MathUtils.clamp(cxOffsetPx, -maxOffsetX, maxOffsetX)
      );
      const offsetY = Math.round(
        THREE.MathUtils.clamp(cyOffsetPx, -maxOffsetY, maxOffsetY)
      );
      if (offsetX !== 0 || offsetY !== 0) {
        ctx.mainCamera.setViewOffset(
          fullWidth,
          fullHeight,
          -offsetX,
          -offsetY,
          fullWidth,
          fullHeight
        );
      } else {
        ctx.mainCamera.clearViewOffset();
      }
      ctx.mainCamera.updateProjectionMatrix();
    } else {
      ctx.mainCamera.clearViewOffset();
      if (Math.abs(ctx.mainCamera.fov - ctx.DEFAULT_MAIN_FOV) > 1e-6) {
        ctx.mainCamera.fov = ctx.DEFAULT_MAIN_FOV;
        ctx.mainCamera.updateProjectionMatrix();
      }
    }
  }

  if (ctx.controls) {
    ctx.controls.target.copy(lookAt);
  }
}

export function resolveWorldRobotPose(
  ctx: LocalizationViewerPovContext,
  transform: LocalizationViewerProps['robotTransform'] | null | undefined
): WorldRobotPose | null {
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

  const worldPos = ctx.rootGroup ? ctx.rootGroup.localToWorld(localPos.clone()) : localPos;
  const worldQuat = localQuat.clone();
  if (ctx.rootGroup) {
    const rootQuat = new THREE.Quaternion();
    ctx.rootGroup.getWorldQuaternion(rootQuat);
    worldQuat.premultiply(rootQuat).normalize();
  }
  return { position: worldPos, quaternion: worldQuat };
}

export function applyRobotFollowPovDelta(ctx: LocalizationViewerPovContext) {
  if (!ctx.mainCamera || !ctx.controls) return;
  if (ctx.cameraPovEnabled || !ctx.robotFollowPovEnabled) {
    ctx.lastRobotFollowWorldPose = null;
    return;
  }

  const current = resolveWorldRobotPose(
    ctx,
    ctx.robotTransform ?? ctx.lastKnownRobotTransform
  );
  if (!current) {
    ctx.lastRobotFollowWorldPose = null;
    return;
  }

  const previous = ctx.lastRobotFollowWorldPose;
  if (!previous) {
    const distance = Math.max(
      ctx.ROBOT_FOLLOW_CLOSE_DISTANCE_M,
      ctx.robotDimensions.length * 0.5
    );
    const height = Math.max(
      ctx.ROBOT_FOLLOW_CLOSE_HEIGHT_M,
      ctx.robotDimensions.bumperHeight * 0.9
    );
    const side = Math.max(
      ctx.ROBOT_FOLLOW_CLOSE_SIDE_M,
      ctx.robotDimensions.width * 0.1
    );
    const targetLift = Math.max(
      ctx.ROBOT_FOLLOW_TARGET_UP_M,
      ctx.robotDimensions.bumperHeight * 0.35
    );
    const forward = new THREE.Vector3(0, 0, 1)
      .applyQuaternion(current.quaternion)
      .normalize();
    const up = new THREE.Vector3(0, 1, 0)
      .applyQuaternion(current.quaternion)
      .normalize();
    const right = new THREE.Vector3(1, 0, 0)
      .applyQuaternion(current.quaternion)
      .normalize();
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
    ctx.controls.target.copy(targetPos);
    ctx.mainCamera.position.copy(cameraPos);
    ctx.lastRobotFollowWorldPose = {
      position: current.position.clone(),
      quaternion: current.quaternion.clone()
    };
    return;
  }

  const translationDelta = current.position.clone().sub(previous.position);
  if (ctx.robotFollowUserInteracting) {
    ctx.mainCamera.position.add(translationDelta);
    ctx.controls.target.add(translationDelta);
    ctx.lastRobotFollowWorldPose = {
      position: current.position.clone(),
      quaternion: current.quaternion.clone()
    };
    return;
  }

  const previousInverseQuat = previous.quaternion.clone().invert();
  const mapPoint = (point: THREE.Vector3): THREE.Vector3 => {
    const local = point
      .clone()
      .sub(previous.position)
      .applyQuaternion(previousInverseQuat);
    return local.applyQuaternion(current.quaternion).add(current.position);
  };

  ctx.mainCamera.position.copy(mapPoint(ctx.mainCamera.position));
  ctx.controls.target.copy(mapPoint(ctx.controls.target));
  ctx.lastRobotFollowWorldPose = {
    position: current.position.clone(),
    quaternion: current.quaternion.clone()
  };
}

type MinimapTrailPoint = {
  point: THREE.Vector3;
  timestampMs: number;
};

export type LocalizationViewerMinimapContext = {
  get lastKnownMinimapPoseDot(): MinimapPoseDotSnapshot | null;
  set lastKnownMinimapPoseDot(value: MinimapPoseDotSnapshot | null);
  get minimapPoseDot(): LocalizationViewerProps['minimapPoseDot'];
  get minimapPoseDotGroup(): THREE.Group | undefined;
  get minimapPoseDotInnerMaterial(): THREE.MeshBasicMaterial | null;
  get minimapPoseDotOuterMaterial(): THREE.MeshBasicMaterial | null;
  get minimapTrailGroup(): THREE.Group | undefined;
  get minimapTrailLine(): Line2 | null;
  get minimapTrailLineGeometry(): LineGeometry | null;
  get minimapTrailMaterial(): LineMaterial | null;
  get minimapTrailPoints(): MinimapTrailPoint[];
  get minimapTrailSegmentCount(): number;
  set minimapTrailSegmentCount(value: number);
  get MINIMAP_TRAIL_DUPLICATE_DISTANCE_M(): number;
  get MINIMAP_TRAIL_MAX_POINTS(): number;
  get MINIMAP_TRAIL_WINDOW_MS(): number;
  get rootGroup(): THREE.Group | undefined;
  get showMinimapTrail(): boolean;
  get topCamera(): THREE.OrthographicCamera | undefined;
};

export function clearMinimapTrail(ctx: LocalizationViewerMinimapContext) {
  ctx.minimapTrailPoints.length = 0;
  ctx.minimapTrailSegmentCount = 0;
  syncMinimapTrailGeometry(ctx);
}

export function pruneMinimapTrail(
  ctx: LocalizationViewerMinimapContext,
  nowMs: number
) {
  const cutoff = nowMs - ctx.MINIMAP_TRAIL_WINDOW_MS;
  let removeCount = 0;
  for (const entry of ctx.minimapTrailPoints) {
    if (entry.timestampMs >= cutoff) break;
    removeCount += 1;
  }
  if (removeCount > 0) {
    ctx.minimapTrailPoints.splice(0, removeCount);
  }
  if (ctx.minimapTrailPoints.length > ctx.MINIMAP_TRAIL_MAX_POINTS) {
    ctx.minimapTrailPoints.splice(
      0,
      ctx.minimapTrailPoints.length - ctx.MINIMAP_TRAIL_MAX_POINTS
    );
  }
}

export function syncMinimapTrailGeometry(ctx: LocalizationViewerMinimapContext) {
  if (!ctx.minimapTrailLineGeometry || !ctx.minimapTrailLine) return;
  const count = Math.min(
    ctx.minimapTrailPoints.length,
    ctx.MINIMAP_TRAIL_MAX_POINTS
  );
  if (count < 2) {
    ctx.minimapTrailSegmentCount = 0;
    ctx.minimapTrailLineGeometry.setPositions([0, 0, 0, 0, 0, 0]);
    return;
  }
  const positions: number[] = [];
  for (let i = 0; i < count; i += 1) {
    const entry = ctx.minimapTrailPoints[i];
    if (!entry) continue;
    positions.push(entry.point.x, entry.point.y, entry.point.z);
  }
  if (positions.length < 6) {
    ctx.minimapTrailSegmentCount = 0;
    ctx.minimapTrailLineGeometry.setPositions([0, 0, 0, 0, 0, 0]);
    return;
  }
  ctx.minimapTrailSegmentCount = Math.max(0, positions.length / 3 - 1);
  ctx.minimapTrailLineGeometry.setPositions(positions);
  ctx.minimapTrailLineGeometry.computeBoundingSphere();
}

export function pushMinimapTrailPoint(
  ctx: LocalizationViewerMinimapContext,
  point: THREE.Vector3,
  nowMs = performance.now()
) {
  pruneMinimapTrail(ctx, nowMs);
  const last = ctx.minimapTrailPoints[ctx.minimapTrailPoints.length - 1] ?? null;
  if (last) {
    const dist2 = last.point.distanceToSquared(point);
    const duplicateDist2 =
      ctx.MINIMAP_TRAIL_DUPLICATE_DISTANCE_M *
      ctx.MINIMAP_TRAIL_DUPLICATE_DISTANCE_M;
    if (dist2 <= duplicateDist2) {
      return;
    }
  }

  ctx.minimapTrailPoints.push({ point: point.clone(), timestampMs: nowMs });
  pruneMinimapTrail(ctx, nowMs);
  syncMinimapTrailGeometry(ctx);
}

export function updateMinimapPoseAndTrail(ctx: LocalizationViewerMinimapContext) {
  if (
    !ctx.minimapPoseDotGroup ||
    !ctx.minimapPoseDotOuterMaterial ||
    !ctx.minimapPoseDotInnerMaterial ||
    !ctx.minimapTrailGroup
  ) {
    return;
  }
  const dot = ctx.minimapPoseDot;
  const trailEnabled = ctx.showMinimapTrail;
  const nowMs = performance.now();
  pruneMinimapTrail(ctx, nowMs);
  syncMinimapTrailGeometry(ctx);

  if (!dot) {
    const ghost = ctx.lastKnownMinimapPoseDot;
    if (ghost) {
      ctx.minimapPoseDotGroup.visible = true;
      ctx.minimapPoseDotGroup.position.set(...ghost.position);
      ctx.minimapPoseDotOuterMaterial.opacity = 0.55;
      ctx.minimapPoseDotInnerMaterial.opacity = 0.42;
      ctx.minimapPoseDotInnerMaterial.color.set(ghost.color);
      if (trailEnabled && ctx.minimapTrailMaterial) {
        ctx.minimapTrailMaterial.color.set(ghost.color);
        ctx.minimapTrailMaterial.opacity = 1;
      }
    } else {
      ctx.minimapPoseDotGroup.visible = false;
    }
    ctx.minimapTrailGroup.visible = trailEnabled && ctx.minimapTrailSegmentCount > 0;
    return;
  }

  const [x, y, z] = dot.position;
  if (![x, y, z].every((value) => Number.isFinite(value))) {
    const ghost = ctx.lastKnownMinimapPoseDot;
    if (ghost) {
      ctx.minimapPoseDotGroup.visible = true;
      ctx.minimapPoseDotGroup.position.set(...ghost.position);
      ctx.minimapPoseDotOuterMaterial.opacity = 0.55;
      ctx.minimapPoseDotInnerMaterial.opacity = 0.42;
      ctx.minimapPoseDotInnerMaterial.color.set(ghost.color);
      if (trailEnabled && ctx.minimapTrailMaterial) {
        ctx.minimapTrailMaterial.color.set(ghost.color);
        ctx.minimapTrailMaterial.opacity = 1;
      }
    } else {
      ctx.minimapPoseDotGroup.visible = false;
    }
    ctx.minimapTrailGroup.visible = trailEnabled && ctx.minimapTrailSegmentCount > 0;
    return;
  }

  ctx.minimapPoseDotGroup.visible = true;
  ctx.minimapPoseDotGroup.position.set(x, y, z);
  const color = dot.color ?? '#38bdf8';
  ctx.lastKnownMinimapPoseDot = { position: [x, y, z], color };
  ctx.minimapPoseDotOuterMaterial.opacity = 0.95;
  ctx.minimapPoseDotInnerMaterial.opacity = 1;
  ctx.minimapPoseDotInnerMaterial.color.set(color);

  if (trailEnabled) {
    pushMinimapTrailPoint(ctx, new THREE.Vector3(x, y, z), nowMs);
    if (ctx.minimapTrailMaterial) {
      ctx.minimapTrailMaterial.color.set(color);
      ctx.minimapTrailMaterial.opacity = 1;
    }
    ctx.minimapTrailGroup.visible = ctx.minimapTrailSegmentCount > 0;
  } else {
    ctx.minimapTrailGroup.visible = false;
  }
}

export function applyTopCameraCenterBias(ctx: LocalizationViewerMinimapContext) {
  if (!ctx.topCamera || !ctx.rootGroup) return;
  const centerX = ctx.rootGroup.position.x;
  const centerY = ctx.rootGroup.position.y;
  const centerZ = ctx.rootGroup.position.z;
  ctx.topCamera.position.set(centerX, ctx.topCamera.position.y, centerZ);
  ctx.topCamera.lookAt(centerX, centerY, centerZ);
}

export function setMinimapOnlyLayer(object: THREE.Object3D) {
  object.traverse((node) => {
    node.layers.set(1);
  });
}
