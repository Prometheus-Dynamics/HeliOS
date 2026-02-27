import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';

type ParseResult = { success: true; value: unknown } | { success: false; error: string };

const NUMERIC_ERROR = 'Enter a numeric value';

const INTEGER_ERROR = 'Enter an integer value';

const BOOLEAN_ERROR = 'Enter either true or false';

const JSON_ERROR = 'Enter a valid JSON value';

const EMPTY_ERROR = 'Value is required';
const ENUM_INLINE_REGEX = /^enum\s*<\s*(.+)\s*>$/iu;
const OPTIONAL_INLINE_REGEX = /^optional\s*<\s*(.+)\s*>$/iu;
const ENUM_VARIANT_DELIMITER = '|';
const NON_SETTABLE_TYPE_KEYS = new Set(['image', 'gray', 'grey', 'binary', 'metrics', 'tensor']);

const unwrapOptionalTypeString = (raw: string): string => {
  let key = raw.trim();
  while (true) {
    const match = key.match(OPTIONAL_INLINE_REGEX);
    if (!match?.[1]) break;
    key = match[1].trim();
  }
  return key;
};

const isOptionalTypeKey = (raw: string | null | undefined): boolean => {
  if (typeof raw !== 'string') return false;
  const trimmed = raw.trim();
  if (!trimmed) return false;
  return trimmed.toLowerCase() === 'optional' || OPTIONAL_INLINE_REGEX.test(trimmed);
};

const isInlineEnumType = (raw: string | null | undefined): boolean => {
  if (typeof raw !== 'string') return false;
  return ENUM_INLINE_REGEX.test(raw.trim());
};

const normalizeTypeKeyLoose = (key: string | null | undefined): string | null => {
  if (typeof key !== 'string') return null;
  const trimmed = key.trim().toLowerCase();
  if (!trimmed) return null;
  const colonIndex = trimmed.lastIndexOf(':');
  return colonIndex >= 0 ? trimmed.slice(colonIndex + 1) : trimmed;
};

export type EnumVariantParts = {
  label: string;
  value: string;
};

export function parseEnumVariant(raw: string | null | undefined): EnumVariantParts {
  if (typeof raw !== 'string') {
    return { label: raw ?? '', value: raw ?? '' };
  }
  const trimmed = raw.trim();
  const [maybeLabel, maybeValue] = trimmed.split(ENUM_VARIANT_DELIMITER, 2);
  if (maybeValue === undefined) {
    return { label: trimmed, value: trimmed };
  }
  const label = maybeLabel.trim();
  const value = maybeValue.trim();
  return {
    label: label.length ? label : value,
    value: value.length ? value : label
  };
}

export function resolveDataTypeKey(dataType: PipelineDataType | undefined): string | null {
  if (!dataType) return null;
  if (typeof dataType === 'string') {
    const trimmed = unwrapOptionalTypeString(dataType);
    if (!trimmed) return null;
    if (isInlineEnumType(trimmed)) {
      return 'enum';
    }
    return trimmed;
  }
  if (typeof dataType.kind === 'string' && dataType.kind.trim()) {
    const kind = unwrapOptionalTypeString(dataType.kind);
    if (isInlineEnumType(kind)) {
      return 'enum';
    }
    if (kind.toLowerCase() !== 'optional') {
      return kind;
    }
  }
  if (isOptionalTypeKey(dataType.kind)) {
    const elementKey = resolveDataTypeKey(dataType.element as PipelineDataType | undefined);
    if (elementKey) {
      return elementKey;
    }
  }
  const descriptorId = dataType.descriptor?.id;
  if (typeof descriptorId === 'string' && descriptorId.trim()) {
    const trimmed = unwrapOptionalTypeString(descriptorId);
    if (isInlineEnumType(trimmed)) {
      return 'enum';
    }
    return trimmed;
  }
  const descriptorKind = (dataType.descriptor as { kind?: string } | undefined)?.kind;
  if (typeof descriptorKind === 'string' && descriptorKind.trim()) {
    const trimmed = unwrapOptionalTypeString(descriptorKind);
    if (isInlineEnumType(trimmed)) {
      return 'enum';
    }
    if (trimmed.toLowerCase() !== 'optional') {
      return trimmed;
    }
  }
  const elementKey = resolveDataTypeKey(dataType.element as PipelineDataType | undefined);
  if (elementKey) {
    return elementKey;
  }
  return null;
}

