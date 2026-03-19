import type { LogSegment, SegmentState } from './types';

const ANSI_ESCAPE_REGEX = new RegExp(`${String.fromCharCode(27)}\\[((?:\\d{1,3};?)*)m`, 'g');

export function parseAnsiSegments(raw: string): LogSegment[] {
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
    pushSegment(raw.slice(lastIndex, match.index));
    lastIndex = ANSI_ESCAPE_REGEX.lastIndex;
    applyAnsiCodes(match[1], state);
  }
  pushSegment(raw.slice(lastIndex));

  if (!segments.length) {
    return [{ text: raw }];
  }
  return segments;
}

export function trimLeadingSegments(segments: LogSegment[], removeChars: number): LogSegment[] {
  if (removeChars <= 0) return segments;
  const result: LogSegment[] = [];
  let remaining = removeChars;
  for (const segment of segments) {
    const textLength = segment.text.length;
    if (remaining >= textLength) {
      remaining -= textLength;
      continue;
    }
    if (remaining > 0) {
      const trimmedText = segment.text.slice(remaining);
      remaining = 0;
      result.push({ ...segment, text: trimmedText });
    } else {
      result.push(segment);
    }
  }
  return result.filter((segment) => segment.text.length > 0);
}

export function removeSegmentRange(segments: LogSegment[], start: number, length: number): LogSegment[] {
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

export function segmentsToHtml(segments: LogSegment[]): string {
  if (!segments.length) return '';
  const parts: string[] = [];
  for (const segment of segments) {
    const classes: string[] = [];
    if (segment.bold) {
      classes.push('font-semibold');
    }
    if (segment.italic) {
      classes.push('italic');
    }
    if (segment.underline) {
      classes.push('underline decoration-dotted');
    }
    if (segment.dim) {
      classes.push('opacity-70');
    }
    const styleParts: string[] = [];
    if (segment.color) {
      styleParts.push(`color:${segment.color}`);
    }
    if (segment.background) {
      styleParts.push(`background-color:${segment.background}`);
    }
    const attrClass = classes.length ? ` class="${classes.join(' ')}"` : '';
    const attrStyle = styleParts.length ? ` style="${styleParts.join(';')}"` : '';
    parts.push(`<span${attrClass}${attrStyle}>${escapeHtml(segment.text)}</span>`);
  }
  return parts.join('');
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function applyAnsiCodes(payload: string, state: SegmentState): void {
  const codes = payload
    .split(';')
    .map((code) => Number(code))
    .map((code) => (Number.isNaN(code) ? 0 : code));
  if (!codes.length || (codes.length === 1 && codes[0] === 0)) {
    resetSegmentState(state);
    return;
  }
  for (let i = 0; i < codes.length; i++) {
    const code = codes[i];
    switch (code) {
      case 0:
        resetSegmentState(state);
        break;
      case 1:
        state.bold = true;
        break;
      case 2:
        state.dim = true;
        break;
      case 3:
        state.italic = true;
        break;
      case 4:
        state.underline = true;
        break;
      case 21:
      case 22:
        state.bold = undefined;
        state.dim = undefined;
        break;
      case 23:
        state.italic = undefined;
        break;
      case 24:
        state.underline = undefined;
        break;
      case 39:
        state.color = undefined;
        break;
      case 49:
        state.background = undefined;
        break;
      case 38: {
        const mode = codes[i + 1];
        if (mode === 5 && typeof codes[i + 2] === 'number') {
          const colorIdx = codes[i + 2];
          state.color = ansi256ToHex(colorIdx);
          i += 2;
          break;
        }
        if (mode === 2 && typeof codes[i + 2] === 'number' && typeof codes[i + 3] === 'number' && typeof codes[i + 4] === 'number') {
          const [r, g, b] = [codes[i + 2], codes[i + 3], codes[i + 4]];
          state.color = `rgb(${r}, ${g}, ${b})`;
          i += 4;
          break;
        }
        break;
      }
      case 48: {
        const mode = codes[i + 1];
        if (mode === 5 && typeof codes[i + 2] === 'number') {
          state.background = ansi256ToHex(codes[i + 2]);
          i += 2;
          break;
        }
        if (mode === 2 && typeof codes[i + 2] === 'number' && typeof codes[i + 3] === 'number' && typeof codes[i + 4] === 'number') {
          const [r, g, b] = [codes[i + 2], codes[i + 3], codes[i + 4]];
          state.background = `rgb(${r}, ${g}, ${b})`;
          i += 4;
          break;
        }
        break;
      }
      default:
        if ((code >= 30 && code <= 37) || (code >= 90 && code <= 97)) {
          state.color = basicAnsiColor(code);
        } else if ((code >= 40 && code <= 47) || (code >= 100 && code <= 107)) {
          state.background = basicAnsiColor(code - 10);
        }
        break;
    }
  }
}

function resetSegmentState(state: SegmentState): void {
  state.color = undefined;
  state.background = undefined;
  state.bold = undefined;
  state.dim = undefined;
  state.italic = undefined;
  state.underline = undefined;
}

function basicAnsiColor(code: number): string | undefined {
  const map: Record<number, string> = {
    30: '#94a3b8',
    31: '#f87171',
    32: '#4ade80',
    33: '#facc15',
    34: '#60a5fa',
    35: '#f472b6',
    36: '#5eead4',
    37: '#f8fafc',
    90: '#cbd5f5',
    91: '#ff9ca3',
    92: '#9ae6b4',
    93: '#fde68a',
    94: '#a5b4fc',
    95: '#fbcfe8',
    96: '#a7f3d0',
    97: '#ffffff'
  };
  return map[code];
}

function ansi256ToHex(code: number): string {
  if (code == null || Number.isNaN(code)) return '#f8fafc';
  if (code >= 0 && code <= 15) {
    const basic = [
      '#000000',
      '#800000',
      '#008000',
      '#808000',
      '#000080',
      '#800080',
      '#008080',
      '#c0c0c0',
      '#808080',
      '#ff0000',
      '#00ff00',
      '#ffff00',
      '#0000ff',
      '#ff00ff',
      '#00ffff',
      '#ffffff'
    ];
    return basic[code] ?? '#f8fafc';
  }
  if (code >= 16 && code <= 231) {
    const idx = code - 16;
    const r = Math.floor(idx / 36);
    const g = Math.floor((idx % 36) / 6);
    const b = idx % 6;
    const palette = [0, 95, 135, 175, 215, 255];
    return rgbToHex(palette[r], palette[g], palette[b]);
  }
  if (code >= 232 && code <= 255) {
    const shade = 8 + (code - 232) * 10;
    return rgbToHex(shade, shade, shade);
  }
  return '#f8fafc';
}

function rgbToHex(r: number, g: number, b: number): string {
  const clamp = (value: number) => Math.max(0, Math.min(255, value));
  return `#${[clamp(r), clamp(g), clamp(b)]
    .map((value) => value.toString(16).padStart(2, '0'))
    .join('')}`;
}
