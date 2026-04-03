import type { Interval, Mode } from '$lib/api/client';

export type CameraModeSelectorDeps = {
  modeKey: (value: unknown) => string | null;
};

type ModeSelectionState = {
  selectedModeKey: string | null;
  selectedFormat: string;
  selectedResolution: string;
};

export function normalizeCameraFormat(value: unknown): string {
  const raw = typeof value === 'string' ? value : value == null ? '' : String(value);
  const trimmed = raw.trim();
  if (!trimmed) return '';
  return trimmed.split(/\s+/)[0]?.toUpperCase() ?? '';
}

export function normalizeCameraModes(value: unknown): Mode[] {
  if (!Array.isArray(value)) return [];
  return value.filter(Boolean) as Mode[];
}

export function cameraModeSignature(mode: Mode | null | undefined, deps: CameraModeSelectorDeps): string {
  if (!mode) return '';
  const fmt = normalizeCameraFormat(mode?.format?.code ?? '');
  const res = mode?.format?.resolution ?? null;
  const resKey = res?.width && res?.height ? `${res.width}x${res.height}` : '';
  const color = typeof mode?.format?.color === 'string' ? mode.format.color : '';
  const idKey = deps.modeKey(mode?.id) ?? '';
  return [fmt, resKey, color, idKey].filter(Boolean).join('|');
}

export function effectiveCameraModes(
  backendModes: unknown,
  streamModes: unknown,
  deps: CameraModeSelectorDeps
): Mode[] {
  const backendList = normalizeCameraModes(backendModes);
  const streamList = normalizeCameraModes(streamModes);
  if (!backendList.length) return streamList;
  if (!streamList.length) return backendList;
  const merged: Mode[] = [];
  const seen = new Set<string>();
  for (const mode of [...backendList, ...streamList]) {
    const key = cameraModeSignature(mode, deps);
    if (key && seen.has(key)) continue;
    if (key) seen.add(key);
    merged.push(mode);
  }
  return merged;
}

export function cameraModeFormat(mode: Mode | null | undefined): string {
  const fmt = mode?.format ?? null;
  return normalizeCameraFormat(fmt?.code ?? '');
}

export function cameraModeResolution(mode: Mode | null | undefined): string {
  const res = mode?.format?.resolution;
  if (!res?.width || !res?.height) return 'Unknown';
  return `${res.width}x${res.height}`;
}

export function cameraModeColorLabel(mode: Mode | null | undefined): string {
  const val = mode?.format?.color ?? '';
  return typeof val === 'string' && val.length ? val : 'Auto';
}

export function cameraModeLabel(mode: Mode | null | undefined): string {
  if (!mode) return 'Unknown';
  const fmt = cameraModeFormat(mode);
  const res = cameraModeResolution(mode);
  const color = cameraModeColorLabel(mode);
  return [fmt, res, color].filter(Boolean).join(' • ');
}

export function cameraModeResolutionKey(mode: Mode | null | undefined): string {
  const res = mode?.format?.resolution;
  if (!res?.width || !res?.height) return '';
  return `${res.width}x${res.height}`;
}

export function uniqueCameraFormats(modes: Mode[]): string[] {
  const seen = new Set<string>();
  const list: string[] = [];
  for (const mode of modes) {
    const fmt = cameraModeFormat(mode);
    if (!fmt || seen.has(fmt)) continue;
    seen.add(fmt);
    list.push(fmt);
  }
  return list;
}

export function cameraResolutionsForFormat(modes: Mode[], format: string): string[] {
  const key = normalizeCameraFormat(format);
  const seen = new Set<string>();
  const list: string[] = [];
  for (const mode of modes) {
    if (cameraModeFormat(mode) !== key) continue;
    const resolution = cameraModeResolutionKey(mode);
    if (!resolution || seen.has(resolution)) continue;
    seen.add(resolution);
    list.push(resolution);
  }
  return list;
}

export function cameraIntervalsForSelection(
  modes: Mode[],
  selectedFormat: string,
  selectedResolution: string
): Interval[] {
  const formatKey = normalizeCameraFormat(selectedFormat);
  const match = modes.find(
    (mode) => cameraModeFormat(mode) === formatKey && cameraModeResolutionKey(mode) === selectedResolution
  );
  return match?.intervals ?? [];
}

export function cameraFpsLabel(interval: Interval | undefined | null): string {
  if (!interval) return '—';
  const { numerator, denominator } = interval;
  if (!numerator || !denominator) return '—';
  const fps = denominator / numerator;
  return Number.isFinite(fps) ? `${fps.toFixed(2).replace(/\.00$/, '')} fps` : '—';
}

export function firstCameraFormat(modes: Mode[]): string {
  return uniqueCameraFormats(modes)[0] ?? '';
}

export function firstCameraResolution(modes: Mode[], format: string): string {
  return cameraResolutionsForFormat(modes, format)[0] ?? '';
}

export function currentCameraMode(
  modes: Mode[],
  state: ModeSelectionState,
  deps: CameraModeSelectorDeps
): Mode | null {
  const formatKey = normalizeCameraFormat(state.selectedFormat);
  const byFormatRes = modes.find(
    (mode) => cameraModeFormat(mode) === formatKey && cameraModeResolutionKey(mode) === state.selectedResolution
  );
  return byFormatRes ?? modes.find((mode) => deps.modeKey(mode.id) === state.selectedModeKey) ?? modes[0] ?? null;
}

export function syncCameraModeSelectionState<T extends ModeSelectionState>(
  state: T,
  modes: Mode[],
  deps: CameraModeSelectorDeps
): T {
  const mode = currentCameraMode(modes, state, deps);
  const normalizedFormat = normalizeCameraFormat(state.selectedFormat);
  return {
    ...state,
    selectedModeKey: deps.modeKey(mode?.id) ?? null,
    selectedFormat: !normalizedFormat ? cameraModeFormat(mode) : normalizedFormat,
    selectedResolution: state.selectedResolution || cameraModeResolution(mode)
  };
}
