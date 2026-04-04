type AccessorTransform = {
  get?: (value: unknown) => unknown;
  set?: (value: unknown) => unknown;
};

type AccessorBindings<TTarget extends object, TKeys extends readonly (keyof TTarget & string)[]> = {
  [K in TKeys[number]]: TTarget[K];
};

function createAccessorBindings<TTarget extends object, const TKeys extends readonly (keyof TTarget & string)[]>(
  target: TTarget,
  keys: TKeys,
  transforms: Partial<{ [K in TKeys[number]]: AccessorTransform }> = {}
): AccessorBindings<TTarget, TKeys> {
  const allowedKeys = new Set<string>(keys);
  return new Proxy({} as AccessorBindings<TTarget, TKeys>, {
    get(_bindings, property) {
      if (typeof property !== 'string' || !allowedKeys.has(property)) return undefined;
      const key = property as TKeys[number];
      const value = target[key];
      return transforms[key]?.get ? transforms[key].get!(value) : value;
    },
    set(_bindings, property, value: unknown) {
      if (typeof property !== 'string' || !allowedKeys.has(property)) return false;
      const key = property as TKeys[number];
      (target as { [K in keyof TTarget]: TTarget[K] })[key] = (
        transforms[key]?.set ? transforms[key].set!(value) : value
      ) as TTarget[typeof key];
      return true;
    },
    has(_bindings, property) {
      return typeof property === 'string' && allowedKeys.has(property);
    },
    ownKeys() {
      return [...keys];
    },
    getOwnPropertyDescriptor(_bindings, property) {
      if (typeof property !== 'string' || !allowedKeys.has(property)) return undefined;
      return {
        configurable: true,
        enumerable: true
      };
    }
  });
}

const PIPELINE_STATE_KEYS = [
  'pipelineGridRows',
  'pipelineGridColumns',
  'pipelineGridSlots',
  'pipelineGridSlotOutputKeys',
  'assignedPipelineIds',
  'pipelineAssignDraft',
  'pipelineOutputByPipelineId',
  'selectedPipelineId',
  'selectedPipelineOutput',
  'pipelineLayoutTouched',
  'pipelineAssignmentsTouched'
] as const;

const STREAM_STATE_KEYS = [
  'activeTab',
  'cameraAlias',
  'selectedBackendIndex',
  'selectedModeKey',
  'selectedFormat',
  'selectedResolution',
  'selectedIntervalIdx',
  'libcameraTargetFps',
  'netcamTargetFps',
  'fileBackendFps',
  'fileBackendLoop',
  'fileBackendPathsText',
  'decoderImpl',
  'streamDefaults',
  'decoderDefaultIdsByCaptureFormat',
  'encoderImpl',
  'decoderEnabled',
  'encoderEnabled',
  'decoderSelectionMode',
  'encoderSelectionMode',
  'hostBuffer',
  'previewJpegQuality',
  'decoderFpsLimit',
  'decoderRotationDegrees',
  'decoderMirrorHorizontal',
  'shadowRecorderEnabled',
  'encoderFpsLimit',
  'encoderSettingsOpen',
  'encoderSettings',
  'streamCrop',
  'streamCropGuidesEnabled',
  'streamCropApplying',
  'streamCropError',
  'streamCropWarning',
  'streamCrosshair',
  'streamCrosshairEnabled',
  'streamCrosshairGuidesEnabled',
  'streamCrosshairApplying',
  'streamCrosshairError',
  'streamCrosshairWarning',
  'streamOrderingMode',
  'streamOrderingApplying',
  'streamOrderingError',
  'streamOrderingWarning',
  'controlsQuery',
  'controlState',
  'controlAppliedState',
  'controlBusy',
  'streamMetrics'
] as const;

const PIPELINE_MODAL_KEYS = [
  'pipelineGridRows',
  'pipelineGridColumns',
  'selectedPipelineOutput',
  'pipelineRemoveModalOpen',
  'pipelineRemoveCandidateId',
  'pipelineAssignModalOpen',
  'pipelineAssignQuery',
  'pipelineAssignDraft'
] as const;

type CameraPagePipelineBindingKey = (typeof PIPELINE_STATE_KEYS)[number] | (typeof PIPELINE_MODAL_KEYS)[number];

export type CameraPagePipelineBindingTarget = {
  [K in CameraPagePipelineBindingKey]: unknown;
};

export type CameraPageStreamBindingTarget = {
  [K in (typeof STREAM_STATE_KEYS)[number]]: unknown;
};

export function createCameraPageStateBindings({
  streamState,
  pipelineState,
  normalizeStreamOrderingMode
}: {
  streamState: CameraPageStreamBindingTarget;
  pipelineState: CameraPagePipelineBindingTarget;
  normalizeStreamOrderingMode: (value: unknown) => unknown;
}) {
  return {
    pipelineStateBindings: createAccessorBindings(pipelineState, PIPELINE_STATE_KEYS),
    streamBindings: createAccessorBindings(streamState, STREAM_STATE_KEYS, {
      streamCrosshairEnabled: { set: (value) => Boolean(value) },
      streamOrderingMode: { set: normalizeStreamOrderingMode }
    }),
    pipelineBindings: createAccessorBindings(pipelineState, PIPELINE_MODAL_KEYS)
  };
}
