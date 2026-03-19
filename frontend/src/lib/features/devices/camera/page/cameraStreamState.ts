import type { Interval, Mode } from '$lib/ts-bindings/http/client';

export function intervalToFps(interval: Interval | undefined | null): number | null {
  if (!interval) return null;
  const numerator = Number(interval.numerator ?? 0);
  const denominator = Number(interval.denominator ?? 0);
  if (!Number.isFinite(numerator) || !Number.isFinite(denominator) || numerator <= 0 || denominator <= 0) return null;
  const fps = denominator / numerator;
  return Number.isFinite(fps) && fps > 0 ? fps : null;
}

export function mediaFormatMatches(mode: Mode | null | undefined, target: Mode['format'] | null | undefined): boolean {
  if (!mode || !target) return false;
  const a = mode.format ?? null;
  const b = target ?? null;
  if (!a || !b) return false;
  const ra = a.resolution ?? null;
  const rb = b.resolution ?? null;
  return (
    String(a.code ?? '').toUpperCase() === String(b.code ?? '').toUpperCase() &&
    String(a.color ?? '') === String(b.color ?? '') &&
    Number(ra?.width ?? 0) === Number(rb?.width ?? 0) &&
    Number(ra?.height ?? 0) === Number(rb?.height ?? 0)
  );
}

export function frameRateToFps(rate: Interval | null | undefined): number | null {
  const numerator = Number(rate?.numerator ?? 0);
  const denominator = Number(rate?.denominator ?? 0);
  if (!Number.isFinite(numerator) || !Number.isFinite(denominator) || numerator <= 0 || denominator <= 0) return null;
  const fps = numerator / denominator;
  return Number.isFinite(fps) && fps > 0 ? fps : null;
}

export function normalizeFpsLimit(value: unknown): number | null {
  const parsed = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return parsed;
}

export function normalizeRotationDegrees(value: unknown): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return 0;
  return Math.trunc(parsed);
}

export function gcd(a: number, b: number): number {
  let x = Math.abs(a);
  let y = Math.abs(b);
  while (y) {
    const t = x % y;
    x = y;
    y = t;
  }
  return x || 1;
}

export function fpsToFrameRate(fps: number | null): Interval | null {
  if (!fps || !Number.isFinite(fps) || fps <= 0) return null;
  const scale = 1000;
  const numerator = Math.max(1, Math.round(fps * scale));
  const denominator = scale;
  const div = gcd(numerator, denominator);
  return { numerator: Math.floor(numerator / div), denominator: Math.floor(denominator / div) };
}
