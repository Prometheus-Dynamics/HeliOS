import type { CodecInfo, Mode } from '$lib/api/httpClient';
import { deriveStreamCodecSelections } from './cameraStreamConfigBuilder';
import {
  cameraIntervalsForSelection,
  cameraResolutionsForFormat,
  firstCameraFormat,
  firstCameraResolution,
  syncCameraModeSelectionState,
  type CameraModeSelectorDeps
} from './cameraModeSelectors';

export type StreamSelectionMode = 'auto' | 'manual';

export type CameraStreamEditorState = {
  selectedBackendIndex: number;
  selectedModeKey: string | null;
  selectedFormat: string;
  selectedResolution: string;
  selectedIntervalIdx: number;
  shadowRecorderEnabled: boolean;
  encoderImpl: string | null;
  decoderImpl: string | null;
  encoderEnabled: boolean;
  decoderEnabled: boolean;
  encoderSelectionMode: StreamSelectionMode;
  decoderSelectionMode: StreamSelectionMode;
};

type EditorModeDeps = CameraModeSelectorDeps;

type BackendSelectedAction = {
  type: 'backend_selected';
  backendIndex: number;
  backendKind: string | null;
  modes: Mode[];
};

type FormatSelectedAction = {
  type: 'format_selected';
  format: string;
  modes: Mode[];
};

type ResolutionSelectedAction = {
  type: 'resolution_selected';
  resolution: string;
  modes: Mode[];
};

type IntervalSelectedAction = {
  type: 'interval_selected';
  intervalIndex: number;
};

type EncoderSelectedAction = {
  type: 'encoder_selected';
  value: string | null;
};

type DecoderSelectedAction = {
  type: 'decoder_selected';
  value: string | null;
};

type CameraStreamEditorAction =
  | BackendSelectedAction
  | FormatSelectedAction
  | ResolutionSelectedAction
  | IntervalSelectedAction
  | EncoderSelectedAction
  | DecoderSelectedAction;

export type CameraStreamCodecSyncInput = {
  state: CameraStreamEditorState;
  encoders: CodecInfo[];
  decoders: CodecInfo[];
  resolvedEncoderId?: string | null;
  resolvedDecoderId?: string | null;
  requestedEncoderId?: string | null;
  requestedDecoderId?: string | null;
  defaultEncoderId?: string | null;
  decoderDefaultIdsByCaptureFormat?: Record<string, string>;
};

export function reduceCameraStreamEditorState(
  state: CameraStreamEditorState,
  action: CameraStreamEditorAction,
  deps: EditorModeDeps
): CameraStreamEditorState {
  switch (action.type) {
    case 'backend_selected': {
      const nextFormat = firstCameraFormat(action.modes);
      const nextResolution = firstCameraResolution(action.modes, nextFormat);
      return syncCameraModeSelectionState(
        {
          ...state,
          selectedBackendIndex: action.backendIndex,
          shadowRecorderEnabled: String(action.backendKind ?? '').trim().toLowerCase() !== 'file',
          selectedFormat: nextFormat,
          selectedResolution: nextResolution,
          selectedIntervalIdx: 0
        },
        action.modes,
        deps
      );
    }
    case 'format_selected': {
      const nextResolutionList = cameraResolutionsForFormat(action.modes, action.format);
      const nextResolution = nextResolutionList.includes(state.selectedResolution)
        ? state.selectedResolution
        : firstCameraResolution(action.modes, action.format);
      return syncCameraModeSelectionState(
        {
          ...state,
          selectedFormat: action.format,
          selectedResolution: nextResolution,
          selectedIntervalIdx: 0
        },
        action.modes,
        deps
      );
    }
    case 'resolution_selected':
      return syncCameraModeSelectionState(
        {
          ...state,
          selectedResolution: action.resolution,
          selectedIntervalIdx: 0
        },
        action.modes,
        deps
      );
    case 'interval_selected':
      return {
        ...state,
        selectedIntervalIdx: Math.max(0, Math.trunc(action.intervalIndex))
      };
    case 'encoder_selected': {
      const nextValue = typeof action.value === 'string' && action.value.trim().length ? action.value.trim() : null;
      return {
        ...state,
        encoderEnabled: Boolean(nextValue),
        encoderImpl: nextValue,
        encoderSelectionMode: 'manual'
      };
    }
    case 'decoder_selected': {
      const nextValue = typeof action.value === 'string' && action.value.trim().length ? action.value.trim() : null;
      return {
        ...state,
        decoderEnabled: Boolean(nextValue),
        decoderImpl: nextValue,
        decoderSelectionMode: 'manual'
      };
    }
  }
}

