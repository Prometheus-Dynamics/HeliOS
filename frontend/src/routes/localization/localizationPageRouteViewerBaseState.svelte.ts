import { openStreamMetricsSocket } from '$lib/api/streamMetrics';
import {
  buildActiveCustomField,
  buildActiveFieldDimensions,
  buildActiveFieldMapBitsStatus,
  buildActiveFieldOrigin,
  buildFieldSceneTransform,
  buildOriginFromFieldCenterForEditor,
  buildSourceStatusRows
} from '$lib/features/localization/page/localizationPageViewHelpers';
import type { CustomField } from '$lib/features/localization/types';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import { composeTransforms, quaternionToEulerDegreesXYZ } from '$lib/features/localization/poseMath';
import { extractPovIntrinsicsSet, imuSourcePriority, isImuSource, parseImuRotationSample, streamIdentityKeys } from './localizationCameraPovUtils';
import type { LocalizationPageRouteCore } from './localizationPageRouteCore.svelte';
import type { LocalizationPageRouteProfileState } from './localizationPageRouteProfileState.svelte';

export function createLocalizationPageRouteViewerBaseState(
  core: LocalizationPageRouteCore,
  profile: LocalizationPageRouteProfileState
) {
  const cameraIntrinsicsByKey = $derived.by(() => {
    const out: Record<string, ReturnType<typeof extractPovIntrinsicsSet>> = {};
    for (const stream of core.state.streamInfos) {
      const intrinsics = extractPovIntrinsicsSet(stream);
      if (!intrinsics) continue;
      for (const key of streamIdentityKeys(stream)) {
        out[key] = intrinsics;
      }
    }
    return out;
  });

  $effect(() => {
    if (!core.browser) return;
    if (core.state.sources.length === 0) {
      core.state.streamInfos = [];
      return;
    }
    void core.loadStreamsSnapshot();
  });

  const baseFrame = $derived.by(() => {
    if (core.state.coordinateSpace === 'tag_in_camera' || core.state.coordinateSpace === 'camera_in_tag') {
      return 'camera' as const;
    }
    if (core.state.coordinateSpace === 'tag_in_robot' || core.state.coordinateSpace === 'robot_in_tag') {
      return 'robot' as const;
    }
    return 'field' as const;
  });

  const selectedFieldMapId = $derived.by<string | null>(() => {
    const mapId = core.activeProfile.current?.fieldMapId ?? null;
    return typeof mapId === 'string' && mapId.trim() ? mapId.trim() : null;
  });

  const calibratedCameraIds = $derived.by(() => {
    const ids = new Set<string>();
    for (const camera of core.state.rigLayoutState.layout.cameras ?? []) {
      if (!camera.pose) continue;
      if (camera.uid) ids.add(camera.uid);
      if (camera.cameraUid) ids.add(camera.cameraUid);
      if (camera.hardwareId) ids.add(camera.hardwareId);
      if (camera.streamId) ids.add(camera.streamId);
      if (camera.streamAlias) ids.add(camera.streamAlias);
      if (camera.driverCameraId) ids.add(camera.driverCameraId);
    }
    return ids;
  });

  const uncalibratedSources = $derived.by(() =>
    profile.selectedSources.filter((source) => !core.isSourceCalibrated(source, calibratedCameraIds))
  );
  const calibrationReady = $derived.by(() => profile.selectedSources.length > 0 && uncalibratedSources.length === 0);
  const fieldSpaceAllowed = $derived.by(() => Boolean(selectedFieldMapId) && calibrationReady);

  $effect(() => {
    if (profile.availableCoordinateSpaces.length === 0) return;
    if (!profile.availableCoordinateSpaces.includes(core.state.coordinateSpace)) {
      if (fieldSpaceAllowed && profile.availableCoordinateSpaces.includes('camera_in_field')) {
        core.state.coordinateSpace = 'camera_in_field';
      } else if (fieldSpaceAllowed && profile.availableCoordinateSpaces.includes('robot_in_field')) {
        core.state.coordinateSpace = 'robot_in_field';
      } else if (profile.availableCoordinateSpaces.includes('tag_in_robot')) {
        core.state.coordinateSpace = 'tag_in_robot';
      } else if (profile.availableCoordinateSpaces.includes('tag_in_camera')) {
        core.state.coordinateSpace = 'tag_in_camera';
      } else {
        core.state.coordinateSpace = profile.availableCoordinateSpaces[0] ?? 'tag_in_camera';
      }
    }
  });

  const activeFieldMapDoc = $derived.by(() => {
    const id = selectedFieldMapId;
    return id ? core.state.fieldMapDocs[id] ?? null : null;
  });
  const activeFieldMapBitsStatus = $derived.by(() => buildActiveFieldMapBitsStatus(activeFieldMapDoc));
  const selectedCustomField = $derived.by<CustomField | null>(() => {
    if (core.state.viewMode !== 'custom-field') return null;
    const id = core.state.selectedCustomFieldId;
    return id ? core.customFields.current.find((entry) => entry.id === id) ?? null : null;
  });
  const activeCustomField = $derived.by(() =>
    buildActiveCustomField({ viewMode: core.state.viewMode, selectedCustomField, activeFieldMapDoc })
  );
  const activeFieldDimensions = $derived.by(() =>
    buildActiveFieldDimensions({ viewMode: core.state.viewMode, activeCustomField })
  );
  const activeFieldOrigin = $derived.by(() =>
    buildActiveFieldOrigin({
      viewMode: core.state.viewMode,
      profileFieldOrigin: profile.profileFieldOrigin,
      selectedCustomField,
      selectedCustomFieldOriginId: core.state.selectedCustomFieldOriginId,
      activeFieldDimensions
    })
  );
  const fieldSceneTransform = $derived.by(() => buildFieldSceneTransform({ baseFrame, activeFieldOrigin }));
  const originFromFieldCenterForEditor = $derived.by(() => buildOriginFromFieldCenterForEditor(activeFieldOrigin));
  const viewerSceneTransform = $derived.by(() => (baseFrame === 'field' ? fieldSceneTransform ?? null : null));

  $effect(() => {
    if (core.state.viewMode !== 'custom-field') return;
    const field = core.state.selectedCustomFieldId
      ? core.customFields.current.find((entry) => entry.id === core.state.selectedCustomFieldId) ?? null
      : core.customFields.current[0] ?? null;
    if (!field) {
      core.state.selectedCustomFieldId = null;
      core.state.selectedCustomFieldOriginId = null;
      return;
    }
    if (field.id !== core.state.selectedCustomFieldId) {
      core.state.selectedCustomFieldId = field.id;
    }
    const origin = core.state.selectedCustomFieldOriginId
      ? field.origins.find((entry) => entry.id === core.state.selectedCustomFieldOriginId) ?? null
      : null;
    if (!origin) {
      core.state.selectedCustomFieldOriginId = field.origins[0]?.id ?? null;
    }
  });

  $effect(() => {
    const id = selectedFieldMapId;
    if (!id) return;
    void core.ensureFieldMapLoaded(id);
  });

  $effect(() => {
    const fieldSpaceActive =
      core.state.coordinateSpace === 'camera_in_field' || core.state.coordinateSpace === 'robot_in_field';
    if (!fieldSpaceActive) {
      if (core.state.viewMode !== 'isolated') {
        core.state.viewMode = 'isolated';
      }
      return;
    }
    if (!selectedFieldMapId) {
      core.state.viewMode = 'isolated';
      return;
    }
    const matchingField = core.customFields.current.find((field) => field.mapId === selectedFieldMapId) ?? null;
    if (core.state.viewMode !== 'frc-field') {
      core.state.viewMode = 'frc-field';
    }
    if (matchingField) {
      if (core.state.selectedCustomFieldId !== matchingField.id) {
        core.state.selectedCustomFieldId = matchingField.id;
      }
      if (
        !core.state.selectedCustomFieldOriginId ||
        !matchingField.origins.some((origin) => origin.id === core.state.selectedCustomFieldOriginId)
      ) {
        core.state.selectedCustomFieldOriginId = matchingField.origins[0]?.id ?? null;
      }
    }
  });

  $effect(() => {
    const next = selectedFieldMapId ?? '';
    if (core.state.fieldMapSelection !== next) {
      core.state.fieldMapSelection = next;
    }
  });

  const activeImuSource = $derived.by<LocalizationPipelineSource | null>(() => {
    const unique = new Map<string, LocalizationPipelineSource>();
    for (const source of profile.viewerSourcePool) {
      if (!isImuSource(source)) continue;
      unique.set(source.id, source);
    }
    const candidates = Array.from(unique.values());
    if (candidates.length === 0) return null;
    candidates.sort((left, right) => {
      const delta = imuSourcePriority(left) - imuSourcePriority(right);
      if (delta !== 0) return delta;
      const labelDelta = left.streamLabel.localeCompare(right.streamLabel);
      if (labelDelta !== 0) return labelDelta;
      return left.id.localeCompare(right.id);
    });
    return candidates[0] ?? null;
  });

  $effect(() => {
    void profile.hasAnyFeedSources;
    if (!core.browser || !profile.hasAnyFeedSources || !activeImuSource) {
      if (!activeImuSource) {
        core.state.imuRotationSample = null;
      }
      core.state.imuRotationError = null;
      return;
    }
    if (core.state.imuRotationSample?.sourceId !== activeImuSource.id) {
      core.state.imuRotationSample = null;
    }
    const controller = new AbortController();
    void (async () => {
      try {
        const sample = await core.fetchPipelineOutputSample(
          activeImuSource.streamId,
          activeImuSource.pipelineId,
          activeImuSource.outputKey,
          controller.signal
        );
        if (controller.signal.aborted) return;
        if (!sample) {
          core.state.imuRotationError = `${activeImuSource.streamLabel}: no sample available`;
          return;
        }
        const parsed = parseImuRotationSample(sample.value);
        if (!parsed) {
          core.state.imuRotationError = `${activeImuSource.streamLabel}: sample has no rotation`;
          return;
        }
        core.state.imuRotationSample = {
          sourceId: activeImuSource.id,
          sourceLabel: activeImuSource.streamLabel || activeImuSource.cameraUid || activeImuSource.id,
          outputKey: activeImuSource.outputKey,
          roll: parsed.roll,
          pitch: parsed.pitch,
          yaw: parsed.yaw,
          quaternion: parsed.quaternion,
          translation: parsed.translation,
          sampleTimestampMs: parsed.sampleTimestampMs,
          receivedAtMs: Date.now()
        };
        core.state.imuRotationError = null;
      } catch (error) {
        if (controller.signal.aborted) return;
        core.state.imuRotationError = error instanceof Error ? error.message : 'Failed to fetch IMU sample';
      }
    })();
    return () => controller.abort();
  });

  const imuRotationOverlayData = $derived.by(() => {
    const sample = core.state.imuRotationSample;
    if (!sample) return null;
    return {
      sourceId: sample.sourceId,
      sourceLabel: sample.sourceLabel,
      outputKey: sample.outputKey,
      roll: sample.roll,
      pitch: sample.pitch,
      yaw: sample.yaw,
      quaternion: sample.quaternion,
      translation: sample.translation,
      sampleTimestampMs: sample.sampleTimestampMs,
      ageMs: Math.max(0, Date.now() - sample.receivedAtMs)
    };
  });
  const imuRotationStatusMessage = $derived.by<string | null>(() => {
    if (!activeImuSource) return 'No IMU source selected for this view';
    if (core.state.imuRotationError) return core.state.imuRotationError;
    if (!core.state.imuRotationSample || core.state.imuRotationSample.sourceId !== activeImuSource.id) {
      return 'Waiting for IMU sample...';
    }
    return null;
  });

  $effect(() => {
    const streamIds = core.state.showMetricsOverlay
      ? Array.from(
          new Set(
            profile.selectedSources
              .filter((source) => !isImuSource(source))
              .map((source) => source.streamId)
              .filter(Boolean)
          )
        )
      : [];

    for (const [streamId, cleanup] of Array.from(core.streamMetricsCleanup.entries())) {
      if (streamIds.includes(streamId)) continue;
      cleanup();
      core.streamMetricsCleanup.delete(streamId);
      ({
        streamMetricsById: core.state.streamMetricsById,
        streamMetricsUpdatedAtById: core.state.streamMetricsUpdatedAtById,
        streamMetricsErrorById: core.state.streamMetricsErrorById
      } = core.removeStreamMetrics({
        streamId,
        streamMetricsById: core.state.streamMetricsById,
        streamMetricsUpdatedAtById: core.state.streamMetricsUpdatedAtById,
        streamMetricsErrorById: core.state.streamMetricsErrorById
      }));
    }

    for (const streamId of streamIds) {
      if (core.streamMetricsCleanup.has(streamId)) continue;
      const cleanup = openStreamMetricsSocket(
        streamId,
        {
          onMetrics: (event) => {
            core.state.streamMetricsById = { ...core.state.streamMetricsById, [streamId]: event.metrics };
            core.state.streamMetricsUpdatedAtById = { ...core.state.streamMetricsUpdatedAtById, [streamId]: Date.now() };
            if (core.state.streamMetricsErrorById[streamId]) {
              const restErr = { ...core.state.streamMetricsErrorById };
              delete restErr[streamId];
              core.state.streamMetricsErrorById = restErr;
            }
          },
          onError: (error) => {
            core.state.streamMetricsErrorById = { ...core.state.streamMetricsErrorById, [streamId]: error.error };
          }
        },
        { intervalMs: 200 }
      );
      core.streamMetricsCleanup.set(streamId, cleanup);
    }
  });

  $effect(() => {
    if (!core.browser) return;
    const key = profile.primaryCameraKey;
    if (!key) {
      core.state.cameraPoseInputsKey = null;
      core.state.cameraPoseEditorError = null;
      return;
    }
    const origin = activeFieldOrigin;
    const originSignature = origin ? `${origin.id}:${origin.x}:${origin.z}:${origin.yawDeg}` : 'none';
    const inputsKey = `${key}|${originSignature}`;
    if (core.state.cameraPoseInputsKey === inputsKey) return;
    core.state.cameraPoseInputsKey = inputsKey;
    const centerFromCamera = core.cameraExtrinsicsTransform(key);
    const originFromCamera = composeTransforms(originFromFieldCenterForEditor, centerFromCamera);
    const euler = quaternionToEulerDegreesXYZ(originFromCamera.quaternion);
    core.state.cameraPoseXInput = core.formatMeters(originFromCamera.position[0], 'm', 3);
    core.state.cameraPoseYInput = core.formatMeters(originFromCamera.position[1], 'm', 3);
    core.state.cameraPoseZInput = core.formatMeters(originFromCamera.position[2], 'm', 3);
    core.state.cameraPoseRollDeg = euler.roll.toFixed(2);
    core.state.cameraPosePitchDeg = euler.pitch.toFixed(2);
    core.state.cameraPoseYawDeg = euler.yaw.toFixed(2);
    core.state.cameraPoseEditorError = null;
  });

  $effect(() => {
    let next = core.state.rawMarkers;
    if (baseFrame === 'camera') {
      next = core.applyDeviceSeparation({
        markers: next,
        separateCameras: core.state.separateCameras,
        selectedSources: profile.viewOverlaySources,
        primaryCameraKey: profile.primaryCameraKey
      });
    }
    core.state.liveMarkers = next;
  });

  const detectionsBySourceId = $derived.by<Record<string, number>>(() => {
    const counts: Record<string, number> = {};
    for (const entry of core.state.pollResults) {
      counts[entry.sourceId] = entry.detections;
    }
    return counts;
  });
  const sourceStatusRows = $derived.by(() =>
    buildSourceStatusRows({
      pollResults: core.state.pollResults,
      sources: core.state.sources,
      streamMetricsById: core.state.streamMetricsById,
      streamMetricsUpdatedAtById: core.state.streamMetricsUpdatedAtById,
      streamMetricsErrorById: core.state.streamMetricsErrorById,
      detectionsBySourceId,
      toNumber: core.toNumber
    })
  );
  const activeSolveMs = $derived.by<number | null>(() => {
    const solverMs = core.state.solveResponse?.timings?.solverMs;
    return typeof solverMs === 'number' && Number.isFinite(solverMs) ? solverMs : null;
  });
  const profileTimingRows = $derived.by(() =>
    core.profiles.current
      .map((profileEntry) => {
        const resp =
          core.state.solveResponsesByProfile[profileEntry.id] ??
          (profileEntry.id === core.state.solveResponse?.profileId ? core.state.solveResponse : null);
        if (!resp) return null;
        const timings = resp.timings;
        const finiteOrNull = (value: number | null | undefined): number | null =>
          typeof value === 'number' && Number.isFinite(value) ? value : null;
        return {
          profileId: profileEntry.id,
          label: profileEntry.name,
          active: profileEntry.id === (core.activeProfile.current?.id ?? null),
          visible: profileEntry.viewEnabled === true,
          solverMs: finiteOrNull(timings?.solverMs),
          engineMs: finiteOrNull(timings?.engineMs),
          totalMs: finiteOrNull(timings?.totalMs),
          sourceFetchMs: finiteOrNull(timings?.sourceFetchMs),
          sourceParseMs: finiteOrNull(timings?.sourceParseMs),
          cacheHit: timings?.cacheHit === true
        };
      })
      .filter((row): row is NonNullable<typeof row> => row !== null)
      .sort(
        (left, right) =>
          Number(right.active) - Number(left.active) ||
          Number(right.visible) - Number(left.visible) ||
          left.label.localeCompare(right.label)
      )
  );

  return {
    get activeCustomField() {
      return activeCustomField;
    },
    get activeFieldDimensions() {
      return activeFieldDimensions;
    },
    get activeFieldMapBitsStatus() {
      return activeFieldMapBitsStatus;
    },
    get activeFieldMapDoc() {
      return activeFieldMapDoc;
    },
    get activeFieldOrigin() {
      return activeFieldOrigin;
    },
    get activeImuSource() {
      return activeImuSource;
    },
    get activeSolveMs() {
      return activeSolveMs;
    },
    get baseFrame() {
      return baseFrame;
    },
    get calibrationReady() {
      return calibrationReady;
    },
    get calibratedCameraIds() {
      return calibratedCameraIds;
    },
    get cameraIntrinsicsByKey() {
      return cameraIntrinsicsByKey;
    },
    get fieldSceneTransform() {
      return fieldSceneTransform;
    },
    get fieldSpaceAllowed() {
      return fieldSpaceAllowed;
    },
    get imuRotationOverlayData() {
      return imuRotationOverlayData;
    },
    get imuRotationStatusMessage() {
      return imuRotationStatusMessage;
    },
    get originFromFieldCenterForEditor() {
      return originFromFieldCenterForEditor;
    },
    get profileTimingRows() {
      return profileTimingRows;
    },
    get selectedCustomField() {
      return selectedCustomField;
    },
    get selectedFieldMapId() {
      return selectedFieldMapId;
    },
    get sourceStatusRows() {
      return sourceStatusRows;
    },
    get uncalibratedSources() {
      return uncalibratedSources;
    },
    get viewerSceneTransform() {
      return viewerSceneTransform;
    }
  };
}

export type LocalizationPageRouteViewerBaseState = ReturnType<typeof createLocalizationPageRouteViewerBaseState>;
