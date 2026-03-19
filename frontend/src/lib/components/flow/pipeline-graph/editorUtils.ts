import { serializeGraphPlan } from '$lib/features/pipelines/graph';
import type {
  PipelineGraphPlan,
  PipelineNodeValue
} from '$lib/types/pipeline';
import { clonePlan, ensurePlanPortMetadata } from './utils';
import { BackgroundVariant } from '@xyflow/svelte';
import {
  buildGraphDiagnostics,
  findNodeEntryForId,
  matchPortInNode,
  normalizeIdentifier
} from './diagnostics';

export type PipelineGraphTheme = {
  backgroundVariant: BackgroundVariant;
  backgroundGap: number;
  backgroundSize: number;
  backgroundColor: string;
  surface: string;
  surfaceSoft: string;
  border: string;
  text: string;
  accent: string;
  edgeHighlight: string;
  showControls: boolean;
  controlsPosition: 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';
};

export const DEFAULT_THEME: PipelineGraphTheme = {
  backgroundVariant: BackgroundVariant.Dots,
  backgroundGap: 24,
  backgroundSize: 1,
  backgroundColor: 'var(--color-surface-800, #1f2937)',
  surface: 'var(--color-surface-900, #0f172a)',
  surfaceSoft: 'color-mix(in srgb, var(--color-surface-900, #0f172a) 82%, transparent)',
  border: 'var(--color-surface-700, #1f2937)',
  text: 'var(--base-font-color-dark, #f8fafc)',
  accent: 'var(--color-primary-300, #38bdf8)',
  edgeHighlight: 'var(--color-secondary-400, #a855f7)',
  showControls: true,
  controlsPosition: 'bottom-right'
};

export const mergeTheme = (overrides?: Partial<PipelineGraphTheme>): PipelineGraphTheme => ({
  ...DEFAULT_THEME,
  ...overrides
});

const BOOLEAN_TYPE_KEYS = new Set(['bool', 'boolean']);
const NUMERIC_INTEGER_KEYS = new Set([
  'uint',
  'sint',
  'int',
  'int8',
  'uint8',
  'int16',
  'uint16',
  'int32',
  'uint32',
  'int64',
  'uint64',
  'i8',
  'u8',
  'i16',
  'u16',
  'i32',
  'u32',
  'i64',
  'u64',
  'isize',
  'usize'
]);
const NUMERIC_FLOAT_KEYS = new Set(['float', 'double', 'number', 'f16', 'f32', 'f64', 'f128']);

export type PixelColor = { r: number; g: number; b: number; a: number };

export const DEFAULT_PIXEL_COLOR: PixelColor = { r: 255, g: 255, b: 255, a: 255 };
export const DEFAULT_NUMERIC_VALUE = 0;
export const HISTORY_LIMIT = 100;

export type GraphNodeEntry = { key: string; node: PipelineGraphPlan['nodes'][string] };
export type PlanHistoryEntry = {
  plan: PipelineGraphPlan;
  signature: string;
};

export { buildGraphDiagnostics, findNodeEntryForId, matchPortInNode, normalizeIdentifier };

const stableStringify = (value: unknown): string => {
  if (value === null || value === undefined) return 'null';
  if (typeof value === 'number' || typeof value === 'boolean') {
    return JSON.stringify(value);
  }
  if (typeof value === 'string') {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map((item) => stableStringify(item)).join(',')}]`;
  }
  if (typeof value === 'object') {
    const record = value as Record<string, unknown>;
    const entries = Object.keys(record)
      .filter((key) => record[key] !== undefined)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(record[key])}`);
    return `{${entries.join(',')}}`;
  }
  return 'null';
};

export const createPlanSignature = (graph: PipelineGraphPlan): string => {
  const serialized = serializeGraphPlan(graph);
  return stableStringify(serialized);
};

export const createHistoryEntry = (graph: PipelineGraphPlan): PlanHistoryEntry => {
  const snapshot = ensurePlanPortMetadata(clonePlan(graph));
  return {
    plan: snapshot,
    signature: createPlanSignature(snapshot)
  };
};

export const normalizeTypeKey = (raw: string | null | undefined): string | null => {
  if (!raw) return null;
  const trimmed = raw.trim().toLowerCase();
  if (!trimmed) return null;
  const separator = trimmed.lastIndexOf(':');
  return separator >= 0 ? trimmed.slice(separator + 1) : trimmed;
};

export const isBooleanTypeKey = (key: string | null | undefined): boolean => {
  const normalized = normalizeTypeKey(key);
  return normalized != null && BOOLEAN_TYPE_KEYS.has(normalized);
};

export const isPixelTypeKey = (key: string | null | undefined): boolean => {
  const normalized = normalizeTypeKey(key);
  if (!normalized) return false;
  return normalized.endsWith('pixel') || normalized === 'pixel' || normalized.includes('pixel');
};

export const isIntegerNumericTypeKey = (key: string | null | undefined): boolean => {
  const normalized = normalizeTypeKey(key);
  if (!normalized) return false;
  return NUMERIC_INTEGER_KEYS.has(normalized);
};

export const isNumericTypeKey = (key: string | null | undefined): boolean => {
  const normalized = normalizeTypeKey(key);
  if (!normalized) return false;
  return NUMERIC_INTEGER_KEYS.has(normalized) || NUMERIC_FLOAT_KEYS.has(normalized);
};

const clampByte = (value: unknown): number | null => {
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
};

const parsePixelRecord = (raw: unknown): PixelColor | null => {
  if (!raw || typeof raw !== 'object') return null;
  const record = raw as Record<string, unknown>;
  const r = clampByte(record.r);
  const g = clampByte(record.g);
  const b = clampByte(record.b);
  if (r == null || g == null || b == null) return null;
  const a = clampByte(record.a);
  return { r, g, b, a: a ?? 255 };
};

export const parsePixelDraft = (draft: string): PixelColor | null => {
  if (!draft || !draft.trim()) return null;
  try {
    return parsePixelRecord(JSON.parse(draft));
  } catch {
    return null;
  }
};

export const parsePixelValue = (value: PipelineNodeValue | null): PixelColor | null => {
  if (!value) return null;
  return parsePixelRecord(value.value);
};

export const pixelToHex = (pixel: PixelColor): string => {
  const toHex = (channel: number) => channel.toString(16).padStart(2, '0');
  return `#${toHex(pixel.r)}${toHex(pixel.g)}${toHex(pixel.b)}`;
};

export const hexToRgb = (hex: string): Pick<PixelColor, 'r' | 'g' | 'b'> | null => {
  if (!hex || !/^#([\da-f]{6})$/i.test(hex)) return null;
  const normalized = hex.replace('#', '');
  return {
    r: Number.parseInt(normalized.slice(0, 2), 16),
    g: Number.parseInt(normalized.slice(2, 4), 16),
    b: Number.parseInt(normalized.slice(4, 6), 16)
  };
};

export const extractNumericValue = (value: PipelineNodeValue | null | undefined): number | null => {
  if (!value) return null;
  if (typeof value.value === 'number' && Number.isFinite(value.value)) {
    return value.value;
  }
  return null;
};
