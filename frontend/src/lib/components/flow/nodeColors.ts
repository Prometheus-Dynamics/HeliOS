import { clamp01, HEX_COLOR_REGEX } from './nodePalette';

export type RgbColor = {
  r: number;
  g: number;
  b: number;
};

export type NodeTextPalette = {
  strong: string;
  emphasis: string;
  muted: string;
  subtle: string;
  badgeBackground: string;
  badgeBorder: string;
  badgeText: string;
};

const LIGHT_ON_DARK_NODE_TEXT_PALETTE: NodeTextPalette = {
  strong: '#f8fafc',
  emphasis: '#f1f5f9',
  muted: 'rgba(248, 250, 252, 0.82)',
  subtle: 'rgba(248, 250, 252, 0.65)',
  badgeBackground: 'rgba(15, 23, 42, 0.75)',
  badgeBorder: 'rgba(51, 65, 85, 0.6)',
  badgeText: '#f8fafc'
};

const DARK_ON_LIGHT_NODE_TEXT_PALETTE: NodeTextPalette = {
  strong: '#0f172a',
  emphasis: '#111827',
  muted: 'rgba(15, 23, 42, 0.78)',
  subtle: 'rgba(15, 23, 42, 0.6)',
  badgeBackground: 'rgba(15, 23, 42, 0.08)',
  badgeBorder: 'rgba(15, 23, 42, 0.25)',
  badgeText: '#0f172a'
};

const REFERENCE_LIGHT_COLOR: RgbColor = { r: 248, g: 250, b: 252 };
const REFERENCE_DARK_COLOR: RgbColor = { r: 15, g: 23, b: 42 };
const REFERENCE_LIGHT_LUMINANCE = relativeLuminance(REFERENCE_LIGHT_COLOR);
const REFERENCE_DARK_LUMINANCE = relativeLuminance(REFERENCE_DARK_COLOR);

