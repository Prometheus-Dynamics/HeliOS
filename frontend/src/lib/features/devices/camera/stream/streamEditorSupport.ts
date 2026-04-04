import { toaster } from '$lib';
import { apiFetchResponse } from '$lib/api/core/http';
import { reportError } from '$lib/ui/errorPolicy';

export type BenchCodecStat = { implementation: string; avg_ms: number; avg_fps: number; errors?: number };
export type BenchFormatGroup = {
  format: string;
  capture_avg_fps: number;
  host_avg_fps: number;
  decoders?: BenchCodecStat[];
  encoders?: BenchCodecStat[];
};

export type StreamModeDescriptor = { id: string } & Record<string, unknown>;
export type StreamOrderingMode =
  | 'none'
  | 'largest_to_smallest'
  | 'smallest_to_largest'
  | 'top_most'
  | 'bottom_most'
  | 'left_most'
  | 'right_most'
  | 'top_left'
  | 'top_right'
  | 'bottom_left'
  | 'bottom_right'
  | 'center_most'
  | 'crosshair';

export const STREAM_CROP_MIN = -1;
export const STREAM_CROP_MAX = 1;
export const STREAM_CROP_STEP = 0.01;
export const STREAM_ORDERING_MODES: Array<{ value: StreamOrderingMode; label: string }> = [
  { value: 'none', label: 'None (input order)' },
  { value: 'largest_to_smallest', label: 'Largest to smallest' },
  { value: 'smallest_to_largest', label: 'Smallest to largest' },
  { value: 'top_most', label: 'Top most' },
  { value: 'bottom_most', label: 'Bottom most' },
  { value: 'left_most', label: 'Left most' },
  { value: 'right_most', label: 'Right most' },
  { value: 'top_left', label: 'Top left' },
  { value: 'top_right', label: 'Top right' },
  { value: 'bottom_left', label: 'Bottom left' },
  { value: 'bottom_right', label: 'Bottom right' },
  { value: 'center_most', label: 'Center most' },
  { value: 'crosshair', label: 'Crosshair nearest' }
];

export type StreamCrop = [number, number, number, number];
export type StreamCrosshair = [number, number];

export function readStreamCrop(streamCrop: unknown): StreamCrop {
  if (!Array.isArray(streamCrop) || streamCrop.length !== 4) return [-1, 1, -1, 1];
  return [
    Number(streamCrop[0] ?? -1),
    Number(streamCrop[1] ?? 1),
    Number(streamCrop[2] ?? -1),
    Number(streamCrop[3] ?? 1)
  ];
}

export function clampStreamCropValue(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(STREAM_CROP_MIN, Math.min(STREAM_CROP_MAX, value));
}

export function normalizeStreamCrop(crop: StreamCrop): StreamCrop {
  let [x0, x1, y0, y1] = crop;
  x0 = clampStreamCropValue(x0);
  x1 = clampStreamCropValue(x1);
  y0 = clampStreamCropValue(y0);
  y1 = clampStreamCropValue(y1);
  if (x1 < x0) [x0, x1] = [x1, x0];
  if (y1 < y0) [y0, y1] = [y1, y0];
  return [x0, x1, y0, y1];
}

export function formatCropValue(value: number): string {
  return value.toFixed(2);
}

export function readStreamCrosshair(streamCrosshair: unknown): StreamCrosshair {
  if (!Array.isArray(streamCrosshair) || streamCrosshair.length !== 2) return [0, 0];
  return [Number(streamCrosshair[0] ?? 0), Number(streamCrosshair[1] ?? 0)];
}

export function normalizeStreamCrosshair(crosshair: StreamCrosshair): StreamCrosshair {
  const x = clampStreamCropValue(crosshair[0]);
  const y = clampStreamCropValue(crosshair[1]);
  return [x, y];
}

