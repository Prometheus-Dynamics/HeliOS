import type { LogEntry, LogFilter } from './types';
import { normalizeLevelToken } from './logFilters';

export type LogSegment = {
  text: string;
  color?: string;
  background?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
};

type InlineTimestampDetails = {
  value: string;
  removeUntil: number;
};

type LevelMarker = {
  level: Exclude<LogFilter, 'all'>;
  color?: string;
};

type DecoratedLogMessage = {
  inlineTimestamp: string | null;
  rendered: string;
  plainText: string;
  levelMarker?: LevelMarker;
  segments: LogSegment[];
  threadId?: string;
};

type LogFramePayload = {
  stream_id?: string;
  sequence?: number;
  timestamp?: string;
  level?: string;
  message?: string;
  file_id?: string;
  fields?: unknown;
};

const ANSI_ESCAPE_REGEX = new RegExp(`${String.fromCharCode(27)}\\[((?:\\d{1,3};?)*)m`, 'g');

type SegmentState = {
  color?: string;
  background?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
};

function decorateLogMessage(raw: string): DecoratedLogMessage {
  const segments = parseAnsiSegments(raw);
  const sourceSegments = segments;
  const plainWithLevel = segments.map((segment) => segment.text).join('');
  const levelMarker = extractLevelMarker(segments);

  let workingSegments = segments;
  let workingPlain = plainWithLevel;

  const inline = extractInlineTimestamp(workingPlain);
  if (inline) {
    workingSegments = trimLeadingSegments(workingSegments, inline.removeUntil);
    workingPlain = workingPlain.slice(inline.removeUntil);
  }

  const stripped = stripLeadingLevelLines(workingSegments, workingPlain);
  workingSegments = stripped.segments;
  workingPlain = stripped.remainingPlain;

  const threadDetails = stripThreadIdMarker(workingSegments, workingPlain);
  workingSegments = threadDetails.segments;
  workingPlain = threadDetails.remainingPlain;

  return {
    inlineTimestamp: inline?.value ?? null,
    rendered: segmentsToHtml(workingSegments),
    plainText: plainWithLevel,
    levelMarker: levelMarker ?? undefined,
    segments: sourceSegments,
    threadId: threadDetails.threadId
  };
}

function detectLeadingLevelPrefix(text: string): { level: Exclude<LogFilter, 'all'>; removeUntil: number } | null {
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
  while (end < text.length && /[\s:\u00b7-]/.test(text[end] ?? '')) {
    end += 1;
  }
  return { level: normalized, removeUntil: end };
}

function stripLeadingLevelLines(
  initialSegments: LogSegment[],
  initialPlain: string
): { segments: LogSegment[]; remainingPlain: string } {
  let segments = initialSegments;
  let plain = initialPlain;

  while (true) {
    const prefix = detectLeadingLevelPrefix(plain);
    if (!prefix) {
      break;
    }
    const removal = levelLineRemovalLength(plain, prefix.removeUntil);
    segments = trimLeadingSegments(segments, removal);
    plain = plain.slice(removal);
  }

  return { segments, remainingPlain: plain };
}