const imageFlavorFromLabel = (label: string | null | undefined): string | null => {
  if (!label) return null;
  const match = label.trim().match(/^image\s*\(([^)]+)\)\s*$/iu);
  return match?.[1]?.trim() ?? null;
};

export function normalizeImageFlavorType(dataType: PipelineDataType | undefined): PipelineDataType | undefined {
  if (!dataType) return dataType;
  if (typeof dataType === 'string') {
    const trimmed = dataType.trim();
    if (!trimmed) return dataType;
    const flavor = imageFlavorFromLabel(trimmed);
    if (!flavor) return dataType;
    return `image:${flavor.toLowerCase()}`;
  }

  const label =
    (typeof dataType.label === 'string' ? dataType.label.trim() : '') ||
    (typeof (dataType.descriptor as { label?: string } | undefined)?.label === 'string'
      ? (dataType.descriptor as { label?: string }).label!.trim()
      : '');
  const flavor = imageFlavorFromLabel(label);
  if (!flavor) return dataType;

  const normalizedKind = `image:${flavor.toLowerCase()}`;
  if (dataType.kind && dataType.kind.trim().toLowerCase() === normalizedKind) {
    return dataType;
  }

  const next = { ...(dataType as Exclude<PipelineDataType, string>) } as Exclude<PipelineDataType, string>;
  const descriptor = Object.getOwnPropertyDescriptor(dataType as object, 'descriptor');
  if (descriptor?.value !== undefined) {
    Object.defineProperty(next as object, 'descriptor', {
      value: descriptor.value,
      enumerable: false,
      configurable: true,
      writable: true
    });
  }
  next.kind = normalizedKind;
  if (!next.label || !next.label.trim()) {
    next.label = `Image (${flavor})`;
  }
  return next;
}

export function isDataTypeSettable(dataType: PipelineDataType | undefined): boolean {
  if (!dataType) return false;
  const typeKey = resolveDataTypeKey(dataType)?.toLowerCase() ?? null;
  const explicitSettable =
    typeof dataType === 'object' && typeof (dataType as { settable?: boolean }).settable === 'boolean'
      ? (dataType as { settable: boolean }).settable
      : typeof (dataType as { descriptor?: { settable?: boolean } }).descriptor?.settable === 'boolean'
        ? (dataType as { descriptor?: { settable: boolean } }).descriptor!.settable
        : null;
  if (typeof explicitSettable === 'boolean') {
    return explicitSettable;
  }
  const looseKey = normalizeTypeKeyLoose(typeKey);
  if ((typeKey && NON_SETTABLE_TYPE_KEYS.has(typeKey)) || (looseKey && NON_SETTABLE_TYPE_KEYS.has(looseKey))) {
    return false;
  }
  return true;
}

const collectStringVariants = (variants: unknown): string[] => {
  if (!Array.isArray(variants)) return [];
  return variants
    .map((value) => (typeof value === 'string' ? value.trim() : ''))
    .filter((value, index, array): value is string => value.length > 0 && array.indexOf(value) === index);
};

const parseInlineEnumVariants = (value: string): string[] => {
  const match = unwrapOptionalTypeString(value).match(ENUM_INLINE_REGEX);
  if (!match) return [];
  const body = match[1]?.trim() ?? '';
  if (!body) return [];
  if (body.startsWith('[')) {
    try {
      const parsed = JSON.parse(body);
      return collectStringVariants(parsed);
    } catch {
      return [];
    }
  }
  return collectStringVariants(body.split('|'));
};

