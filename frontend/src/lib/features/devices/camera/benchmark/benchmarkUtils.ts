import type { Mode } from '$lib/api/client';

export type CpuSample = { engine_cpu_avg?: number | null; system_cpu_avg?: number | null };
export type BenchCodecStat = { implementation: string; avg_ms: number; avg_fps: number; errors?: number };
export type BenchCodecStatCpu = { stat: BenchCodecStat; cpu?: CpuSample; cpu_delta?: CpuSample; cpuDelta?: CpuSample };
export type ModeResult = {
  format: string;
  resolution: string;
  capture_avg_fps: number;
  host_avg_fps: number;
  baseline_cpu?: CpuSample;
  encoder_input_decoder?: string | null;
  decoders: BenchCodecStatCpu[];
  encoders: BenchCodecStatCpu[];
};

export type BenchmarkSummary = {
  benchmark_id: string;
  backend: string;
  device_keys: string[];
  started_at: string;
  completed_at: string;
  canceled?: boolean;
};

export type SensorBenchmarkStatus =
  | {
      status: 'running';
      started_at: string;
      progress: {
        total_modes: number;
        completed_modes: number;
        current_format?: string | null;
        current_resolution?: string | null;
      };
    }
  | { status: 'failed'; started_at: string; error: string }
  | { status: 'completed'; summary: BenchmarkSummary; result: { summary: BenchmarkSummary; modes: ModeResult[]; warnings: string[] } };

export type BenchmarkListItem = { summary: BenchmarkSummary };

export type CodecInfo = {
  kind?: string | null;
  implementation?: string | null;
  input?: string | null;
  output?: string | null;
  fourcc?: string | null;
};

export type ModeRankRow = {
  format: string;
  resolution: string;
  captureFps: number;
  hostFps: number;
  bestDecoderImpl: string | null;
  bestDecoderFps: number;
  bestDecoderCpu: string;
  bestEncoderImpl: string | null;
  bestEncoderFps: number;
  bestEncoderCpu: string;
  score: number;
};

export function formatLabel(fmt: string | null | undefined): string {
  if (!fmt) return 'Unspecified';
  return fmt.toUpperCase();
}

export function resolutionKey(mode: Mode | undefined): string | null {
  const res = mode?.format?.resolution;
  if (!res?.width || !res?.height) return null;
  return `${res.width}x${res.height}`;
}

export function availableFormats(modes: Mode[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const mode of modes) {
    const fmt = formatLabel(mode.format?.code);
    if (seen.has(fmt)) continue;
    seen.add(fmt);
    out.push(fmt);
  }
  out.sort();
  return out;
}

export function availableResolutions(modes: Mode[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const mode of modes) {
    const key = resolutionKey(mode);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push(key);
  }
  out.sort((a, b) => {
    const [aw, ah] = a.split('x').map((v) => Number(v));
    const [bw, bh] = b.split('x').map((v) => Number(v));
    const ap = (aw || 0) * (ah || 0);
    const bp = (bw || 0) * (bh || 0);
    return ap - bp;
  });
  return out;
}

export function toggleIncluded(list: string[], value: string): string[] {
  return list.includes(value) ? list.filter((v) => v !== value) : [...list, value];
}

export function formatCpu(sample?: CpuSample | null): string {
  if (!sample) return '—';
  const parts: string[] = [];
  if (typeof sample.engine_cpu_avg === 'number') parts.push(`engine ${sample.engine_cpu_avg.toFixed(1)}%`);
  if (typeof sample.system_cpu_avg === 'number') parts.push(`system ${sample.system_cpu_avg.toFixed(1)}%`);
  return parts.length ? parts.join(' / ') : '—';
}

export function cpuDeltaEngine(sample?: CpuSample | null): number | null {
  if (!sample) return null;
  return typeof sample.engine_cpu_avg === 'number' ? sample.engine_cpu_avg : null;
}

export function cpuDeltaSystem(sample?: CpuSample | null): number | null {
  if (!sample) return null;
  return typeof sample.system_cpu_avg === 'number' ? sample.system_cpu_avg : null;
}

export function msToDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '—';
  let seconds = Math.round(ms / 1000);
  const days = Math.floor(seconds / 86_400);
  seconds -= days * 86_400;
  const hours = Math.floor(seconds / 3600);
  seconds -= hours * 3600;
  const minutes = Math.floor(seconds / 60);
  seconds -= minutes * 60;

  const parts: string[] = [];
  if (days > 0) parts.push(`${days}d`);
  if (hours > 0 || days > 0) parts.push(`${hours}h`);
  parts.push(`${minutes}m`);
  parts.push(`${seconds}s`);
  return parts.join(' ');
}