const VAR_FUNCTION_PATTERN = /^var\(/iu;
const RGB_FUNCTION_PATTERN = /^rgba?\(/iu;
const HSL_FUNCTION_PATTERN = /^hsla?\(/iu;

export function clampChannel(value: unknown): number | null {
  let numeric: number;
  if (typeof value === 'number') {
    numeric = value;
  } else {
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) return null;
    numeric = parsed;
  }
  if (!Number.isFinite(numeric)) return null;
  const rounded = Math.round(numeric);
  if (rounded < 0 || rounded > 255) {
    return Math.min(255, Math.max(0, rounded));
  }
  return rounded;
}

export function resolveNodeTextPalette(background: string | null | undefined): NodeTextPalette {
  const parsed = parseCssColor(background);
  if (!parsed) {
    return LIGHT_ON_DARK_NODE_TEXT_PALETTE;
  }
  const luminance = relativeLuminance(parsed);
  const contrastWithLight = computeContrastRatio(luminance, REFERENCE_LIGHT_LUMINANCE);
  const contrastWithDark = computeContrastRatio(luminance, REFERENCE_DARK_LUMINANCE);
  if (contrastWithDark >= contrastWithLight) {
    return DARK_ON_LIGHT_NODE_TEXT_PALETTE;
  }
  return LIGHT_ON_DARK_NODE_TEXT_PALETTE;
}

function parseCssColor(value: string | null | undefined): RgbColor | null {
  if (!value) return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  const lower = trimmed.toLowerCase();
  if (lower === 'transparent' || lower === 'currentcolor') {
    return null;
  }
  if (VAR_FUNCTION_PATTERN.test(trimmed)) {
    const fallback = extractVarFallback(trimmed);
    if (fallback) {
      return parseCssColor(fallback);
    }
    return null;
  }
  if (HEX_COLOR_REGEX.test(trimmed)) {
    return parseHexColor(trimmed);
  }
  if (RGB_FUNCTION_PATTERN.test(trimmed)) {
    return parseRgbFunctionColor(trimmed);
  }
  if (HSL_FUNCTION_PATTERN.test(trimmed)) {
    return parseHslFunctionColor(trimmed);
  }
  return null;
}

function extractVarFallback(expression: string): string | null {
  const start = expression.indexOf('(');
  const end = expression.lastIndexOf(')');
  if (start === -1 || end === -1 || end <= start + 1) {
    return null;
  }
  const inner = expression.slice(start + 1, end);
  let depth = 0;
  for (let index = 0; index < inner.length; index += 1) {
    const char = inner[index];
    if (char === '(') {
      depth += 1;
    } else if (char === ')') {
      if (depth === 0) {
        return null;
      }
      depth -= 1;
    } else if (char === ',' && depth === 0) {
      return inner.slice(index + 1).trim();
    }
  }
  return null;
}

function parseHexColor(value: string): RgbColor | null {
  const hex = value.slice(1);
  const expand = (segment: string): number =>
    parseInt(segment.length === 1 ? `${segment}${segment}` : segment, 16);
  if (hex.length === 3 || hex.length === 4) {
    const [r, g, b] = [expand(hex[0]), expand(hex[1]), expand(hex[2])];
    if ([r, g, b].some((channel) => Number.isNaN(channel))) {
      return null;
    }
    return { r, g, b };
  }
  if (hex.length === 6 || hex.length === 8) {
    const r = parseInt(hex.slice(0, 2), 16);
    const g = parseInt(hex.slice(2, 4), 16);
    const b = parseInt(hex.slice(4, 6), 16);
    if ([r, g, b].some((channel) => Number.isNaN(channel))) {
      return null;
    }
    return { r, g, b };
  }
  return null;
}

function parseRgbFunctionColor(value: string): RgbColor | null {
  const args = extractColorFunctionArgs(value);
  if (args.length < 3) {
    return null;
  }
  const parseComponent = (component: string): number | null => {
    const trimmed = component.trim();
    if (!trimmed) return null;
    if (trimmed.endsWith('%')) {
      const numeric = Number.parseFloat(trimmed.slice(0, -1));
      if (!Number.isFinite(numeric)) return null;
      const scaled = clamp01(numeric / 100) * 255;
      return clampChannel(Math.round(scaled));
    }
    const numeric = Number.parseFloat(trimmed);
    if (!Number.isFinite(numeric)) return null;
    return clampChannel(numeric);
  };
  const r = parseComponent(args[0]);
  const g = parseComponent(args[1]);
  const b = parseComponent(args[2]);
  if (r == null || g == null || b == null) {
    return null;
  }
  return { r, g, b };
}

function parseHslFunctionColor(value: string): RgbColor | null {
  const args = extractColorFunctionArgs(value);
  if (args.length < 3) {
    return null;
  }
  const hue = parseHue(args[0]);
  const saturation = parsePercentageChannel(args[1]);
  const lightness = parsePercentageChannel(args[2]);
  if (hue == null || saturation == null || lightness == null) {
    return null;
  }
  return hslToRgbColor(hue, saturation, lightness);
}

function extractColorFunctionArgs(value: string): string[] {
  const start = value.indexOf('(');
  const end = value.lastIndexOf(')');
  if (start === -1 || end === -1 || end <= start + 1) {
    return [];
  }
  const inner = value.slice(start + 1, end);
  const [argumentsSegment] = inner.split('/');
  if (!argumentsSegment) {
    return [];
  }
  if (argumentsSegment.includes(',')) {
    return argumentsSegment
      .split(',')
      .map((segment) => segment.trim())
      .filter(Boolean);
  }
  return argumentsSegment
    .split(/\s+/u)
    .map((segment) => segment.trim())
    .filter(Boolean);
}

function parseHue(segment: string): number | null {
  if (!segment) return null;
  let normalized = segment.trim().toLowerCase();
  if (!normalized) return null;
  let multiplier = 1;
  if (normalized.endsWith('deg')) {
    normalized = normalized.slice(0, -3);
  } else if (normalized.endsWith('rad')) {
    multiplier = 180 / Math.PI;
    normalized = normalized.slice(0, -3);
  } else if (normalized.endsWith('turn')) {
    multiplier = 360;
    normalized = normalized.slice(0, -4);
  }
  const numeric = Number.parseFloat(normalized);
  if (!Number.isFinite(numeric)) return null;
  const result = (numeric * multiplier) % 360;
  return result >= 0 ? result : result + 360;
}

function parsePercentageChannel(segment: string): number | null {
  if (!segment) return null;
  const normalized = segment.trim();
  if (!normalized) return null;
  if (normalized.endsWith('%')) {
    const numeric = Number.parseFloat(normalized.slice(0, -1));
    if (!Number.isFinite(numeric)) return null;
    return clamp01(numeric / 100);
  }
  const numeric = Number.parseFloat(normalized);
  if (!Number.isFinite(numeric)) return null;
  return clamp01(numeric);
}

function hslToRgbColor(h: number, s: number, l: number): RgbColor {
  const chroma = (1 - Math.abs(2 * l - 1)) * s;
  const hueSegment = h / 60;
  const secondComponent = chroma * (1 - Math.abs((hueSegment % 2) - 1));
  let r1 = 0;
  let g1 = 0;
  let b1 = 0;
  if (hueSegment >= 0 && hueSegment < 1) {
    r1 = chroma;
    g1 = secondComponent;
  } else if (hueSegment >= 1 && hueSegment < 2) {
    r1 = secondComponent;
    g1 = chroma;
  } else if (hueSegment >= 2 && hueSegment < 3) {
    g1 = chroma;
    b1 = secondComponent;
  } else if (hueSegment >= 3 && hueSegment < 4) {
    g1 = secondComponent;
    b1 = chroma;
  } else if (hueSegment >= 4 && hueSegment < 5) {
    r1 = secondComponent;
    b1 = chroma;
  } else if (hueSegment >= 5 && hueSegment < 6) {
    r1 = chroma;
    b1 = secondComponent;
  }
  const match = l - chroma / 2;
  const r = clampChannel(Math.round((r1 + match) * 255));
  const g = clampChannel(Math.round((g1 + match) * 255));
  const b = clampChannel(Math.round((b1 + match) * 255));
  return {
    r: r ?? 0,
    g: g ?? 0,
    b: b ?? 0
  };
}

function relativeLuminance(color: RgbColor): number {
  const normalize = (channel: number): number => {
    const scaled = clamp01(channel / 255);
    if (scaled <= 0.04045) {
      return scaled / 12.92;
    }
    return Math.pow((scaled + 0.055) / 1.055, 2.4);
  };
  const r = normalize(color.r);
  const g = normalize(color.g);
  const b = normalize(color.b);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function computeContrastRatio(l1: number, l2: number): number {
  const [lighter, darker] = l1 > l2 ? [l1, l2] : [l2, l1];
  return (lighter + 0.05) / (darker + 0.05);
}