export function getDataTypeVariants(dataType: PipelineDataType | undefined): string[] {
  const variantsFor = (candidate: PipelineDataType | undefined, depth = 0): string[] => {
    if (!candidate || depth > 8) return [];

    if (typeof candidate === 'string') {
      return parseInlineEnumVariants(candidate);
    }

    const collect = (variants: unknown): string[] => collectStringVariants(variants);

    const direct = collect((candidate as { variants?: unknown }).variants);
    if (direct.length > 0) {
      return direct;
    }

    const descriptorVariants = collect((candidate.descriptor as { variants?: unknown } | undefined)?.variants);
    if (descriptorVariants.length > 0) {
      return descriptorVariants;
    }

    if (typeof candidate.kind === 'string') {
      const inlineFromKind = parseInlineEnumVariants(candidate.kind);
      if (inlineFromKind.length > 0) {
        return inlineFromKind;
      }
    }

    const descriptorKind = (candidate.descriptor as { kind?: string } | undefined)?.kind;
    if (typeof descriptorKind === 'string') {
      const inlineFromDescriptorKind = parseInlineEnumVariants(descriptorKind);
      if (inlineFromDescriptorKind.length > 0) {
        return inlineFromDescriptorKind;
      }
    }

    if (isOptionalTypeKey(candidate.kind)) {
      const nested = variantsFor(candidate.element as PipelineDataType | undefined, depth + 1);
      if (nested.length > 0) {
        return nested;
      }
    }

    return [];
  };

  return variantsFor(dataType);
}

const normalizeEnumOption = (entry: unknown): string | null => {
  if (typeof entry === 'string') {
    const trimmed = entry.trim();
    return trimmed ? trimmed : null;
  }
  if (typeof entry === 'number' && Number.isFinite(entry)) {
    return String(entry);
  }
  if (entry && typeof entry === 'object') {
    const record = entry as { value?: unknown; label?: unknown };
    const value = record.value ?? record.label;
    if (typeof value === 'string') {
      const trimmed = value.trim();
      return trimmed ? trimmed : null;
    }
    if (typeof value === 'number' && Number.isFinite(value)) {
      return String(value);
    }
  }
  return null;
};

const normalizeEnumOptions = (raw: unknown): string[] => {
  if (!raw) return [];
  const list = Array.isArray(raw) ? raw : typeof raw === 'string' ? [raw] : [];
  if (list.length === 0) return [];
  const values: string[] = [];
  const seen = new Set<string>();
  for (const entry of list) {
    const normalized = normalizeEnumOption(entry);
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    values.push(normalized);
  }
  return values;
};

export function getMetadataEnumOptions(metadata: unknown): string[] {
  if (!metadata || typeof metadata !== 'object') return [];
  const record = metadata as Record<string, unknown>;
  const raw =
    record.allowedValues ??
    record.allowed_values ??
    record.allowed ??
    record.options ??
    record.variants;
  return normalizeEnumOptions(raw);
}

type FractionStruct = { numerator: number; denominator: number };

type StepwiseIntervalStruct = {
  interval?: FractionStruct;
  min?: FractionStruct;
  max?: FractionStruct;
  step?: FractionStruct;
};

const NUMBER_EPSILON = 1e-6;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null;

const toFraction = (value: unknown): FractionStruct | null => {
  if (!isRecord(value)) return null;
  const { numerator, denominator } = value as Record<string, unknown>;
  if (typeof numerator !== 'number' || typeof denominator !== 'number') return null;
  if (!Number.isFinite(numerator) || !Number.isFinite(denominator)) return null;
  if (denominator === 0) return null;
  return { numerator, denominator };
};

const fractionToFps = (fraction: FractionStruct | null): number | null => {
  if (!fraction) return null;
  const { numerator, denominator } = fraction;
  if (numerator <= 0 || denominator <= 0) return null;
  const fps = numerator / denominator;
  return Number.isFinite(fps) ? fps : null;
};

const formatFpsValue = (fps: number): string => {
  if (Math.abs(fps - Math.round(fps)) <= NUMBER_EPSILON) {
    return `${Math.round(fps)}`;
  }
  if (fps >= 100) return fps.toFixed(0);
  if (fps >= 10) return fps.toFixed(1);
  return fps.toFixed(2);
};

const formatFractionLabel = (fraction: FractionStruct | null): string | null => {
  if (!fraction) return null;
  const fps = fractionToFps(fraction);
  if (fps === null) {
    return `${fraction.numerator}/${fraction.denominator}`;
  }
  return `${formatFpsValue(fps)} fps`;
};

