import type { PipelineDataType, PipelineNodeValue, PipelinePortMetadata } from '$lib/types/pipeline';
import { getDataTypeVariants, parseEnumVariant } from '../valueFormatting';
import { normalizePipelinePortName } from '../boundary';
import type { DaedalusValue } from '../daedalusTypes';

export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const normalizeEnumVariants = (variants: string[]): string[] =>
  variants
    .map((value) => value.trim())
    .filter((value, index, array) => value.length > 0 && array.indexOf(value) === index);

export const resolvePortEntry = <T>(
  record: Record<string, T> | null | undefined,
  port: string
): T | undefined => {
  if (!record) return undefined;
  if (Object.prototype.hasOwnProperty.call(record, port)) {
    return record[port];
  }
  const normalized = normalizePipelinePortName(port);
  if (Object.prototype.hasOwnProperty.call(record, normalized)) {
    return record[normalized];
  }
  const fallbackKey = Object.keys(record).find((key) => normalizePipelinePortName(key) === normalized);
  return fallbackKey ? record[fallbackKey] : undefined;
};

const enumVariantsForPort = (
  dataType: PipelineDataType | string | undefined,
  metadata: PipelinePortMetadata | undefined
): string[] => {
  const allowed = normalizeEnumVariants(metadata?.allowedValues ?? []);
  if (allowed.length > 0) return allowed;
  return normalizeEnumVariants(getDataTypeVariants(dataType as PipelineDataType | undefined));
};

const enumIndexForValue = (raw: string, variants: string[]): number | null => {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const inputParts = parseEnumVariant(trimmed);
  for (let idx = 0; idx < variants.length; idx += 1) {
    const variant = variants[idx];
    if (!variant) continue;
    if (variant === trimmed) return idx;
    const candidate = parseEnumVariant(variant);
    if (candidate.value === trimmed || candidate.label === trimmed) return idx;
    if (inputParts.value && (candidate.value === inputParts.value || candidate.label === inputParts.value)) return idx;
    if (inputParts.label && (candidate.value === inputParts.label || candidate.label === inputParts.label)) return idx;
  }
  const numeric = Number.parseInt(trimmed, 10);
  if (Number.isFinite(numeric) && numeric >= 0 && numeric < variants.length) {
    return numeric;
  }
  return null;
};

export const mapEnumIndexToValue = (raw: unknown, metadata: PipelinePortMetadata | undefined): string | null => {
  if (typeof raw !== 'number' || !Number.isFinite(raw)) return null;
  const allowed = normalizeEnumVariants(metadata?.allowedValues ?? []);
  if (allowed.length === 0) return null;
  const idx = Math.trunc(raw);
  if (idx < 0 || idx >= allowed.length) return null;
  return allowed[idx] ?? null;
};

export const decodeDaedalusValue = (raw: unknown): unknown => {
  if (!raw || typeof raw !== 'object') return null;
  const value = raw as Record<string, unknown>;
  const ty = value.type;
  if (typeof ty !== 'string') return null;
  switch (ty) {
    case 'Unit':
      return null;
    case 'Bool':
      return Boolean(value.value);
    case 'Int':
    case 'Float': {
      const numeric = typeof value.value === 'number' ? value.value : Number(value.value);
      return Number.isFinite(numeric) ? numeric : 0;
    }
    case 'String':
      return typeof value.value === 'string' ? value.value : value.value == null ? '' : String(value.value);
    case 'Bytes':
      return value.value ?? null;
    case 'List':
    case 'Tuple':
      return Array.isArray(value.value) ? value.value.map((entry) => decodeDaedalusValue(entry)) : [];
    case 'Map':
      return Array.isArray(value.value)
        ? value.value
            .filter((entry): entry is [unknown, unknown] => Array.isArray(entry) && entry.length === 2)
            .map(([k, v]) => [decodeDaedalusValue(k), decodeDaedalusValue(v)])
        : [];
    case 'Struct': {
      const obj: Record<string, unknown> = {};
      const fields = Array.isArray(value.value) ? value.value : [];
      for (const field of fields) {
        if (!field || typeof field !== 'object') continue;
        const rec = field as Record<string, unknown>;
        const name = typeof rec.name === 'string' ? rec.name.trim() : '';
        if (!name) continue;
        obj[name] = decodeDaedalusValue(rec.value);
      }
      return obj;
    }
    case 'Enum': {
      const enumValue = value.value;
      if (!enumValue || typeof enumValue !== 'object') return null;
      const name = (enumValue as Record<string, unknown>).name;
      return typeof name === 'string' ? name : null;
    }
    default:
      return null;
  }
};

