import type { PipelineNodeStyle } from '$lib/types/pipeline';

export type NodeColorPalette = {
  background: string;
  border: string;
  header: string;
};

const FLOW_SURFACE = 'var(--flow-surface, var(--color-surface-900, #0f172a))';
const FLOW_SURFACE_SOFT =
  'var(--flow-surface-soft, color-mix(in srgb, var(--color-surface-800, #1f2937) 82%, transparent))';
const FLOW_BORDER = 'var(--flow-border, var(--color-surface-700, #1f2937))';

export const DEFAULT_NODE_PALETTE: NodeColorPalette = {
  background: 'var(--color-surface-900, #0f172a)',
  border: 'var(--color-surface-800, #1f2937)',
  header: 'var(--color-surface-800, #1e293b)'
};

export const HEX_COLOR_REGEX = /^#([\da-f]{3}|[\da-f]{4}|[\da-f]{6}|[\da-f]{8})$/i;

const THEME_COLOR_ALIASES: Record<string, string> = {
  primary: 'var(--color-primary-400)',
  'primary-soft': 'var(--color-primary-300)',
  'primary-strong': 'var(--color-primary-500)',
  secondary: 'var(--color-secondary-400)',
  accent: 'var(--color-secondary-400)',
  tertiary: 'var(--color-tertiary-400)',
  info: 'var(--color-tertiary-300)',
  success: 'var(--color-success-400)',
  warning: 'var(--color-warning-400)',
  danger: 'var(--color-error-500)',
  error: 'var(--color-error-500)',
  neutral: 'var(--color-surface-500)',
  'neutral-soft': 'var(--color-surface-400)',
  'neutral-strong': 'var(--color-surface-600)',
  surface: 'var(--color-surface-700)',
  'surface-soft': 'var(--color-surface-600)',
  'surface-strong': 'var(--color-surface-800)',
  bg: 'var(--flow-surface, var(--color-surface-900))',
  'bg-soft': 'var(--flow-surface-soft, color-mix(in srgb, var(--color-surface-800) 82%, transparent))',
  border: 'var(--flow-border, var(--color-surface-600))',
  text: 'var(--flow-text, var(--color-surface-50))',
  edge: 'var(--flow-edge-highlight, var(--color-secondary-400))'
};

const SCALABLE_THEME_BASES = new Set([
  'primary',
  'secondary',
  'tertiary',
  'success',
  'warning',
  'error',
  'surface'
]);

const isCssFunction = (value: string): boolean =>
  value.startsWith('var(') ||
  value.startsWith('rgb') ||
  value.startsWith('hsl') ||
  value.startsWith('oklch') ||
  value.startsWith('color(') ||
  value.startsWith('color-mix(') ||
  value.startsWith('linear-gradient(');

export const clamp01 = (value: number): number => Math.max(0, Math.min(1, value));

const blendNodeSurface = (
  target: string | null,
  surface: string,
  ratio: number,
  fallback: string
): string => {
  const color = target ?? fallback;
  const weight = Math.round(clamp01(ratio) * 100);
  const complement = 100 - weight;
  if (weight <= 0) {
    return surface;
  }
  if (weight >= 100) {
    return color;
  }
  return `color-mix(in srgb, ${color} ${weight}%, ${surface} ${complement}%)`;
};

export function normalizeColor(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

const normalizeStyleCandidate = (candidate: PipelineNodeStyle | null | undefined): PipelineNodeStyle | null => {
  if (!candidate || typeof candidate !== 'object') {
    return null;
  }
  const bgRaw =
    (candidate as { bg_color?: string | null; bgColor?: string | null }).bg_color ??
    (candidate as { bgColor?: string | null }).bgColor ??
    null;
  const borderRaw =
    (candidate as { border_color?: string | null; borderColor?: string | null }).border_color ??
    (candidate as { borderColor?: string | null }).borderColor ??
    null;
  const bg = normalizeColor(bgRaw);
  const border = normalizeColor(borderRaw);
  if (!bg && !border) {
    return null;
  }
  return {
    bg_color: bg ?? '',
    border_color: border ?? ''
  };
};

export function resolveThemeColor(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!trimmed) return null;
  if (isCssFunction(trimmed) || HEX_COLOR_REGEX.test(trimmed)) return trimmed;

  let token = trimmed.toLowerCase();
  if (token.startsWith('theme:') || token.startsWith('token:')) {
    token = token.slice(token.indexOf(':') + 1);
  } else if (token.startsWith('@')) {
    token = token.slice(1);
  }

  if (THEME_COLOR_ALIASES[token]) {
    return THEME_COLOR_ALIASES[token];
  }

  if (token.endsWith('-soft')) {
    const base = token.slice(0, -5);
    if (SCALABLE_THEME_BASES.has(base)) {
      return `var(--color-${base}-300)`;
    }
  }

  if (token.endsWith('-strong')) {
    const base = token.slice(0, -7);
    if (SCALABLE_THEME_BASES.has(base)) {
      return `var(--color-${base}-500)`;
    }
  }

  const contrastMatch = token.match(
    /^(primary|secondary|tertiary|success|warning|error|surface)(-contrast)?-(\d{2,3}|950)$/u
  );
  if (contrastMatch) {
    const [, base, contrast, scale] = contrastMatch;
    const contrastSegment = contrast ?? '';
    return `var(--color-${base}${contrastSegment}-${scale})`;
  }

  if (SCALABLE_THEME_BASES.has(token)) {
    return `var(--color-${token}-400)`;
  }

  if (token === 'bg') {
    return THEME_COLOR_ALIASES.bg;
  }

  return trimmed;
}

