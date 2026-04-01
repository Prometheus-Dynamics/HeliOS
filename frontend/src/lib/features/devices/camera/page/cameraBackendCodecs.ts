import type { CodecInfo } from '$lib/api/httpClient';
import { encoderSelectionId } from '$lib/api/streamEncoderSettings';

import { asTrimmedString, normalizeFormatKey } from './cameraBackendSupport';

export function dedupeCodecs(list: CodecInfo[], keyFn: (codec: CodecInfo) => string): CodecInfo[] {
  const seen = new Set<string>();
  const out: CodecInfo[] = [];
  for (const codec of list) {
    const key = keyFn(codec);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(codec);
  }
  return out;
}

export function codecSelectionId(codec: CodecInfo): string | null {
  if (String(codec.kind ?? '').toLowerCase() === 'encoder') {
    return encoderSelectionId(codec);
  }
  const name = String(codec.name ?? '').trim();
  const implementation = String(codec.implementation ?? '').trim();
  return implementation || name || null;
}

export function preferredCodecMatch(list: CodecInfo[], key: string): CodecInfo | null {
  const normalized = key.trim().toLowerCase();
  if (!normalized) return null;

  const directSelection = list.find((codec) => String(codecSelectionId(codec) ?? '').trim().toLowerCase() === normalized);
  if (directSelection) return directSelection;

  const directImpl = list.find((codec) => String(codec.implementation ?? '').trim().toLowerCase() === normalized);
  if (directImpl) return directImpl;

  return list.find((codec) => String(codec.name ?? '').trim().toLowerCase() === normalized) ?? null;
}

export function pickCodecId(
  list: CodecInfo[],
  desired: string | null | undefined,
  preferred: string[] = [],
): string | null {
  if (!list.length) return null;
  const wanted = typeof desired === 'string' ? desired.trim() : '';
  if (wanted) {
    const match = preferredCodecMatch(list, wanted);
    if (match) return codecSelectionId(match);
  }
  for (const pref of preferred) {
    const key = String(pref ?? '').trim();
    if (!key) continue;
    const match = preferredCodecMatch(list, key);
    if (match) return codecSelectionId(match);
  }
  return list[0] ? codecSelectionId(list[0]) : null;
}

export function decoderPreferencesForFormat(
  format: string | null | undefined,
  decoderDefaultIdsByCaptureFormat: Record<string, string>
): string[] {
  const defaults = decoderDefaultIdsByCaptureFormat ?? {};
  const key = normalizeFormatKey(format ?? '');
  const preferred = key ? asTrimmedString(defaults[key]) : '';
  const fallback = asTrimmedString(defaults.ANY);
  if (preferred && fallback && preferred !== fallback) return [preferred, fallback];
  if (preferred) return [preferred];
  if (fallback) return [fallback];
  return [];
}

export function normalizeDecoderDefaultIdsByCaptureFormat(value: unknown): Record<string, string> {
  return value && typeof value === 'object'
    ? Object.fromEntries(
        Object.entries(value).flatMap(([key, rawValue]) => {
          const normalizedKey = normalizeFormatKey(key);
          const normalizedValue = asTrimmedString(rawValue);
          return normalizedKey && normalizedValue ? [[normalizedKey, normalizedValue] as const] : [];
        })
      )
    : {};
}
