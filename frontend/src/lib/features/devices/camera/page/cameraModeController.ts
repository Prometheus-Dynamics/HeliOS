import type { CodecInfo, Interval, Mode, StreamInfo } from '$lib/api/httpClient';
import {
  cameraIntervalsForSelection,
  cameraFpsLabel,
  cameraModeColorLabel,
  cameraModeFormat,
  cameraModeLabel,
  cameraModeResolution,
  cameraModeResolutionKey,
  cameraResolutionsForFormat,
  currentCameraMode,
  effectiveCameraModes,
  firstCameraFormat,
  syncCameraModeSelectionState,
  uniqueCameraFormats
} from './cameraModeSelectors';

export type ModeControllerState = {
  get stream(): StreamInfo | null;
  backendModes?: () => Mode[] | null;
  get selectedModeKey(): string | null;
  set selectedModeKey(value: string | null);
  get selectedFormat(): string;
  set selectedFormat(value: string);
  get selectedResolution(): string;
  set selectedResolution(value: string);
  get decoders(): CodecInfo[];
};

type ModeControllerDeps = {
  modeKey: (value: unknown) => string | null;
};

export function createCameraModeController(state: ModeControllerState, deps: ModeControllerDeps) {
  function effectiveModes(): Mode[] {
    const backendModes =
      typeof state.backendModes === 'function'
        ? (state.backendModes as unknown as () => Mode[] | null)()
        : (state.backendModes ?? null);
    return effectiveCameraModes(backendModes, state.stream?.descriptor?.modes ?? [], deps);
  }

  function modeFormat(mode: Mode | null | undefined): string {
    return cameraModeFormat(mode);
  }

  function modeResolution(mode: Mode | null | undefined): string {
    return cameraModeResolution(mode);
  }

  function colorLabel(mode: Mode | null | undefined): string {
    return cameraModeColorLabel(mode);
  }

  function modeLabel(mode: Mode | null | undefined): string {
    return cameraModeLabel(mode);
  }

  function resolutionKey(mode: Mode | null | undefined): string {
    return cameraModeResolutionKey(mode);
  }

  function uniqueFormats(): string[] {
    return uniqueCameraFormats(effectiveModes());
  }

  function resolutionsForFormat(fmt: string): string[] {
    return cameraResolutionsForFormat(effectiveModes(), fmt);
  }

  function intervalsForSelection(): Interval[] {
    return cameraIntervalsForSelection(effectiveModes(), state.selectedFormat, state.selectedResolution);
  }

  function fpsLabel(interval: Interval | undefined | null): string {
    return cameraFpsLabel(interval);
  }

  function firstFormat(): string {
    return firstCameraFormat(effectiveModes());
  }

  function firstResolution(): string {
    const fmt = firstFormat();
    return resolutionsForFormat(fmt)[0] ?? '';
  }

  function firstInterval(): string {
    const first = intervalsForSelection()[0];
    return first ? fpsLabel(first) : '';
  }

  function currentMode(): Mode | null {
    return currentCameraMode(
      effectiveModes(),
      {
        selectedModeKey: state.selectedModeKey,
        selectedFormat: state.selectedFormat,
        selectedResolution: state.selectedResolution
      },
      deps
    );
  }

  function syncModeSelection(): void {
    const next = syncCameraModeSelectionState(
      {
        selectedModeKey: state.selectedModeKey,
        selectedFormat: state.selectedFormat,
        selectedResolution: state.selectedResolution
      },
      effectiveModes(),
      deps
    );
    state.selectedModeKey = next.selectedModeKey;
    state.selectedFormat = next.selectedFormat;
    state.selectedResolution = next.selectedResolution;
  }

  return {
    effectiveModes,
    modeFormat,
    modeResolution,
    colorLabel,
    modeLabel,
    resolutionKey,
    uniqueFormats,
    resolutionsForFormat,
    intervalsForSelection,
    fpsLabel,
    firstFormat,
    firstResolution,
    firstInterval,
    currentMode,
    syncModeSelection
  };
}