const formatFractionRange = (min?: FractionStruct | null, max?: FractionStruct | null): string | null => {
  const minFps = fractionToFps(min ?? null);
  const maxFps = fractionToFps(max ?? null);
  if (minFps !== null && maxFps !== null) {
    if (Math.abs(minFps - maxFps) <= NUMBER_EPSILON) {
      return `${formatFpsValue(minFps)} fps`;
    }
    return `${formatFpsValue(minFps)}-${formatFpsValue(maxFps)} fps`;
  }
  if (minFps !== null) {
    return `${formatFpsValue(minFps)} fps`;
  }
  if (maxFps !== null) {
    return `${formatFpsValue(maxFps)} fps`;
  }
  const minLabel = min ? `${min.numerator}/${min.denominator}` : null;
  const maxLabel = max ? `${max.numerator}/${max.denominator}` : null;
  if (minLabel && maxLabel) {
    if (minLabel === maxLabel) return minLabel;
    return `${minLabel}-${maxLabel}`;
  }
  return minLabel ?? maxLabel;
};

const formatIntervalValue = (value: unknown): string | null => {
  if (!isRecord(value)) return null;
  const discrete = toFraction((value as Record<string, unknown>).Discrete);
  if (discrete) {
    return formatFractionLabel(discrete);
  }
  const stepwiseRaw = (value as Record<string, unknown>).Stepwise;
  if (isRecord(stepwiseRaw)) {
    const stepwise = stepwiseRaw as StepwiseIntervalStruct;
    const intervalLabel = formatFractionLabel(toFraction(stepwise.interval));
    const rangeLabel = formatFractionRange(toFraction(stepwise.min), toFraction(stepwise.max));
    if (intervalLabel && rangeLabel && intervalLabel !== rangeLabel) {
      return `${intervalLabel} (${rangeLabel})`;
    }
    return intervalLabel ?? rangeLabel ?? null;
  }
  return null;
};

const formatStructuredValue = (value: unknown): string | null => {
  const intervalLabel = formatIntervalValue(value);
  if (intervalLabel) return intervalLabel;
  const fractionLabel = formatFractionLabel(toFraction(value));
  if (fractionLabel) return fractionLabel;
  return null;
};

const resolveEnumInput = (raw: string, variants: string[]): string | null => {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  if (/^-?\d+$/.test(trimmed)) {
    const idx = Number.parseInt(trimmed, 10);
    if (Number.isInteger(idx) && idx >= 0 && idx < variants.length) {
      return variants[idx];
    }
  }
  const normalized = trimmed.toLowerCase();
  for (const variant of variants) {
    const parsed = parseEnumVariant(variant);
    if (variant.toLowerCase() === normalized) return variant;
    if (parsed.label.toLowerCase() === normalized) return variant;
    if (parsed.value.toLowerCase() === normalized) return variant;
  }
  return null;
};

export function formatPipelineValue(value: PipelineNodeValue | null | undefined): string {
  if (!value) return '';
  const raw = value.value;
  if (raw === null || raw === undefined) {
    return '';
  }
  const key =
    typeof value.dataType === 'string'
      ? value.dataType.trim().toLowerCase()
      : (resolveDataTypeKey(value.dataType)?.toLowerCase() ?? '');
  const variants = getDataTypeVariants(value.dataType);
  if (typeof raw === 'number' || typeof raw === 'boolean') {
    if (typeof raw === 'number' && key === 'enum' && variants.length) {
      const idx = Math.trunc(raw);
      if (idx >= 0 && idx < variants.length) {
        return parseEnumVariant(variants[idx]).label;
      }
    }
    return String(raw);
  }
  if (typeof raw === 'string') {
    if (key === 'enum') {
      const resolved = variants.length ? resolveEnumInput(raw, variants) : raw;
      return parseEnumVariant(resolved).label;
    }
    return raw;
  }
  if (typeof raw === 'object' && raw !== null) {
    const structured = formatStructuredValue(raw);
    if (structured) return structured;
  }
  try {
    return JSON.stringify(raw);
  } catch {
    return String(raw);
  }
}

export type NodeValueBuildResult =
  | { success: true; value: PipelineNodeValue; error?: string }
  | { success: false; error: string };