function levelLineRemovalLength(text: string, minimum: number): number {
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

function stripThreadIdMarker(
  segments: LogSegment[],
  plain: string
): { segments: LogSegment[]; remainingPlain: string; threadId?: string } {
  const match = plain.match(/ThreadId\((\d+)\)/);
  if (!match || match.index == null) {
    return { segments, remainingPlain: plain };
  }
  const start = match.index;
  let end = start + match[0].length;
  while (end < plain.length && /[\s:]/.test(plain[end] ?? '')) {
    end += 1;
  }
  const length = end - start;
  const trimmedSegments = removeSegmentRange(segments, start, length);
  const updatedPlain = plain.slice(0, start) + plain.slice(end);
  return { segments: trimmedSegments, remainingPlain: updatedPlain, threadId: match[1] };
}

function removeSegmentRange(segments: LogSegment[], start: number, length: number): LogSegment[] {
  if (length <= 0) return segments;
  const result: LogSegment[] = [];
  let offset = 0;
  const removeEnd = start + length;
  for (const segment of segments) {
    const segStart = offset;
    const segEnd = offset + segment.text.length;
    if (segEnd <= start || segStart >= removeEnd) {
      result.push(segment);
    } else {
      if (start > segStart) {
        result.push({ ...segment, text: segment.text.slice(0, start - segStart) });
      }
      if (removeEnd < segEnd) {
        result.push({ ...segment, text: segment.text.slice(removeEnd - segStart) });
      }
    }
    offset = segEnd;
  }
  return result.filter((segment) => segment.text.length > 0);
}

function parseAnsiSegments(raw: string): LogSegment[] {
  const segments: LogSegment[] = [];
  const state: SegmentState = {};
  ANSI_ESCAPE_REGEX.lastIndex = 0;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  const pushSegment = (text: string) => {
    if (!text) return;
    segments.push({
      text,
      color: state.color,
      background: state.background,
      bold: state.bold,
      dim: state.dim,
      italic: state.italic,
      underline: state.underline
    });
  };

  while ((match = ANSI_ESCAPE_REGEX.exec(raw)) !== null) {
    const index = match.index;
    if (index > lastIndex) {
      pushSegment(raw.slice(lastIndex, index));
    }
    lastIndex = ANSI_ESCAPE_REGEX.lastIndex;
    const codes = match[1] ? match[1].split(';').map((value) => Number.parseInt(value, 10)) : [0];
    applyAnsiCodes(state, codes);
  }
  if (lastIndex < raw.length) {
    pushSegment(raw.slice(lastIndex));
  }
  return segments;
}

function applyAnsiCodes(state: SegmentState, codes: number[]) {
  let idx = 0;
  while (idx < codes.length) {
    const code = codes[idx] ?? 0;
    switch (code) {
      case 0: {
        state.color = undefined;
        state.background = undefined;
        state.bold = undefined;
        state.dim = undefined;
        state.italic = undefined;
        state.underline = undefined;
        idx += 1;
        break;
      }
      case 1:
        state.bold = true;
        idx += 1;
        break;
      case 2:
        state.dim = true;
        idx += 1;
        break;
      case 3:
        state.italic = true;
        idx += 1;
        break;
      case 4:
        state.underline = true;
        idx += 1;
        break;
      case 22:
        state.bold = undefined;
        state.dim = undefined;
        idx += 1;
        break;
      case 23:
        state.italic = undefined;
        idx += 1;
        break;
      case 24:
        state.underline = undefined;
        idx += 1;
        break;
      case 39:
        state.color = undefined;
        idx += 1;
        break;
      case 49:
        state.background = undefined;
        idx += 1;
        break;
      default: {
        const isForeground = code >= 30 && code <= 37;
        const isBackground = code >= 40 && code <= 47;
        if (isForeground || isBackground) {
          const color = ansiColorToCss(code - (isBackground ? 10 : 0));
          if (isForeground) state.color = color;
          if (isBackground) state.background = color;
          idx += 1;
          break;
        }
        const isBrightForeground = code >= 90 && code <= 97;
        const isBrightBackground = code >= 100 && code <= 107;
        if (isBrightForeground || isBrightBackground) {
          const color = ansiColorToCss(code - (isBrightBackground ? 10 : 0));
          if (isBrightForeground) state.color = color;
          if (isBrightBackground) state.background = color;
          idx += 1;
          break;
        }
        if (code === 38 || code === 48) {
          const isBackground = code === 48;
          const mode = codes[idx + 1];
          if (mode === 2) {
            const r = codes[idx + 2] ?? 0;
            const g = codes[idx + 3] ?? 0;
            const b = codes[idx + 4] ?? 0;
            const color = `rgb(${r}, ${g}, ${b})`;
            if (isBackground) state.background = color;
            else state.color = color;
            idx += 5;
            break;
          }
          idx += 2;
          break;
        }
        idx += 1;
      }
    }
  }
}

function ansiColorToCss(code: number): string | undefined {
  const map: Record<number, string> = {
    30: '#1f1f1f',
    31: '#e05f5f',
    32: '#3abf7c',
    33: '#d4a93e',
    34: '#4c7bd9',
    35: '#ad6bd8',
    36: '#46b3c8',
    37: '#d9d9d9',
    90: '#6e6e6e',
    91: '#f27d7d',
    92: '#5cd39d',
    93: '#f4c95d',
    94: '#6a92f3',
    95: '#c190f0',
    96: '#72c5d9',
    97: '#f2f2f2'
  };
  return map[code];
}

function segmentsToHtml(segments: LogSegment[]): string {
  return segments
    .map((segment) => {
      const styles: string[] = [];
      if (segment.color) styles.push(`color:${segment.color}`);
      if (segment.background) styles.push(`background:${segment.background}`);
      if (segment.bold) styles.push('font-weight:600');
      if (segment.dim) styles.push('opacity:0.7');
      if (segment.italic) styles.push('font-style:italic');
      if (segment.underline) styles.push('text-decoration:underline');
      const style = styles.length ? ` style="${styles.join(';')}"` : '';
      return `<span${style}>${escapeHtml(segment.text)}</span>`;
    })
    .join('');
}

function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function extractInlineTimestamp(message: string): InlineTimestampDetails | null {
  const match = message.match(/^\s*\[?(\d{4}-\d{2}-\d{2}T[^\s\]]+)\]?\s*/);
  if (!match?.[1]) return null;
  const value = match[1] ?? '';
  return { value, removeUntil: match[0].length };
}

