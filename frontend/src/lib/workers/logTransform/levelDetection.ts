import type { LogFilter, LogSegment, LevelMarker } from './types';

export const LOG_LEVEL_ALIASES: Record<string, Exclude<LogFilter, 'all'>> = {
  info: 'info',
  notice: 'info',
  warn: 'warn',
  warning: 'warn',
  error: 'error',
  err: 'error',
  fatal: 'error',
  critical: 'error',
  debug: 'debug',
  trace: 'debug'
};

export function normalizeLevelToken(value: string | null | undefined): Exclude<LogFilter, 'all'> | null {
  if (!value) return null;
  const cleaned = value.replace(/^[^a-zA-Z]+/, '').replace(/[^a-zA-Z]+$/, '');
  if (!cleaned) return null;
  return LOG_LEVEL_ALIASES[cleaned.toLowerCase()] ?? null;
}

export function extractLevelFromText(text: string): Exclude<LogFilter, 'all'> | null {
  if (!text) return null;
  const trimmed = text.trim();
  if (!trimmed) return null;
  const tokens = trimmed.split(/\s+/);
  for (const token of tokens) {
    const normalized = normalizeLevelToken(token);
    if (normalized) {
      return normalized;
    }
  }
  return null;
}

export function extractLevelMarker(segments: LogSegment[]): LevelMarker | null {
  for (const segment of segments) {
    const marker = extractLevelFromText(segment.text);
    if (marker) {
      return { level: marker, color: segment.color };
    }
  }
  return null;
}

export function detectLeadingLevelPrefix(
  text: string
): { level: Exclude<LogFilter, 'all'>; removeUntil: number } | null {
  if (!text) return null;
  let idx = 0;
  while (idx < text.length && /\s/.test(text[idx] ?? '')) {
    idx += 1;
  }
  const tokenStart = idx;
  while (idx < text.length && !/\s/.test(text[idx] ?? '')) {
    idx += 1;
  }
  if (tokenStart === idx) {
    return null;
  }
  const token = text.slice(tokenStart, idx);
  const normalized = normalizeLevelToken(token);
  if (!normalized) {
    return null;
  }
  let end = idx;
  while (end < text.length && /[\s:·-]/.test(text[end] ?? '')) {
    end += 1;
  }
  return { level: normalized, removeUntil: end };
}

export function levelLineRemovalLength(text: string, minimum: number): number {
  let removal = minimum;
  const newlineIdx = text.indexOf('\n', minimum);
  if (newlineIdx >= 0 && newlineIdx <= 512) {
    removal = newlineIdx + 1;
    while (removal < text.length && /[\r\n]/.test(text[removal] ?? '')) {
      removal += 1;
    }
  }
  return removal;
}

export function detectLevelFromText(text: string): Exclude<LogFilter, 'all'> | null {
  if (!text) return null;
  const windowed = text.trimStart().slice(0, 80);
  const match = windowed.match(/\b(info|warn|warning|error|debug|trace|fatal|critical|notice|err)\b/i);
  if (!match) return null;
  return normalizeLevelToken(match[1]);
}

export function resolveLogLevel(rawLevel: string | undefined, message: string, marker?: LevelMarker): LevelMarker {
  if (marker) {
    return marker;
  }
  const inferred = detectLevelFromText(message);
  if (inferred) {
    return { level: inferred };
  }
  return { level: normalizeLevelToken(rawLevel) ?? 'info' };
}

export function findLevelColor(segments: LogSegment[], level: Exclude<LogFilter, 'all'>): string | undefined {
  for (const segment of segments) {
    if (!segment.text) continue;
    const tokens = segment.text.split(/\s+/);
    for (const token of tokens) {
      if (normalizeLevelToken(token) === level) {
        if (segment.color) {
          return segment.color;
        }
      }
    }
  }
  return undefined;
}
