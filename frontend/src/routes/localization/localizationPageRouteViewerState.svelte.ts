import {
  buildViewProfileOverlays,
  buildViewerCameraTransforms,
  buildViewerCameras,
  buildViewerRobotTransform,
  buildViewerShowRobot
} from '$lib/features/localization/page/localizationPageViewHelpers';
import type { LocalizationMarker } from '$lib/features/localization/viewers/localizationViewerTypes';
import { composeTransforms, invertTransform, type PoseQuaternion, type PoseTransform, type Vec3 } from '$lib/features/localization/poseMath';
import {
  cameraFramePositionForDetection,
  cameraKeyVariants,
  detectionInsidePovFov,
  imuSampleQuaternion,
  isImuSource,
  rigPoseToViewerTransform,
  selectedPovIntrinsics,
  sourceKeys
} from './localizationCameraPovUtils';
import {
  buildCameraPovByOptionId,
  buildCameraPovOptions,
  buildCameraPovOptionsBase,
  buildViewerRenderableCameras,
  mergeCameraPovStateCache,
  resolveSelectedCameraPov,
  resolveSelectedLiveCameraPov
} from './localizationViewerCameraState';
import {
  buildDetectedFieldTagDetections,
  buildFieldSpacePoseCache,
  buildLocalTagPoseCache,
  buildMinimapPoseDot,
  buildReferenceMarkersForViewer,
  buildTargetSpaceOverlay,
  buildViewerTagLineMarkers,
  type ViewerDetectionPose
} from './localizationViewerOverlayState';
import {
  FIELD_POSE_LINGER_MS,
  FIELD_POSE_REFRESH_MS,
  LOCAL_TAG_POSE_LINGER_MS,
  LOCAL_TAG_POSE_REFRESH_MS,
  fieldDetectionsForOutputs
} from './localizationPageRouteSupport';
import type { LocalizationPageRouteCore } from './localizationPageRouteCore.svelte';
import type { LocalizationPageRouteProfileState } from './localizationPageRouteProfileState.svelte';
import { createLocalizationPageRouteViewerBaseState } from './localizationPageRouteViewerBaseState.svelte';
import { detectionPosesForSpace } from '$lib/features/localization/markerUtils';