function extractLevelMarker(segments: LogSegment[]): LevelMarker | null {
  const first = segments[0]?.text?.trim().toLowerCase() ?? '';
  const normalized = normalizeLevelToken(first);
  if (!normalized) return null;
  return { level: normalized, color: segments[0]?.color };
}

function findLevelColor(segments: LogSegment[], level: Exclude<LogFilter, 'all'>): string | undefined {
  const color = segments.find((segment) => segment.color)?.color;
  return color ?? (level === 'error' ? '#f27d7d' : undefined);
}

function resolveLogLevel(
  rawLevel: string | null | undefined,
  fallbackMessage: string,
  marker?: LevelMarker | null
): { level: Exclude<LogFilter, 'all'>; color?: string } {
  const normalized = rawLevel ? normalizeLevelToken(rawLevel) : null;
  if (normalized) return { level: normalized };
  if (marker?.level) return { level: marker.level, color: marker.color };
  const inline = detectLeadingLevelPrefix(fallbackMessage);
  if (inline?.level) return { level: inline.level };
  return { level: 'info' };
}

function trimLeadingSegments(segments: LogSegment[], length: number): LogSegment[] {
  if (length <= 0) return segments;
  const result: LogSegment[] = [];
  let offset = 0;
  const limit = length;
  for (const segment of segments) {
    const segStart = offset;
    const segEnd = offset + segment.text.length;
    if (segEnd <= limit) {
      offset = segEnd;
      continue;
    }
    if (segStart < limit) {
      result.push({ ...segment, text: segment.text.slice(limit - segStart) });
    } else {
      result.push(segment);
    }
    offset = segEnd;
  }
  return result;
}

function normalizeLogFrame(frame: LogFramePayload, fallbackStreamId: string): LogEntry | null {
  const streamId = frame.stream_id ?? fallbackStreamId;
  const identifier = frame.sequence != null ? String(frame.sequence) : frame.timestamp ?? Math.random().toString(36).slice(2);
  const fieldsRaw = frame.fields;
  const fields =
    fieldsRaw && typeof fieldsRaw === 'object' && !Array.isArray(fieldsRaw)
      ? (fieldsRaw as Record<string, unknown>)
      : {};
  const rawMessage = frame.message ?? '(no message)';
  const decorated = decorateLogMessage(rawMessage);
  const resolvedLevel = resolveLogLevel(frame.level, decorated.plainText, decorated.levelMarker);
  const levelColor = resolvedLevel.color ?? findLevelColor(decorated.segments, resolvedLevel.level);
  return {
    id: `${streamId}-${identifier}`,
    level: resolvedLevel.level,
    levelColor,
    threadId: decorated.threadId,
    message: rawMessage,
    timestamp: frame.timestamp ?? '',
    fileId: frame.file_id ?? undefined,
    fields,
    inlineTimestamp: decorated.inlineTimestamp ?? null,
    renderedMessage: decorated.rendered
  };
}

export function parseNdjson(payload: string, fallbackStreamId: string): LogEntry[] {
  const entries: LogEntry[] = [];
  const lines = payload.split('\n');
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const frame = JSON.parse(trimmed) as LogFramePayload & { heartbeat?: boolean };
      if (frame.heartbeat) continue;
      const entry = normalizeLogFrame(frame, fallbackStreamId);
      if (entry) {
        entries.push(entry);
      }
    } catch {
      // ignore malformed frames
    }
  }
  return entries;
}

export function parseLogFrame(raw: string, fallbackStreamId: string): LogEntry[] {
  try {
    const frame = JSON.parse(raw) as LogFramePayload & { heartbeat?: boolean };
    if (frame.heartbeat) return [];
    const entry = normalizeLogFrame(frame, fallbackStreamId);
    return entry ? [entry] : [];
  } catch {
    return [];
  }
}

export function extractStreamError(raw: string | null | undefined): string {
  if (!raw) return 'Log stream reported an error.';
  try {
    const parsed = JSON.parse(raw) as { message?: string };
    return parsed.message ?? 'Log stream reported an error.';
  } catch {
    return raw;
  }
}