export const dataTypeFromDaedalusValue = (raw: unknown): string => {
  if (!raw || typeof raw !== 'object') return 'Generic';
  const value = raw as Record<string, unknown>;
  const ty = value.type;
  if (ty === 'Bool') return 'bool';
  if (ty === 'Int') return 'int';
  if (ty === 'Float') return 'float';
  if (ty === 'String') return 'string';
  if (ty === 'Bytes') return 'bytes';
  if (ty === 'Enum') return 'enum';
  return 'Generic';
};

export const decodeDaedalusConstValue = (
  raw: DaedalusValue,
  portMeta: PipelinePortMetadata | undefined
): PipelineNodeValue => {
  const decoded = decodeDaedalusValue(raw);
  const mapped = mapEnumIndexToValue(decoded, portMeta);
  return {
    dataType: mapped ? 'enum' : dataTypeFromDaedalusValue(raw),
    value: mapped ?? decoded
  };
};

export const encodeDaedalusValue = (raw: unknown): DaedalusValue => {
  if (raw === null || raw === undefined) return { type: 'Unit' };
  if (typeof raw === 'boolean') return { type: 'Bool', value: raw };
  if (typeof raw === 'number') {
    if (Number.isFinite(raw) && Math.abs(raw - Math.round(raw)) <= Number.EPSILON) {
      return { type: 'Int', value: Math.trunc(raw) };
    }
    return { type: 'Float', value: Number.isFinite(raw) ? raw : 0 };
  }
  if (typeof raw === 'string') return { type: 'String', value: raw };
  if (Array.isArray(raw)) return { type: 'List', value: raw.map((entry) => encodeDaedalusValue(entry)) };
  if (typeof raw === 'object') {
    const record = raw as Record<string, unknown>;
    const fields = Object.entries(record)
      .filter(([key]) => typeof key === 'string' && key.trim().length > 0)
      .map(([name, value]) => ({ name, value: encodeDaedalusValue(value) }));
    return { type: 'Struct', value: fields };
  }
  return { type: 'String', value: String(raw) };
};

export const encodeConstInputs = (
  values: Record<string, PipelineNodeValue> | null | undefined,
  inputTypes: Record<string, PipelineDataType> | undefined,
  inputMetadata: Record<string, PipelinePortMetadata> | undefined
): Array<[string, DaedalusValue]> => {
  if (!values) return [];
  const entries = Object.entries(values)
    .filter(([key]) => typeof key === 'string' && key.trim().length > 0)
    .sort(([a], [b]) => a.localeCompare(b));
  return entries.map(([key, value]) => {
    const portType = resolvePortEntry(inputTypes ?? null, key);
    const portMetadata = resolvePortEntry(inputMetadata ?? null, key);
    const variants = enumVariantsForPort(portType ?? value?.dataType, portMetadata);
    if (typeof value?.value === 'string' && variants.length > 0) {
      const idx = enumIndexForValue(value.value, variants);
      if (idx != null) {
        return [key, { type: 'Int', value: idx }] as [string, DaedalusValue];
      }
    }
    return [key, encodeDaedalusValue(value?.value)] as [string, DaedalusValue];
  });
};