export function normalizeFourcc(value: string | null | undefined): string {
  return (value ?? '').trim().toUpperCase();
}

export function uniqueImpls(kind: string, inputFourcc: string, inventory: CodecInfo[]): string[] {
  const wantKind = kind.toLowerCase();
  const wantInput = normalizeFourcc(inputFourcc);
  const impls: string[] = [];
  for (const c of inventory) {
    const ck = (c.kind ?? '').toString().toLowerCase();
    if (ck !== wantKind) continue;
    const impl = (c.implementation ?? '').toString();
    if (!impl) continue;
    const input = normalizeFourcc(c.input);
    if (input === wantInput) impls.push(impl);
  }
  return Array.from(new Set(impls));
}

export function codecCountsForMode(mode: Mode, inventory: CodecInfo[]): { decoderCount: number; encoderCount: number } {
  const fourcc = normalizeFourcc(mode.format?.code);
  if (!fourcc) return { decoderCount: 0, encoderCount: 0 };

  const decoders = uniqueImpls('decoder', fourcc, inventory);
  const rg24Encoders = uniqueImpls('encoder', 'RG24', inventory);
  return { decoderCount: decoders.length, encoderCount: decoders.length ? rg24Encoders.length : 0 };
}

export function scoreCodec(avgFps: number, cpuDelta: CpuSample | null | undefined, targetFps: number): number {
  const fps = Number(avgFps ?? 0);
  const want = Math.max(1, Number(targetFps ?? 120));
  const fpsPenalty = Math.abs(want - fps);
  const cpu = cpuDeltaEngine(cpuDelta) ?? 0;
  const cpuPenalty = Math.max(0, cpu) * 0.5;
  return fpsPenalty * 10 + cpuPenalty;
}

export function bestByScore(
  list: Array<BenchCodecStatCpu | null | undefined>,
  targetFps: number
) {
  let best: (typeof list)[number] | null = null;
  let bestScore = Infinity;
  for (const item of list ?? []) {
    const fps = Number(item?.stat?.avg_fps ?? 0);
    const cpuDelta = item.cpu_delta ?? item.cpuDelta ?? null;
    const err = Number(item?.stat?.errors ?? 0);
    const s = scoreCodec(fps, cpuDelta, targetFps) + (err > 0 ? 1000 : 0);
    if (s < bestScore) {
      bestScore = s;
      best = item;
    }
  }
  return best;
}

type BenchModeResult = Pick<
  ModeResult,
  'format' | 'resolution' | 'capture_avg_fps' | 'host_avg_fps' | 'decoders' | 'encoders'
>;

const cpuDeltaFor = (value: BenchCodecStatCpu | null | undefined): CpuSample | null =>
  value?.cpu_delta ?? value?.cpu ?? null;

export function rankRows(result: { modes: BenchModeResult[] } | null, targetFps: number): ModeRankRow[] {
  if (!result) return [];
  const target = Math.max(1, Math.trunc(Number(targetFps) || 120));
  const rows: ModeRankRow[] = [];
  for (const m of result.modes ?? []) {
    const bestDec = bestByScore(m.decoders ?? [], target);
    const bestEnc = bestByScore(m.encoders ?? [], target);
    const decFps = Number(bestDec?.stat?.avg_fps ?? 0);
    const encFps = Number(bestEnc?.stat?.avg_fps ?? 0);
    const score = scoreCodec(decFps, cpuDeltaFor(bestDec), target) + scoreCodec(encFps, cpuDeltaFor(bestEnc), target);
    rows.push({
      format: String(m.format ?? ''),
      resolution: String(m.resolution ?? ''),
      captureFps: Number(m.capture_avg_fps ?? 0),
      hostFps: Number(m.host_avg_fps ?? 0),
      bestDecoderImpl: bestDec?.stat?.implementation ?? null,
      bestDecoderFps: decFps,
      bestDecoderCpu: formatCpu(cpuDeltaFor(bestDec)),
      bestEncoderImpl: bestEnc?.stat?.implementation ?? null,
      bestEncoderFps: encFps,
      bestEncoderCpu: formatCpu(cpuDeltaFor(bestEnc)),
      score
    });
  }
  rows.sort((a, b) => a.score - b.score);
  return rows;
}
