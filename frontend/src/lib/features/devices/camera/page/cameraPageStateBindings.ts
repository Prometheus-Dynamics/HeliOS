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
  const bindings = {} as AccessorBindings<TTarget, TKeys>;
  for (const key of keys) {
    Object.defineProperty(bindings, key, {
      enumerable: true,
      configurable: true,
      get() {
        const value = target[key];
        return transforms[key]?.get ? transforms[key].get!(value) : value;
      },
      set(value: unknown) {
        (target as { [K in keyof TTarget]: TTarget[K] })[key] = (
          transforms[key]?.set ? transforms[key].set!(value) : value
        ) as TTarget[typeof key];
      }
    });
  }
  return bindings;
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

export type CameraPagePipelineBindingTarget = {
  [K in (typeof PIPELINE_STATE_KEYS)[number]]: unknown;
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