export const encodeConstOverrideValue = (
  port: string,
  value: PipelineNodeValue,
  inputTypes: Record<string, PipelineDataType> | undefined,
  inputMetadata: Record<string, PipelinePortMetadata> | undefined
): DaedalusValue => {
  const portType = resolvePortEntry(inputTypes ?? null, port);
  const portMeta = resolvePortEntry(inputMetadata ?? null, port);
  const variants = enumVariantsForPort(portType ?? value?.dataType, portMeta);
  if (typeof value?.value === 'string' && variants.length > 0) {
    const idx = enumIndexForValue(value.value, variants);
    if (idx != null) {
      return { type: 'Int', value: idx };
    }
  }
  return encodeDaedalusValue(value?.value);
};

const decodeNumber = (value: unknown): number | null => {
  if (!value || typeof value !== 'object') return null;
  const record = value as Record<string, unknown>;
  const type = record.type;
  if (type !== 'Int' && type !== 'Float') return null;
  const numeric = typeof record.value === 'number' ? record.value : Number(record.value);
  return Number.isFinite(numeric) ? numeric : null;
};

export const decodeString = (value: unknown): string | null => {
  if (!value || typeof value !== 'object') return null;
  const record = value as Record<string, unknown>;
  if (record.type !== 'String') return null;
  return typeof record.value === 'string' && record.value.trim() ? record.value.trim() : null;
};

const decodeStringArray = (value: unknown): string[] | null => {
  const decoded = decodeDaedalusValue(value);
  const list = Array.isArray(decoded) ? decoded : null;
  if (!list) return null;
  const values = list
    .map((entry) => (typeof entry === 'string' ? entry : typeof entry === 'number' ? String(entry) : null))
    .filter((entry): entry is string => Boolean(entry && entry.trim().length > 0))
    .map((entry) => entry.trim());
  return values.length > 0 ? values : null;
};

export const extractPortMetadata = (
  metadata: Record<string, DaedalusValue> | null | undefined
): { inputs: Record<string, PipelinePortMetadata>; outputs: Record<string, PipelinePortMetadata> } => {
  const out = { inputs: {}, outputs: {} } as {
    inputs: Record<string, PipelinePortMetadata>;
    outputs: Record<string, PipelinePortMetadata>;
  };
  if (!metadata) return out;
  for (const [key, value] of Object.entries(metadata)) {
    const match = key.match(
      /^(inputs|outputs)\.([^.]*)\.(description|min|max|step|allowed_values|allowedValues|allowed|ui_min|ui_max|ui_step|ui_control|ui)$/
    );
    if (!match?.[1] || !match[2] || !match[3]) continue;
    const direction = match[1] as 'inputs' | 'outputs';
    const port = match[2];
    const field = match[3];
    const target = direction === 'inputs' ? out.inputs : out.outputs;
    const entry = (target[port] ??= {});

    if (field === 'description') {
      const desc = decodeString(value);
      if (desc) entry.description = desc;
      continue;
    }
    if (field === 'min') {
      const n = decodeNumber(value);
      if (n != null) entry.min = n;
      continue;
    }
    if (field === 'max') {
      const n = decodeNumber(value);
      if (n != null) entry.max = n;
      continue;
    }
    if (field === 'step') {
      const n = decodeNumber(value);
      if (n != null) entry.step = n;
      continue;
    }
    if (field === 'ui_min') {
      const n = decodeNumber(value);
      if (n != null) entry.uiMin = n;
      continue;
    }
    if (field === 'ui_max') {
      const n = decodeNumber(value);
      if (n != null) entry.uiMax = n;
      continue;
    }
    if (field === 'ui_step') {
      const n = decodeNumber(value);
      if (n != null) entry.uiStep = n;
      continue;
    }
    if (field === 'ui_control' || field === 'ui') {
      const desc = decodeString(value);
      if (desc) entry.uiControl = desc;
      continue;
    }
    const allowed = decodeStringArray(value);
    if (allowed) entry.allowedValues = allowed;
  }
  return out;
};

export const encodeFloatValue = (value: number): DaedalusValue => ({ type: 'Float', value });