export function syncCameraStreamEditorCodecs(input: CameraStreamCodecSyncInput): CameraStreamEditorState {
  const { state } = input;
  const selections = deriveStreamCodecSelections({
    encoders: input.encoders,
    decoders: input.decoders,
    encoderImpl: state.encoderImpl,
    decoderImpl: state.decoderImpl,
    resolvedEncoderId: input.resolvedEncoderId ?? null,
    resolvedDecoderId: input.resolvedDecoderId ?? null,
    requestedEncoderId: input.requestedEncoderId ?? null,
    requestedDecoderId: input.requestedDecoderId ?? null,
    defaultEncoderId: input.defaultEncoderId ?? null,
    selectedFormat: state.selectedFormat,
    decoderDefaultIdsByCaptureFormat: input.decoderDefaultIdsByCaptureFormat ?? {},
    encoderSelectionMode: state.encoderSelectionMode,
    decoderSelectionMode: state.decoderSelectionMode
  });

  return {
    ...state,
    encoderImpl:
      state.encoderSelectionMode === 'manual' ? state.encoderImpl : selections.encoderImpl,
    decoderImpl:
      state.decoderSelectionMode === 'manual' ? state.decoderImpl : selections.decoderImpl
  };
}

export function applyCameraStreamModeSelection(
  state: CameraStreamEditorState,
  modes: Mode[],
  deps: EditorModeDeps
): CameraStreamEditorState {
  return syncCameraModeSelectionState(state, modes, deps);
}

export function clampCameraStreamIntervalSelection(
  state: CameraStreamEditorState,
  modes: Mode[]
): CameraStreamEditorState {
  const intervals = cameraIntervalsForSelection(modes, state.selectedFormat, state.selectedResolution);
  if (!intervals.length) {
    return {
      ...state,
      selectedIntervalIdx: 0
    };
  }
  return {
    ...state,
    selectedIntervalIdx: Math.max(0, Math.min(state.selectedIntervalIdx, intervals.length - 1))
  };
}

export function createInitialCameraStreamEditorState(): CameraStreamEditorState {
  return {
    selectedBackendIndex: 0,
    selectedModeKey: null,
    selectedFormat: '',
    selectedResolution: '',
    selectedIntervalIdx: 0,
    shadowRecorderEnabled: false,
    encoderImpl: null,
    decoderImpl: null,
    encoderEnabled: false,
    decoderEnabled: false,
    encoderSelectionMode: 'auto',
    decoderSelectionMode: 'auto'
  };
}

export function withAutoCameraStreamSelections(
  state: CameraStreamEditorState,
  next: Partial<Pick<CameraStreamEditorState, 'encoderImpl' | 'decoderImpl'>>
): CameraStreamEditorState {
  return {
    ...state,
    encoderImpl: state.encoderSelectionMode === 'manual' ? state.encoderImpl : next.encoderImpl ?? state.encoderImpl,
    decoderImpl: state.decoderSelectionMode === 'manual' ? state.decoderImpl : next.decoderImpl ?? state.decoderImpl
  };
}

export function cameraStreamSelectionTouched(mode: StreamSelectionMode): boolean {
  return mode === 'manual';
}

export function streamSelectionModeFromTouched(touched: boolean): StreamSelectionMode {
  return touched ? 'manual' : 'auto';
}

export function defaultCameraStreamEditorState(
  state: Partial<CameraStreamEditorState>
): CameraStreamEditorState {
  return {
    ...createInitialCameraStreamEditorState(),
    ...state
  };
}