export function createLocalizationPageRouteViewerState(
  core: LocalizationPageRouteCore,
  profile: LocalizationPageRouteProfileState
) {
  const base = createLocalizationPageRouteViewerBaseState(core, profile);

  const viewerCameras = $derived.by(() => {
    const cameras =
      profile.viewerSourcePool.length > 0
        ? buildViewerCameras({
            baseFrame: base.baseFrame,
            rigCameras: core.state.rigLayoutState.layout.cameras,
            selectedSources: profile.viewerSourcePool
          })
        : [];
    if (cameras.length > 0 || base.baseFrame !== 'field') {
      return cameras;
    }
    const cameraInField = profile.activeSolverOutputs?.cameraInField ?? [];
    if (!cameraInField.length) return cameras;
    const seen = new Set(cameras.map((camera) => camera.uid));
    const fallback = [...cameras];
    for (const entry of cameraInField) {
      const key = (entry.cameraUid || entry.sourceId || '').trim();
      if (!key || seen.has(key)) continue;
      seen.add(key);
      fallback.push({
        uid: key,
        streamId: null,
        cameraUid: entry.cameraUid?.trim() || null,
        streamAlias: null,
        driverCameraId: key,
        displayName: key,
        backend: 'Localization',
        pose: { translation: { x: 0, y: 0, z: 0 }, rotation: { roll: 0, pitch: 0, yaw: 0 } }
      });
    }
    return fallback;
  });

  const solverCameraTransforms = $derived.by<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(() => {
    const outputsForCamera =
      profile.viewProfiles.length === 1
        ? (() => {
            const profileEntry = profile.viewProfiles[0];
            const resp =
              core.state.solveResponsesByProfile[profileEntry.id] ??
              (profileEntry.id === core.state.solveResponse?.profileId ? core.state.solveResponse : null);
            const solvers = resp?.solvers ?? [];
            const activeProfileId = core.activeProfile.current?.id ?? null;
            const activeSolverKey = profile.activeSolverConfig?.id ?? core.state.activeSolverId;
            if (profileEntry.id === activeProfileId && activeSolverKey) {
              return solvers.find((solver) => solver.id === activeSolverKey)?.outputs ?? solvers[0]?.outputs ?? null;
            }
            return solvers[0]?.outputs ?? null;
          })()
        : profile.activeSolverOutputs ?? null;

    const list = outputsForCamera?.cameraInField ?? null;
    if (!list || list.length === 0) return null;
    const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = {};
    for (const entry of list) {
      const pose = entry.pose;
      const transform = {
        position: [pose.translation.x, pose.translation.y, pose.translation.z] as Vec3,
        quaternion: pose.rotation.quaternion
      };
      const keys = new Set<string>();
      const addKeys = (value: string | null | undefined) => {
        for (const key of cameraKeyVariants(value)) keys.add(key);
      };
      addKeys(entry.cameraUid);
      addKeys(entry.sourceId);
      const source = core.state.sources.find((candidate) => candidate.id === entry.sourceId) ?? null;
      if (source) {
        addKeys(source.id);
        addKeys(source.cameraUid);
        addKeys(source.streamId);
        addKeys(source.cameraPath);
        for (const key of source.cameraKeys ?? []) addKeys(key);
      }
      const sourceKeysSet = new Set(keys);
      const rigMatch = core.state.rigLayoutState.layout.cameras.find((camera) => {
        const candidates = [
          camera.uid,
          camera.cameraUid ?? null,
          camera.streamId ?? null,
          camera.driverCameraId ?? null,
          camera.hardwareId ?? null,
          camera.streamAlias ?? null
        ].flatMap((value) => cameraKeyVariants(value));
        return candidates.some((candidate) => sourceKeysSet.has(candidate));
      });
      if (rigMatch) {
        addKeys(rigMatch.uid);
        addKeys(rigMatch.cameraUid ?? null);
        addKeys(rigMatch.streamId ?? null);
        addKeys(rigMatch.driverCameraId ?? null);
        addKeys(rigMatch.hardwareId ?? null);
        addKeys(rigMatch.streamAlias ?? null);
      }
      for (const key of keys) {
        out[key] = transform;
      }
    }
    return out;
  });

  const solverRobotTransform = $derived.by<PoseTransform | null>(() => {
    const pose = profile.activeSolverOutputs?.robotInField?.pose ?? null;
    if (!pose) return null;
    return {
      position: [pose.translation.x, pose.translation.y, pose.translation.z],
      quaternion: pose.rotation.quaternion
    };
  });

  const solverRobotCameraTransforms = $derived.by<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(
    () => {
      const list = profile.activeSolverOutputs?.cameraInField ?? null;
      const robotPose = profile.activeSolverOutputs?.robotInField?.pose ?? null;
      if (list && list.length > 0 && robotPose) {
        const fieldFromRobot: PoseTransform = {
          position: [robotPose.translation.x, robotPose.translation.y, robotPose.translation.z],
          quaternion: robotPose.rotation.quaternion
        };
        const robotFromField = invertTransform(fieldFromRobot);
        const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = {};
        for (const entry of list) {
          const pose = entry.pose;
          const fieldFromCamera: PoseTransform = {
            position: [pose.translation.x, pose.translation.y, pose.translation.z],
            quaternion: pose.rotation.quaternion
          };
          const robotFromCamera = composeTransforms(robotFromField, fieldFromCamera);
          const transform = { position: robotFromCamera.position, quaternion: robotFromCamera.quaternion };
          const keys = new Set<string>();
          const addKeys = (value: string | null | undefined) => {
            for (const key of cameraKeyVariants(value)) keys.add(key);
          };
          addKeys(entry.cameraUid);
          addKeys(entry.sourceId);
          const source = core.state.sources.find((candidate) => candidate.id === entry.sourceId) ?? null;
          if (source) {
            addKeys(source.id);
            addKeys(source.cameraUid);
            addKeys(source.streamId);
            addKeys(source.cameraPath);
            for (const key of source.cameraKeys ?? []) addKeys(key);
          }
          for (const key of keys) {
            out[key] = transform;
          }
        }
        return out;
      }

      const tagInCamera = profile.activeSolverOutputs?.tagInCamera ?? null;
      const tagInRobot = profile.activeSolverOutputs?.tagInRobot ?? null;
      if (!tagInCamera || !tagInRobot || tagInCamera.length === 0 || tagInRobot.length === 0) return null;
      const cameraByKey = new Map<string, (typeof tagInCamera)[number]>();
      for (const det of tagInCamera) {
        cameraByKey.set(`${det.sourceId}|${det.cameraUid}|${det.tagId}`, det);
      }
      const best: Record<string, { dist2: number; transform: PoseTransform }> = {};
      for (const detRobot of tagInRobot) {
        const detCam = cameraByKey.get(`${detRobot.sourceId}|${detRobot.cameraUid}|${detRobot.tagId}`) ?? null;
        if (!detCam) continue;
        const camPose = detCam.pose;
        const robotPose = detRobot.pose;
        const cameraFromTag: PoseTransform = {
          position: [camPose.translation.x, camPose.translation.y, camPose.translation.z],
          quaternion: camPose.rotation.quaternion
        };
        const robotFromTag: PoseTransform = {
          position: [robotPose.translation.x, robotPose.translation.y, robotPose.translation.z],
          quaternion: robotPose.rotation.quaternion
        };
        const tagFromCamera = invertTransform(cameraFromTag);
        const robotFromCamera = composeTransforms(robotFromTag, tagFromCamera);
        const dist2 =
          cameraFromTag.position[0] * cameraFromTag.position[0] +
          cameraFromTag.position[1] * cameraFromTag.position[1] +
          cameraFromTag.position[2] * cameraFromTag.position[2];
        const key = (detRobot.cameraUid || detRobot.sourceId).trim();
        if (!key) continue;
        const current = best[key];
        if (!current || dist2 < current.dist2) {
          best[key] = { dist2, transform: robotFromCamera };
        }
      }
      const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = {};
      for (const [key, entry] of Object.entries(best)) {
        const transform = { position: entry.transform.position, quaternion: entry.transform.quaternion };
        for (const variant of cameraKeyVariants(key)) {
          out[variant] = transform;
        }
      }
      return Object.keys(out).length ? out : null;
    }
  );

  const solverCameraTransformsForViewer = $derived.by<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(
    () => {
      if (base.baseFrame === 'field') return solverCameraTransforms;
      if (base.baseFrame === 'robot') return solverRobotCameraTransforms;
      return null;
    }
  );

  const viewProfileOverlays = $derived.by(() =>
    core.state.coordinateSpace === 'camera_in_field'
      ? []
      : buildViewProfileOverlays({
          baseFrame: base.baseFrame,
          showRobotContext: core.state.showRobotContext,
          coordinateSpace: core.state.coordinateSpace,
          viewProfiles: profile.viewProfiles,
          solveResponsesByProfile: core.state.solveResponsesByProfile,
          profiles: core.profiles.current,
          profileIndexById: profile.profileIndexById,
          profileColors: core.PROFILE_COLORS
        })
  );

  const viewerCameraTransforms = $derived.by<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(() => {
    const baseTransforms = buildViewerCameraTransforms({
      baseFrame: base.baseFrame,
      solverCameraTransforms: solverCameraTransformsForViewer,
      viewerCameras,
      separateCameras: core.state.separateCameras,
      primaryCameraKey: profile.primaryCameraKey
    });
    if (base.baseFrame !== 'field') {
      const imuQuaternion = imuSampleQuaternion(core.state.imuRotationSample);
      if (!base.activeImuSource || !imuQuaternion) {
        return baseTransforms;
      }
      const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = { ...(baseTransforms ?? {}) };
      const keys = sourceKeys(base.activeImuSource);
      const existing =
        keys
          .map((key) => out[key])
          .find(
            (entry): entry is { position: Vec3; quaternion?: PoseQuaternion } =>
              Boolean(entry && Array.isArray(entry.position))
          ) ?? null;
      const transform = {
        position: existing?.position ?? ([0, 0, 0] as Vec3),
        quaternion: imuQuaternion
      };
      for (const key of keys) out[key] = transform;
      return out;
    }
    if (!solverRobotTransform) {
      return baseTransforms;
    }
    const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = { ...(baseTransforms ?? {}) };
    const keysForRigCamera = (camera: (typeof viewerCameras)[number]): string[] => {
      const set = new Set<string>();
      const add = (value: string | null | undefined) => {
        for (const key of cameraKeyVariants(value)) set.add(key);
      };
      add(camera.uid);
      add(camera.cameraUid ?? null);
      add(camera.streamId ?? null);
      add(camera.driverCameraId ?? null);
      add(camera.hardwareId ?? null);
      add(camera.streamAlias ?? null);
      return Array.from(set.values());
    };
    const keysForViewerCamera = (camera: (typeof viewerCameras)[number]): string[] => {
      const set = new Set<string>(keysForRigCamera(camera));
      for (const source of profile.viewOverlaySources) {
        const sourceKeySet = new Set<string>();
        const addSource = (value: string | null | undefined) => {
          for (const key of cameraKeyVariants(value)) sourceKeySet.add(key);
        };
        addSource(source.id);
        addSource(source.cameraUid);
        addSource(source.streamId);
        addSource(source.cameraPath);
        for (const key of source.cameraKeys ?? []) addSource(key);
        const matches = Array.from(sourceKeySet.values()).some((key) => set.has(key));
        if (matches) {
          for (const key of sourceKeySet.values()) set.add(key);
        }
      }
      return Array.from(set.values());
    };
    const findRigCamera = (keys: string[]) => {
      const lookup = new Set(keys);
      for (const camera of core.state.rigLayoutState.layout.cameras) {
        const candidates = keysForRigCamera(camera);
        if (candidates.some((candidate) => lookup.has(candidate))) {
          return camera;
        }
      }
      return null;
    };
    for (const camera of viewerCameras) {
      const keys = keysForViewerCamera(camera);
      if (keys.some((key) => Boolean(out[key]))) continue;
      const rigCamera = findRigCamera(keys);
      const pose = rigCamera?.pose ?? null;
      const robotFromCamera = rigPoseToViewerTransform(pose);
      let fieldFromCamera: PoseTransform | null = null;
      if (robotFromCamera) {
        fieldFromCamera = composeTransforms(solverRobotTransform, robotFromCamera);
      } else {
        const lookup = new Set(keys);
        const sourcePool = [...profile.viewOverlaySources, ...profile.selectedSources];
        const matchedSource = sourcePool.find((source) => sourceKeys(source).some((key) => lookup.has(key))) ?? null;
        if (matchedSource && isImuSource(matchedSource)) {
          fieldFromCamera = solverRobotTransform;
        }
      }
      if (!fieldFromCamera) continue;
      const transform = { position: fieldFromCamera.position, quaternion: fieldFromCamera.quaternion };
      for (const key of keys) out[key] = transform;
    }
    return Object.keys(out).length > 0 ? out : baseTransforms;
  });

  const viewerRenderableCameras = $derived.by(() =>
    buildViewerRenderableCameras({
      baseFrame: base.baseFrame,
      viewerCameras,
      viewerCameraTransforms,
      viewOverlaySources: profile.viewOverlaySources,
      selectedSources: profile.selectedSources
    })
  );

  $effect(() => {
    if (base.baseFrame !== 'field') {
      core.state.lastViewerCameraTransforms = null;
      core.state.lastViewerRenderableCameras = [];
      return;
    }
    const transforms = viewerCameraTransforms ?? null;
    if (transforms && Object.keys(transforms).length > 0) {
      core.state.lastViewerCameraTransforms = transforms;
    }
    if (viewerRenderableCameras.length > 0) {
      core.state.lastViewerRenderableCameras = viewerRenderableCameras;
    }
  });

  const viewerCameraTransformsForRender = $derived.by(() => {
    if (base.baseFrame !== 'field') return viewerCameraTransforms;
    const live = viewerCameraTransforms ?? null;
    if (live && Object.keys(live).length > 0) return live;
    return core.state.lastViewerCameraTransforms;
  });
  const viewerRenderableCamerasForRender = $derived.by(() => {
    if (base.baseFrame !== 'field') return viewerRenderableCameras;
    return viewerRenderableCameras.length > 0 ? viewerRenderableCameras : core.state.lastViewerRenderableCameras;
  });
  const viewerCameraGhostActive = $derived.by(() => {
    if (base.baseFrame !== 'field') return false;
    const liveTransforms = viewerCameraTransforms ?? null;
    const hasLiveTransforms = Boolean(liveTransforms && Object.keys(liveTransforms).length > 0);
    const hasLiveCameras = viewerRenderableCameras.length > 0;
    const ghostTransforms = core.state.lastViewerCameraTransforms ?? null;
    const hasGhostTransforms = Boolean(ghostTransforms && Object.keys(ghostTransforms).length > 0);
    const hasGhostCameras = core.state.lastViewerRenderableCameras.length > 0;
    if (!hasGhostTransforms || !hasGhostCameras) return false;
    return !hasLiveTransforms || !hasLiveCameras;
  });
  const viewerShowCameras = $derived.by(() => viewerRenderableCamerasForRender.length > 0);
  const viewerRobotTransform = $derived.by(() =>
    buildViewerRobotTransform({
      baseFrame: base.baseFrame,
      showRobotContext: core.state.showRobotContext,
      coordinateSpace: core.state.coordinateSpace,
      viewProfileOverlays,
      solverRobotTransform
    })
  );
  const viewerRobotTransformForGhost = $derived.by(() => {
    if (viewerRobotTransform) return viewerRobotTransform;
    if (base.baseFrame === 'field') return solverRobotTransform;
    return null;
  });
  const viewerShowRobot = $derived.by(() =>
    core.state.coordinateSpace === 'camera_in_field'
      ? false
      : buildViewerShowRobot({
          baseFrame: base.baseFrame,
          showRobotContext: core.state.showRobotContext,
          coordinateSpace: core.state.coordinateSpace
        })
  );

  const cameraPovOptionsBase = $derived.by(() =>
    buildCameraPovOptionsBase(profile.viewProfiles, profile.enabledSourcesForProfile)
  );
  const cameraPovByOptionId = $derived.by(() =>
    buildCameraPovByOptionId({
      cameraPovOptionsBase,
      profiles: core.profiles.current,
      sources: core.state.sources,
      outputsForProfile: profile.outputsForProfile,
      rigCameras: core.state.rigLayoutState.layout.cameras,
      coordinateSpace: core.state.coordinateSpace,
      cameraIntrinsicsByKey: base.cameraIntrinsicsByKey
    })
  );
  const cameraPovOptions = $derived.by(() =>
    buildCameraPovOptions(cameraPovOptionsBase, cameraPovByOptionId, core.state.lastCameraPovByOptionId)
  );
  const selectedCameraPov = $derived.by(() =>
    resolveSelectedCameraPov(
      core.state.cameraPovSelectionId,
      core.state.ROBOT_FOLLOW_POV_OPTION_ID,
      cameraPovByOptionId,
      core.state.lastCameraPovByOptionId
    )
  );
  const selectedLiveCameraPov = $derived.by(() =>
    resolveSelectedLiveCameraPov(core.state.cameraPovSelectionId, core.state.ROBOT_FOLLOW_POV_OPTION_ID, cameraPovByOptionId)
  );
  const viewerCameraPovEnabled = $derived.by(() => Boolean(selectedCameraPov));
  const viewerCameraPovTransform = $derived.by<PoseTransform | null>(() => selectedCameraPov?.transform ?? null);
  const viewerCameraPovIntrinsics = $derived.by(() => selectedPovIntrinsics(selectedCameraPov, core.state.cameraPovFovMode));
  const viewerCameraPovApplyFov = $derived.by<boolean>(() => core.state.cameraPovFovMode !== 'none');
  const viewerCameraPovForwardSign = $derived.by<1 | -1>(() => {
    const selected = selectedCameraPov;
    const intrinsics = selectedPovIntrinsics(selected, core.state.cameraPovFovMode);
    if (!selected) return 1;
    const profileEntry = core.profiles.current.find((entry) => entry.id === selected.profileId) ?? null;
    if (!profileEntry) return 1;
    const outputs = profile.outputsForProfile(profileEntry);
    if (!outputs) return 1;
    const sourceById = new Map(core.state.sources.map((source) => [source.id, source]));
    const detectionsForSpace =
      core.state.coordinateSpace === 'camera_in_field' || core.state.coordinateSpace === 'robot_in_field'
        ? fieldDetectionsForOutputs(outputs, sourceById, core.state.rigLayoutState.layout.cameras, core.rigCameraForSource)
        : [];
    const detections = detectionsForSpace.filter((detection) => detection.sourceId === selected.sourceId);
    if (detections.length === 0) return 1;
    const scorePositive = detections.reduce(
      (count, detection) =>
        count + (detectionInsidePovFov(detection, core.state.coordinateSpace, selected, intrinsics, 1) ? 1 : 0),
      0
    );
    const scoreNegative = detections.reduce(
      (count, detection) =>
        count + (detectionInsidePovFov(detection, core.state.coordinateSpace, selected, intrinsics, -1) ? 1 : 0),
      0
    );
    if (scorePositive !== scoreNegative) {
      return scorePositive > scoreNegative ? 1 : -1;
    }
    let positive = 0;
    let negative = 0;
    for (const detection of detections) {
      const position = cameraFramePositionForDetection(detection, core.state.coordinateSpace, selected);
      const z = position?.[2];
      if (z == null || !Number.isFinite(z) || Math.abs(z) < 1e-6) continue;
      if (z > 0) positive += 1;
      else negative += 1;
    }
    return negative > positive ? -1 : 1;
  });
  const minimapPoseDot = $derived.by(() =>
    buildMinimapPoseDot({
      hasAnyViewsEnabled: profile.hasAnyViewsEnabled,
      viewerAccentColor: profile.viewerAccentColor,
      coordinateSpace: core.state.coordinateSpace,
      viewerRobotTransform,
      solverRobotTransform,
      selectedLiveCameraPov,
      viewerCameraTransforms,
      primaryCameraKey: profile.primaryCameraKey,
      viewerRenderableCameras,
      baseFrame: base.baseFrame
    })
  );

  $effect(() => {
    const current = cameraPovByOptionId;
    const next = mergeCameraPovStateCache(
      current,
      core.state.lastCameraPovByOptionId,
      cameraPovOptionsBase.map((option) => option.id)
    );
    if (next !== core.state.lastCameraPovByOptionId) {
      core.state.lastCameraPovByOptionId = next;
    }
  });
  $effect(() => {
    const selected = core.state.cameraPovSelectionId.trim();
    if (!selected) return;
    if (selected === core.state.ROBOT_FOLLOW_POV_OPTION_ID) return;
    if (cameraPovOptionsBase.some((option) => option.id === selected)) return;
    core.state.cameraPovSelectionId = '';
  });

  const viewerMarkers = $derived.by<LocalizationMarker[]>(() => core.state.liveMarkers);
  const detectedFieldTagDetections = $derived.by<ViewerDetectionPose[]>(() =>
    buildDetectedFieldTagDetections({
      baseFrame: base.baseFrame,
      coordinateSpace: core.state.coordinateSpace,
      selectedPov: selectedCameraPov,
      cameraPovFovMode: core.state.cameraPovFovMode,
      povForwardSign: viewerCameraPovForwardSign,
      viewProfiles: profile.viewProfiles,
      outputsForProfile: profile.outputsForProfile,
      profiles: core.profiles.current,
      profileIndexById: profile.profileIndexById,
      sources: core.state.sources,
      fieldDetectionsForOutputs: (outputs, sourceById) =>
        fieldDetectionsForOutputs(outputs, sourceById, core.state.rigLayoutState.layout.cameras, core.rigCameraForSource)
    })
  );
  const referenceMarkersForViewer = $derived.by<LocalizationMarker[]>(() =>
    buildReferenceMarkersForViewer({
      baseFrame: base.baseFrame,
      activeFieldMapDoc: base.activeFieldMapDoc,
      detectedFieldTagDetections
    })
  );
  const viewerTagLineMarkers = $derived.by<LocalizationMarker[]>(() =>
    buildViewerTagLineMarkers({
      baseFrame: base.baseFrame,
      showTagLines: core.state.showTagLines,
      activeFieldMapDoc: base.activeFieldMapDoc,
      detectedFieldTagDetections,
      sources: core.state.sources,
      liveMarkers: core.state.liveMarkers
    })
  );

  $effect(() => {
    const nowMs = Date.now();
    const next = buildLocalTagPoseCache({
      coordinateSpace: core.state.coordinateSpace,
      lastLocalTagPoseByKey: core.state.lastLocalTagPoseByKey,
      nowMs,
      lingerMs: LOCAL_TAG_POSE_LINGER_MS,
      refreshMs: LOCAL_TAG_POSE_REFRESH_MS,
      viewProfilesWithSources: profile.viewProfilesWithSources,
      outputsForProfile: profile.outputsForProfile,
      profiles: core.profiles.current,
      profileIndexById: profile.profileIndexById
    });
    if (next !== core.state.lastLocalTagPoseByKey) {
      core.state.lastLocalTagPoseByKey = next;
    }
  });
  $effect(() => {
    const nowMs = Date.now();
    const next = buildFieldSpacePoseCache({
      coordinateSpace: core.state.coordinateSpace,
      lastFieldSpacePoseByProfileSpace: core.state.lastFieldSpacePoseByProfileSpace,
      nowMs,
      lingerMs: FIELD_POSE_LINGER_MS,
      refreshMs: FIELD_POSE_REFRESH_MS,
      viewProfilesWithSources: profile.viewProfilesWithSources,
      outputsForProfile: profile.outputsForProfile
    });
    if (next !== core.state.lastFieldSpacePoseByProfileSpace) {
      core.state.lastFieldSpacePoseByProfileSpace = next;
    }
  });
  const targetSpaceOverlay = $derived.by(() =>
    buildTargetSpaceOverlay({
      showOutputsOverlay: core.state.showOutputsOverlay,
      coordinateSpace: core.state.coordinateSpace,
      baseFrame: base.baseFrame,
      viewProfilesWithSources: profile.viewProfilesWithSources,
      outputsForProfile: profile.outputsForProfile,
      lastLocalTagPoseByKey: core.state.lastLocalTagPoseByKey,
      lastFieldSpacePoseByProfileSpace: core.state.lastFieldSpacePoseByProfileSpace,
      profiles: core.profiles.current,
      profileIndexById: profile.profileIndexById,
      activeFieldDimensions: base.activeFieldDimensions,
      fieldMapDocs: core.state.fieldMapDocs
    })
  );

  $effect(() => {
    const isFieldSpace = core.state.coordinateSpace === 'camera_in_field' || core.state.coordinateSpace === 'robot_in_field';
    if (isFieldSpace) {
      core.state.rawMarkers = [];
      return;
    }
    const activeProfileId = core.activeProfile.current?.id ?? null;
    const activeSolverKey = profile.activeSolverConfig?.id ?? core.state.activeSolverId;
    const povIntrinsics = selectedPovIntrinsics(selectedCameraPov, core.state.cameraPovFovMode);
    const povProfileId = selectedCameraPov?.profileId ?? null;
    const povSourceId = selectedCameraPov?.sourceId ?? null;
    const sourceById = new Map(core.state.sources.map((source) => [source.id, source]));
    const detections = profile.viewProfiles.flatMap<ViewerDetectionPose>((profileEntry) => {
      const resp =
        core.state.solveResponsesByProfile[profileEntry.id] ??
        (profileEntry.id === core.state.solveResponse?.profileId ? core.state.solveResponse : null);
      const solvers = resp?.solvers ?? [];
      const outputs =
        profileEntry.id === activeProfileId && activeSolverKey
          ? solvers.find((solver) => solver.id === activeSolverKey)?.outputs ?? solvers[0]?.outputs ?? null
          : solvers[0]?.outputs ?? null;
      const color = core.PROFILE_COLORS[profile.profileIndexById.get(profileEntry.id) ?? 0] ?? null;
      const poses =
        core.state.coordinateSpace === 'camera_in_field' || core.state.coordinateSpace === 'robot_in_field'
          ? fieldDetectionsForOutputs(outputs, sourceById, core.state.rigLayoutState.layout.cameras, core.rigCameraForSource)
          : detectionPosesForSpace(outputs, core.state.coordinateSpace);
      return poses.map((det) => ({ ...det, profileId: profileEntry.id, color }));
    });
    const scopedDetections = (() => {
      if (!povSourceId || !povProfileId) return detections;
      const scoped = detections.filter((detection) => detection.sourceId === povSourceId && detection.profileId === povProfileId);
      const inFov = scoped.filter((detection) =>
        detectionInsidePovFov(detection, core.state.coordinateSpace, selectedCameraPov, povIntrinsics, viewerCameraPovForwardSign)
      );
      return inFov.length > 0 ? inFov : scoped;
    })();
    const fallbackMarkersByTagId = new Map(
      (base.activeFieldMapDoc?.markers ?? []).map((marker) => [String(marker.id), marker] as const)
    );
    core.state.rawMarkers = core.markersFromDetections({
      detections: scopedDetections,
      sources: core.state.sources,
      selectedSources: profile.viewOverlaySources,
      colors: core.SOURCE_COLORS
    }).map((marker) => {
      if (marker.targetType === 'polygon') return marker;
      const tagId = marker.tagId;
      if (tagId == null || marker.tagBits) return marker;
      const fallback = fallbackMarkersByTagId.get(String(tagId)) ?? null;
      if (!fallback?.tagBits) return marker;
      return {
        ...marker,
        tagBits: fallback.tagBits,
        tagSize: marker.tagSize ?? fallback.sizeM
      } satisfies LocalizationMarker;
    });
  });

  const api = {
    get cameraPovOptions() {
      return cameraPovOptions;
    },
    get detectedFieldTagDetections() {
      return detectedFieldTagDetections;
    },
    get minimapPoseDot() {
      return minimapPoseDot;
    },
    get referenceMarkersForViewer() {
      return referenceMarkersForViewer;
    },
    get selectedCameraPov() {
      return selectedCameraPov;
    },
    get selectedLiveCameraPov() {
      return selectedLiveCameraPov;
    },
    get targetSpaceOverlay() {
      return targetSpaceOverlay;
    },
    get viewerCameraGhostActive() {
      return viewerCameraGhostActive;
    },
    get viewerCameraPovApplyFov() {
      return viewerCameraPovApplyFov;
    },
    get viewerCameraPovEnabled() {
      return viewerCameraPovEnabled;
    },
    get viewerCameraPovForwardSign() {
      return viewerCameraPovForwardSign;
    },
    get viewerCameraPovIntrinsics() {
      return viewerCameraPovIntrinsics;
    },
    get viewerCameraPovTransform() {
      return viewerCameraPovTransform;
    },
    get viewerCameraTransforms() {
      return viewerCameraTransforms;
    },
    get viewerCameraTransformsForRender() {
      return viewerCameraTransformsForRender;
    },
    get viewerMarkers() {
      return viewerMarkers;
    },
    get viewerRenderableCameras() {
      return viewerRenderableCameras;
    },
    get viewerRenderableCamerasForRender() {
      return viewerRenderableCamerasForRender;
    },
    get viewerRobotTransform() {
      return viewerRobotTransform;
    },
    get viewerRobotTransformForGhost() {
      return viewerRobotTransformForGhost;
    },
    get viewerShowCameras() {
      return viewerShowCameras;
    },
    get viewerShowRobot() {
      return viewerShowRobot;
    },
    get viewProfileOverlays() {
      return viewProfileOverlays;
    },
    get viewerTagLineMarkers() {
      return viewerTagLineMarkers;
    }
  };

  return Object.defineProperties(api, Object.getOwnPropertyDescriptors(base)) as typeof api &
    typeof base;
}

export type LocalizationPageRouteViewerState = ReturnType<typeof createLocalizationPageRouteViewerState>;
