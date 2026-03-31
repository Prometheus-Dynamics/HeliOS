import type { CodecInfo, Interval, Mode, StreamInfo } from '$lib/api/httpClient';

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
  function normalizeFormat(value: unknown): string {
    const raw = typeof value === 'string' ? value : value == null ? '' : String(value);
    const trimmed = raw.trim();
    if (!trimmed) return '';
    return trimmed.split(/\s+/)[0]?.toUpperCase() ?? '';
  }

  function normalizeModes(value: unknown): Mode[] {
    if (!Array.isArray(value)) return [];
    return value.filter(Boolean) as Mode[];
  }

  function modeSignature(mode: Mode | null | undefined): string {
    if (!mode) return '';
    const fmt = normalizeFormat(mode?.format?.code ?? '');
    const res = mode?.format?.resolution ?? null;
    const resKey = res?.width && res?.height ? `${res.width}x${res.height}` : '';
    const color = typeof mode?.format?.color === 'string' ? mode.format.color : '';
    const idKey = deps.modeKey(mode?.id) ?? '';
    return [fmt, resKey, color, idKey].filter(Boolean).join('|');
  }

  function effectiveModes(): Mode[] {
    const backendModes =
      typeof state.backendModes === 'function'
        ? (state.backendModes as unknown as () => Mode[] | null)()
        : (state.backendModes ?? null);
    const backendList = normalizeModes(backendModes);
    const streamList = normalizeModes(state.stream?.descriptor?.modes ?? []);
    if (!backendList.length) return streamList;
    if (!streamList.length) return backendList;
    const merged: Mode[] = [];
    const seen = new Set<string>();
    for (const mode of [...backendList, ...streamList]) {
      const key = modeSignature(mode);
      if (key && seen.has(key)) continue;
      if (key) seen.add(key);
      merged.push(mode);
    }
    return merged;
  }

  function modeFormat(mode: Mode | null | undefined): string {
    const fmt = mode?.format ?? null;
    return normalizeFormat(fmt?.code ?? '');
  }

  function modeResolution(mode: Mode | null | undefined): string {
    const res = mode?.format?.resolution;
    if (!res?.width || !res?.height) return 'Unknown';
    return `${res.width}x${res.height}`;
  }

  function colorLabel(mode: Mode | null | undefined): string {
    const val = mode?.format?.color ?? '';
    return typeof val === 'string' && val.length ? val : 'Auto';
  }

  function modeLabel(mode: Mode | null | undefined): string {
    if (!mode) return 'Unknown';
    const fmt = modeFormat(mode);
    const res = modeResolution(mode);
    const color = colorLabel(mode);
    return [fmt, res, color].filter(Boolean).join(' • ');
  }

  function resolutionKey(mode: Mode | null | undefined): string {
    const res = mode?.format?.resolution;
    if (!res?.width || !res?.height) return '';
    return `${res.width}x${res.height}`;
  }

  function uniqueFormats(): string[] {
    const modes = effectiveModes();
    const seen = new Set<string>();
    const list: string[] = [];
    for (const mode of modes) {
      const fmt = modeFormat(mode);
      if (!fmt || seen.has(fmt)) continue;
      seen.add(fmt);
      list.push(fmt);
    }
    return list;
  }

  function resolutionsForFormat(fmt: string): string[] {
    const modes = effectiveModes();
    const key = normalizeFormat(fmt);
    const seen = new Set<string>();
    const list: string[] = [];
    for (const mode of modes) {
      if (modeFormat(mode) !== key) continue;
      const resKey = resolutionKey(mode);
      if (!resKey || seen.has(resKey)) continue;
      seen.add(resKey);
      list.push(resKey);
    }
    return list;
  }

  function intervalsForSelection(): Interval[] {
    const modes = effectiveModes();
    const formatKey = normalizeFormat(state.selectedFormat);
    const match = modes.find((m) => modeFormat(m) === formatKey && resolutionKey(m) === state.selectedResolution);
    return match?.intervals ?? [];
  }

  function fpsLabel(interval: Interval | undefined | null): string {
    if (!interval) return '—';
    const { numerator, denominator } = interval;
    if (!numerator || !denominator) return '—';
    const fps = denominator / numerator;
    return Number.isFinite(fps) ? `${fps.toFixed(2).replace(/\.00$/, '')} fps` : '—';
  }

  function firstFormat(): string {
    return uniqueFormats()[0] ?? '';
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
    const modes = effectiveModes();
    const formatKey = normalizeFormat(state.selectedFormat);
    const byFormatRes = modes.find((m) => modeFormat(m) === formatKey && resolutionKey(m) === state.selectedResolution);
    return byFormatRes ?? modes.find((m) => deps.modeKey(m.id) === state.selectedModeKey) ?? modes[0] ?? null;
  }

  function syncModeSelection(): void {
    const mode = currentMode();
    state.selectedModeKey = deps.modeKey(mode?.id) ?? null;
    const normalizedFormat = normalizeFormat(state.selectedFormat);
    if (!normalizedFormat) {
      state.selectedFormat = modeFormat(mode);
    } else if (state.selectedFormat !== normalizedFormat) {
      state.selectedFormat = normalizedFormat;
    }
    if (!state.selectedResolution) state.selectedResolution = modeResolution(mode);
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
