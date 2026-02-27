type TimestampLike = {
  name?: string | null;
  captured_at_ms?: number | null;
  createdAt?: string | null;
  updatedAt?: string | null;
};

const MIN_REASONABLE_TIMESTAMP_MS = Date.UTC(2000, 0, 1);
const MAX_REASONABLE_TIMESTAMP_MS = Date.UTC(2100, 0, 1);
const EPOCH_ISO = new Date(0).toISOString();

function parseFiniteTimestampMs(value: unknown): number | null {
  const numeric = typeof value === 'number' ? value : Number.NaN;
  if (!Number.isFinite(numeric)) return null;
  const rounded = Math.trunc(numeric);
  if (rounded < MIN_REASONABLE_TIMESTAMP_MS || rounded > MAX_REASONABLE_TIMESTAMP_MS) return null;
  return rounded;
}

function parseIsoTimestampMs(value: unknown): number | null {
  if (typeof value !== 'string') return null;
  const parsed = Date.parse(value);
  if (!Number.isFinite(parsed)) return null;
  const rounded = Math.trunc(parsed);
  if (rounded < MIN_REASONABLE_TIMESTAMP_MS || rounded > MAX_REASONABLE_TIMESTAMP_MS) return null;
  return rounded;
}

function parseTimestampFromName(name: string): number | null {
  const trimmed = name.trim();
  if (!trimmed.length) return null;

  const msMatches = trimmed.match(/\d{13}/g) ?? [];
  for (const raw of msMatches) {
    const parsed = parseFiniteTimestampMs(Number(raw));
    if (parsed != null) return parsed;
  }

  const secMatches = trimmed.match(/\d{10}/g) ?? [];
  for (const raw of secMatches) {
    const seconds = Number(raw);
    if (!Number.isFinite(seconds)) continue;
    const ms = Math.trunc(seconds * 1000);
    const parsed = parseFiniteTimestampMs(ms);
    if (parsed != null) return parsed;
  }

  return null;
}

export function mediaTimestampMs(value: TimestampLike): number {
  const captured = parseFiniteTimestampMs(value.captured_at_ms);
  if (captured != null) return captured;

  const created = parseIsoTimestampMs(value.createdAt);
  if (created != null) return created;

  const updated = parseIsoTimestampMs(value.updatedAt);
  if (updated != null) return updated;

  const fromName = parseTimestampFromName(String(value.name ?? ''));
  if (fromName != null) return fromName;

  return 0;
}

export function mediaTimestampIso(value: TimestampLike): string {
  const ms = mediaTimestampMs(value);
  return ms > 0 ? new Date(ms).toISOString() : EPOCH_ISO;
}

export function compareMediaName(a: { name?: string | null }, b: { name?: string | null }): number {
  const left = String(a.name ?? '');
  const right = String(b.name ?? '');
  return left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' });
}

export function compareMediaRecent(a: TimestampLike, b: TimestampLike): number {
  const left = mediaTimestampMs(a);
  const right = mediaTimestampMs(b);
  if (left !== right) return right - left;
  return compareMediaName(b, a);
}