export function normalizeStreamOrderingMode(mode: unknown): StreamOrderingMode {
  const normalized = String(mode ?? 'none')
    .trim()
    .toLowerCase()
    .replaceAll('-', '_')
    .replaceAll(' ', '_');
  const allowed = STREAM_ORDERING_MODES.map((entry) => entry.value);
  return (allowed as string[]).includes(normalized) ? (normalized as StreamOrderingMode) : 'none';
}

export function parseSelectedResolution(
  selectedResolution: string | null | undefined
): { width: number; height: number } | null {
  const parts = String(selectedResolution ?? '').split('x');
  if (parts.length !== 2) return null;
  const width = Number(parts[0]);
  const height = Number(parts[1]);
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return null;
  return { width: Math.trunc(width), height: Math.trunc(height) };
}

export function scaledSize(value: number, divisor: number): number {
  const scaled = Math.max(16, Math.round(value / Math.max(1, divisor)));
  return scaled % 2 === 0 ? scaled : scaled - 1;
}

export function applyOutputScaleToEncoderSettings(
  encoderSettings: { outWidth?: number | null; outHeight?: number | null },
  selectedResolution: string | null | undefined,
  divisor: number
): void {
  const src = parseSelectedResolution(selectedResolution);
  if (!src) return;
  encoderSettings.outWidth = scaledSize(src.width, divisor);
  encoderSettings.outHeight = scaledSize(src.height, divisor);
}

export async function fetchStreamFormatBenchmark(args: {
  currentBackend: () => { kind?: string | null; handle?: unknown } | null | undefined;
  currentDevice: () => { identity?: { keys?: string[] | null } | null } | null | undefined;
  selectedResolution: string | null | undefined;
  benchTargetFps: number;
  benchSampleMs: number;
  effectiveModes: () => StreamModeDescriptor[];
  resolutionKey: (mode: StreamModeDescriptor) => string | null;
  apiPath: (path: string) => string;
  setBenchError: (message: string) => void;
}): Promise<{ warnings: string[]; results: BenchFormatGroup[] } | null> {
  const {
    currentBackend,
    currentDevice,
    selectedResolution,
    benchTargetFps,
    benchSampleMs,
    effectiveModes,
    resolutionKey,
    apiPath,
    setBenchError
  } = args;

  const backend = currentBackend();
  const device = currentDevice();
  const resolution = parseSelectedResolution(selectedResolution);
  if (!backend || !device || !resolution) {
    toaster.error({ title: 'Benchmark failed', description: 'Select a device/backend + resolution first.' });
    return null;
  }

  try {
    const fps = Math.max(1, Math.trunc(Number(benchTargetFps) || 120));
    const sampleMs = Math.max(250, Math.trunc(Number(benchSampleMs) || 1500));
    const selectedModes = effectiveModes()
      .filter((mode) => resolutionKey(mode) === selectedResolution)
      .map((mode) => ({ id: mode.id }));

    const payload = {
      backend: backend.kind,
      handle: backend.handle,
      device_keys: device.identity?.keys ?? [],
      modes: selectedModes,
      width: resolution.width,
      height: resolution.height,
      target_fps: fps,
      sample_ms: sampleMs,
      restore_existing: true,
      controls: []
    };

    const response = await apiFetchResponse(apiPath('/streams/bench/formats'), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(payload)
    });
    if (!response.ok) {
      const text = await response.text().catch(() => '');
      throw new Error(text || `HTTP ${response.status}`);
    }

    const json = await response.json();
    const warnings = Array.isArray(json?.warnings) ? json.warnings : [];
    const results = Array.isArray(json?.formats) ? json.formats : [];
    return {
      warnings,
      results: [...results].sort((a, b) => Number(b.capture_avg_fps ?? 0) - Number(a.capture_avg_fps ?? 0))
    };
  } catch (error) {
    console.error('Benchmark failed', error);
    reportError({
      title: 'Benchmark failed',
      error,
      fallback: 'Unable to run the benchmark right now.',
      inline: (message) => {
        setBenchError(message);
      }
    });
    return null;
  }
}
