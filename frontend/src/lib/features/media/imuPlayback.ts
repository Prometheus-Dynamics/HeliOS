import { browser } from '$app/environment';
import { mapImuStatus, type ImuStatusResponse } from '$lib/api/systems/mappers';
import type { ImuStatus } from '$lib/types/systems';

export type MediaImuSample = {
  tMs: number;
  imu: ImuStatus;
};

export type MediaFrameTimeline = {
  offsetsMs: number[];
  spanMs: number;
};

type FrameTsScale = 'ms' | 'pts90k' | 'micros' | 'nanos';

type MediaImuEventPayload = {
  t_ms?: unknown;
  imu?: ImuStatusResponse | null;
};

function isGzipPayload(bytes: Uint8Array): boolean {
  return bytes.length >= 2 && bytes[0] === 0x1f && bytes[1] === 0x8b;
}

async function decodeImuPayload(bytes: Uint8Array): Promise<string> {
  if (!isGzipPayload(bytes)) {
    return new TextDecoder('utf-8', { fatal: false }).decode(bytes);
  }

  const DecompressionStreamCtor = (globalThis as unknown as { DecompressionStream?: new (format: string) => TransformStream })
    .DecompressionStream;
  if (DecompressionStreamCtor) {
    const decompressedResponse = new Response(new Blob([bytes]).stream().pipeThrough(new DecompressionStreamCtor('gzip')));
    return await decompressedResponse.text();
  }

  const { gunzipSync, strFromU8 } = await import('fflate');
  return strFromU8(gunzipSync(bytes));
}

function parseLine(line: string): MediaImuSample | null {
  let parsed: MediaImuEventPayload;
  try {
    parsed = JSON.parse(line) as MediaImuEventPayload;
  } catch {
    return null;
  }
  const tMsRaw = Number(parsed.t_ms);
  if (!Number.isFinite(tMsRaw)) return null;
  const imu = mapImuStatus(parsed.imu ?? null);
  return {
    tMs: Math.max(0, Math.round(tMsRaw)),
    imu
  };
}

export async function loadMediaImuSamples(url: string, signal?: AbortSignal): Promise<MediaImuSample[]> {
  if (!browser) return [];
  const response = await fetch(url, {
    method: 'GET',
    signal,
    headers: {
      Accept: 'application/x-ndjson+gzip, application/x-ndjson, application/json'
    }
  });
  if (!response.ok) {
    throw new Error(`Failed to load IMU sidecar (${response.status})`);
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  const text = await decodeImuPayload(bytes);
  const samples: MediaImuSample[] = [];
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    const sample = parseLine(line);
    if (sample) samples.push(sample);
  }
  samples.sort((left, right) => left.tMs - right.tMs);
  return samples;
}

export async function loadMediaFrameTimeline(url: string, signal?: AbortSignal): Promise<MediaFrameTimeline | null> {
  if (!browser) return null;
  const response = await fetch(url, {
    method: 'GET',
    signal,
    headers: {
      Accept: 'text/plain'
    }
  });
  if (!response.ok) {
    return null;
  }
  const text = await response.text();
  const rawValues: number[] = [];
  let last = 0;
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    const parsed = Number(line);
    if (!Number.isFinite(parsed) || parsed < 0) continue;
    const clamped = Math.max(last, Math.round(parsed));
    rawValues.push(clamped);
    last = clamped;
  }
  if (!rawValues.length) return null;

  const scale = inferFrameTsScale(rawValues);
  const first = rawValues[0];
  const offsetsMs: number[] = [];
  let lastOffset = 0;
  for (const value of rawValues) {
    const rawDelta = Math.max(0, value - first);
    const clamped = Math.max(lastOffset, convertFrameDeltaMs(rawDelta, scale));
    offsetsMs.push(clamped);
    lastOffset = clamped;
  }
  return {
    offsetsMs,
    spanMs: offsetsMs[offsetsMs.length - 1] ?? 0
  };
}

function inferFrameTsScale(values: number[]): FrameTsScale | null {
  for (let index = 1; index < values.length; index += 1) {
    const next = values[index];
    const prev = values[index - 1];
    const step = next - prev;
    if (step <= 0 || !Number.isFinite(step)) continue;

    if (step >= 500 && step <= 6_000) return 'pts90k';
    if (step >= 6_000 && step <= 2_000_000) return 'micros';
    if (step >= 2_000_000 && step <= 5_000_000_000) return 'nanos';
    if (next >= 100_000_000_000_000_000) return 'nanos';
    if (next >= 100_000_000_000_000) return step >= 2_000_000 ? 'nanos' : 'micros';
    return 'ms';
  }
  return null;
}

function convertFrameDeltaMs(delta: number, scale: FrameTsScale | null): number {
  if (!Number.isFinite(delta) || delta <= 0) return 0;
  switch (scale) {
    case 'pts90k':
      return Math.round((delta * 1000) / 90_000);
    case 'micros':
      return Math.round(delta / 1000);
    case 'nanos':
      return Math.round(delta / 1_000_000);
    case 'ms':
      return Math.round(delta);
    default:
      if (delta >= 1_000_000_000) return Math.round(delta / 1_000_000);
      if (delta >= 1_000_000) return Math.round(delta / 1000);
      return Math.round(delta);
  }
}

type ImuSampleOptions = {
  playbackDurationMs?: number | null;
  frameTimeline?: MediaFrameTimeline | null;
};

function clampToRange(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

export function mapPlaybackToImuMs(samples: MediaImuSample[], playbackMs: number, _options: ImuSampleOptions = {}): number {
  void _options;
  if (!samples.length) return 0;
  const last = samples[samples.length - 1].tMs;
  const maxImuTimeMs = Math.max(0, last);
  if (maxImuTimeMs <= 0) return 0;

  const rawPlaybackMs = Number.isFinite(playbackMs) ? Math.max(0, playbackMs) : 0;
  return clampToRange(Math.round(rawPlaybackMs), 0, maxImuTimeMs);
}

export function sampleMediaImuAtMs(samples: MediaImuSample[], playbackMs: number, options: ImuSampleOptions = {}): MediaImuSample | null {
  if (!samples.length) return null;
  const timeMs = mapPlaybackToImuMs(samples, playbackMs, options);

  if (timeMs < samples[0].tMs) return null;
  if (timeMs === samples[0].tMs) return samples[0];
  const last = samples[samples.length - 1];
  if (timeMs >= last.tMs) return last;

  let low = 0;
  let high = samples.length - 1;
  while (low <= high) {
    const mid = (low + high) >> 1;
    const sampleTime = samples[mid].tMs;
    if (sampleTime === timeMs) return samples[mid];
    if (sampleTime < timeMs) {
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }

  const nextIndex = Math.min(samples.length - 1, Math.max(1, low));
  const prevIndex = Math.max(0, nextIndex - 1);
  const prev = samples[prevIndex];
  // Prefer the latest known sample at or before playback time to avoid showing a "future" IMU pose.
  return prev;
}