export function buildNodeValueFromInput(
  raw: string,
  dataTypeKey: string,
  variants: string[] = []
): NodeValueBuildResult {
  const key = normalizeTypeKey(dataTypeKey);
  if (key === 'enum' && variants.length > 0) {
    const resolved = resolveEnumInput(raw, variants);
    if (!resolved) {
      return { success: false, error: 'Select a valid option' };
    }
    return {
      success: true,
      value: {
        dataType: dataTypeKey,
        value: resolved
      }
    };
  }
  const parsed = parseValueForType(raw, dataTypeKey);
  if (!parsed.success) {
    return { success: false, error: 'error' in parsed ? parsed.error : 'Invalid value' };
  }
  return {
    success: true,
    value: {
      dataType: dataTypeKey,
      value: parsed.value
    }
  };
}

const clampByte = (value: unknown): number | null => {
  const numeric = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(numeric)) return null;
  const rounded = Math.round(numeric);
  return Math.max(0, Math.min(255, rounded));
};

const parsePixelInput = (raw: string): { r: number; g: number; b: number; a: number } | null => {
  const trimmed = raw.trim();
  const hexMatch = trimmed.match(/^#?([0-9a-f]{6})$/i);
  if (hexMatch?.[1]) {
    const hex = hexMatch[1];
    return {
      r: Number.parseInt(hex.slice(0, 2), 16),
      g: Number.parseInt(hex.slice(2, 4), 16),
      b: Number.parseInt(hex.slice(4, 6), 16),
      a: 255
    };
  }
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (!parsed || typeof parsed !== 'object') return null;
    const record = parsed as Record<string, unknown>;
    const r = clampByte(record.r);
    const g = clampByte(record.g);
    const b = clampByte(record.b);
    if (r == null || g == null || b == null) return null;
    const a = clampByte(record.a) ?? 255;
    return { r, g, b, a };
  } catch {
    return null;
  }
};

function parseValueForType(raw: string, dataTypeKey: string): ParseResult {
  const preserved = raw;
  const trimmed = raw.trim();
  const key = normalizeTypeKey(dataTypeKey);

  switch (key) {
    case 'uint': {
      if (!trimmed) return { success: false, error: EMPTY_ERROR };
      const parsed = Number.parseInt(trimmed, 10);
      if (!Number.isFinite(parsed) || parsed < 0) {
        return { success: false, error: INTEGER_ERROR };
      }
      return { success: true, value: parsed };
    }
    case 'sint': {
      if (!trimmed) return { success: false, error: EMPTY_ERROR };
      const parsed = Number.parseInt(trimmed, 10);
      if (!Number.isFinite(parsed)) {
        return { success: false, error: INTEGER_ERROR };
      }
      return { success: true, value: parsed };
    }
    case 'float':
    case 'double': {
      if (!trimmed) return { success: false, error: EMPTY_ERROR };
      const parsed = Number.parseFloat(trimmed);
      if (!Number.isFinite(parsed)) {
        return { success: false, error: NUMERIC_ERROR };
      }
      return { success: true, value: parsed };
    }
    case 'bool':
    case 'boolean': {
      if (!trimmed) return { success: false, error: EMPTY_ERROR };
      const lower = trimmed.toLowerCase();
      if (lower === 'true') {
        return { success: true, value: true };
      }
      if (lower === 'false') {
        return { success: true, value: false };
      }
      return { success: false, error: BOOLEAN_ERROR };
    }
    case 'str':
    case 'string': {
      return { success: true, value: preserved };
    }
    case 'enum': {
      if (!trimmed) return { success: false, error: EMPTY_ERROR };
      return { success: true, value: trimmed };
    }
    case 'pixel': {
      if (!trimmed) return { success: false, error: EMPTY_ERROR };
      const parsed = parsePixelInput(trimmed);
      if (!parsed) {
        return { success: false, error: 'Enter a #RRGGBB value or a valid JSON object' };
      }
      return { success: true, value: parsed };
    }
    default: {
      if (!trimmed) return { success: false, error: EMPTY_ERROR };
      try {
        return { success: true, value: JSON.parse(trimmed) };
      } catch {
        return { success: false, error: JSON_ERROR };
      }
    }
  }
}

function normalizeTypeKey(key: string): string {
  const normalized = unwrapOptionalTypeString(key).toLowerCase();
  if (isInlineEnumType(normalized)) {
    return 'enum';
  }
  const colonIndex = normalized.lastIndexOf(':');
  if (colonIndex >= 0) {
    return normalized.slice(colonIndex + 1);
  }
  return normalized;
}
