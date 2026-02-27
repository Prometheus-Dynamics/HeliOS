import type { RequestHandler } from '@sveltejs/kit';
import { OpenAPI } from '$lib/ts-bindings/http/client';
import { fetchDashboardPageData } from '$lib/api/dashboardPage';
import type { Interval } from '$lib/ts-bindings/http/client';
import type { CaptureDeviceStatus } from '$lib/types/cameraConfig';

const JSON_HEADERS = { 'content-type': 'application/json' };

export const GET: RequestHandler = async ({ url }) => {
  try {
    OpenAPI.BASE = url.origin;
    const payload = await fetchDashboardPageData();
    return new Response(JSON.stringify(payload), {
      headers: JSON_HEADERS
    });
  } catch (error) {
    console.error('Failed to load dashboard payload', error);
    return new Response(JSON.stringify({ message: 'Dashboard data unavailable' }), {
      status: 502,
      headers: JSON_HEADERS
    });
  }
};

type FractionStruct = { numerator: number; denominator: number };
type FractionCandidate = { numerator?: unknown; denominator?: unknown } | null | undefined;

type StepwiseStruct = {
  interval?: FractionCandidate;
  min?: FractionCandidate;
  max?: FractionCandidate;
  step?: FractionCandidate;
} | null | undefined;

type IntervalStruct =
  | { Discrete?: FractionCandidate; Stepwise?: StepwiseStruct }
  | Interval
  | FractionCandidate
  | unknown;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function toFraction(candidate: FractionCandidate): FractionStruct | null {
  if (!candidate || !isRecord(candidate)) return null;
  const numerator = Number(candidate.numerator);
  const denominator = Number(candidate.denominator);
  if (!Number.isFinite(numerator) || !Number.isFinite(denominator) || denominator === 0) return null;
  return { numerator, denominator };
}

function computeDiscreteFps(fraction: FractionStruct | null): number | null {
  if (!fraction) return null;
  const { numerator, denominator } = fraction;
  if (!Number.isFinite(numerator) || !Number.isFinite(denominator)) return null;
  if (numerator <= 0 || denominator <= 0) return null;
  return numerator / denominator;
}

function fractionFromInterval(interval: IntervalStruct): FractionStruct | null {
  if (!interval) return null;

  const direct = toFraction(interval as FractionCandidate);
  if (direct) return direct;

  if (!isRecord(interval)) return null;

  if ('Discrete' in interval) {
    const discrete = toFraction((interval as { Discrete?: FractionCandidate }).Discrete);
    if (discrete) return discrete;
  }

  if ('Stepwise' in interval && isRecord(interval.Stepwise)) {
    const stepwise = interval.Stepwise as StepwiseStruct;
    const candidates: FractionCandidate[] = [
      stepwise?.interval,
      stepwise?.min,
      stepwise?.max,
      stepwise?.step
    ];
    for (const candidate of candidates) {
      const fraction = toFraction(candidate);
      if (fraction) return fraction;
    }
  }

  return null;
}

function extractFps(interval: IntervalStruct | null | undefined): number | null {
  return computeDiscreteFps(fractionFromInterval(interval));
}

function manifestCapture(slot: CaptureDeviceStatus): Record<string, unknown> | null {
  if (!slot.manifest || typeof slot.manifest !== 'object') return null;
  const capture = (slot.manifest as Record<string, unknown>).capture;
  if (!capture || typeof capture !== 'object') return null;
  return capture as Record<string, unknown>;
}

function formatTimestamp(value: string | undefined): string {
  if (!value) return 'Unknown';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'Unknown';
  const hours = String(date.getUTCHours()).padStart(2, '0');
  const minutes = String(date.getUTCMinutes()).padStart(2, '0');
  return `${hours}:${minutes} UTC`;
}

function titleCase(value: string): string {
  return value
    .split(/[\s_-]+/)
    .filter(Boolean)
    .map((chunk) => chunk.charAt(0).toUpperCase() + chunk.slice(1).toLowerCase())
    .join(' ');
}

function pluralize(word: string, count: number): string {
  return count === 1 ? word : `${word}s`;
}