export const mergeNodeStyles = (
  ...styles: Array<PipelineNodeStyle | null | undefined>
): PipelineNodeStyle | null => {
  let background: string | null = null;
  let border: string | null = null;
  for (const candidate of styles) {
    const normalized = normalizeStyleCandidate(candidate);
    if (!normalized) continue;
    if (!background) {
      const resolvedBg = normalizeColor(normalized.bg_color);
      if (resolvedBg) {
        background = resolvedBg;
      }
    }
    if (!border) {
      const resolvedBorder = normalizeColor(normalized.border_color);
      if (resolvedBorder) {
        border = resolvedBorder;
      }
    }
    if (background && border) {
      break;
    }
  }
  if (!background && !border) {
    return null;
  }
  return {
    bg_color: background ?? '',
    border_color: border ?? ''
  };
};

export type PaletteEmphasis = 'normal' | 'muted' | 'heatmap';

const EMPHASIS_RATIOS: Record<PaletteEmphasis, { background: number; header: number; border: number }> = {
  normal: { background: 0.42, header: 0.3, border: 0.28 },
  muted: { background: 0.18, header: 0.15, border: 0.2 },
  heatmap: { background: 0.08, header: 0.12, border: 0.14 }
};

export function resolveNodePalette(
  style: PipelineNodeStyle | null | undefined,
  emphasis: PaletteEmphasis = 'normal'
): NodeColorPalette {
  const normalized = normalizeStyleCandidate(style);
  if (!normalized) return DEFAULT_NODE_PALETTE;
  const bg = normalized.bg_color;
  const border = normalized.border_color;
  const backgroundBase = resolveThemeColor(bg ?? null);
  const borderBase = resolveThemeColor(border ?? null);
  const { background: backgroundRatio, header: headerRatio, border: borderRatio } = EMPHASIS_RATIOS[emphasis];

  const treatedBackground = backgroundBase
    ? blendNodeSurface(backgroundBase, FLOW_SURFACE_SOFT, backgroundRatio, DEFAULT_NODE_PALETTE.background)
    : DEFAULT_NODE_PALETTE.background;
  const treatedHeader = blendNodeSurface(
    backgroundBase ?? DEFAULT_NODE_PALETTE.header,
    FLOW_SURFACE,
    headerRatio,
    DEFAULT_NODE_PALETTE.header
  );
  const treatedBorder =
    borderBase || backgroundBase
      ? blendNodeSurface(
          borderBase ?? backgroundBase ?? DEFAULT_NODE_PALETTE.border,
          FLOW_BORDER,
          borderRatio,
          DEFAULT_NODE_PALETTE.border
        )
      : DEFAULT_NODE_PALETTE.border;

  return {
    background: treatedBackground,
    border: treatedBorder,
    header: treatedHeader
  };
}

export type NodePaletteInput = {
  style: PipelineNodeStyle | null;
  emphasis: PaletteEmphasis;
};

export function deriveNodePaletteInput(
  style: PipelineNodeStyle | null | undefined,
  heatmapMode: boolean
): NodePaletteInput {
  return {
    style: style ?? null,
    emphasis: heatmapMode ? 'heatmap' : 'normal'
  };
}

export function buildNodePalette(
  style: PipelineNodeStyle | null | undefined,
  heatmapMode: boolean
): NodeColorPalette {
  const { style: effectiveStyle, emphasis } = deriveNodePaletteInput(style, heatmapMode);
  return resolveNodePalette(effectiveStyle, emphasis);
}
