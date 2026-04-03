import type { DaedalusRegistryFanInPort } from '$lib/api/client';
import type {
  PipelineDataType,
  PipelineFanInPort,
  PipelineNodeMetadata,
  PipelinePortMetadata
} from '$lib/types/pipeline';
import type {
  DaedalusFanInPort,
  DaedalusRegistryPort,
  GpuPreference,
  TypeDescription,
  TypeRegistryLookup
} from './types';

export const normalizedString = (value: unknown): string | null => {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
};

const hashString = (value: string): number => {
  let hash = 2166136261;
  for (let idx = 0; idx < value.length; idx++) {
    hash ^= value.charCodeAt(idx);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
};

const colorFromKey = (key: string): string => {
  const hash = hashString(key);
  const hue = hash % 360;
  return `hsl(${hue} 62% 58%)`;
};

const looksLikeImageStruct = (value: unknown): boolean => {
  if (!value || typeof value !== 'object') return false;
  const record = value as Record<string, unknown>;
  const fields =
    (Array.isArray(record.Struct) ? record.Struct : null) ??
    (record.type === 'Struct' && Array.isArray(record.value) ? record.value : null);
  if (!Array.isArray(fields)) return false;
  const names = new Set(
    fields
      .map((field) => (field && typeof field === 'object' ? normalizedString((field as Record<string, unknown>).name) : null))
      .filter((name): name is string => Boolean(name))
      .map((name) => name.toLowerCase())
  );
  return names.has('data_b64') && names.has('width') && names.has('height');
};

const simplifyRustType = (raw: string): string => {
  const trimmed = raw.trim();
  if (!trimmed) return raw;
  const outer = trimmed.split('<', 1)[0] ?? trimmed;
  const simple = outer.split('::').filter(Boolean).pop() ?? outer;
  return simple || trimmed;
};

const normalizeTypeExprRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object') return null;
  const record = value as Record<string, unknown>;
  if (typeof record.type === 'string') {
    const kind = record.type.trim();
    if (kind === 'Scalar' && typeof record.value === 'string') return { Scalar: record.value };
    if (kind === 'Enum' && Array.isArray(record.value)) return { Enum: record.value };
    if (kind === 'Struct' && Array.isArray(record.value)) return { Struct: record.value };
    if (kind === 'Opaque' && typeof record.value === 'string') return { Opaque: record.value };
    if (kind === 'Optional' && record.value != null) return { Optional: record.value };
    if (kind === 'List' && record.value != null) return { List: record.value };
    if (kind === 'Map' && Array.isArray(record.value)) return { Map: record.value };
    if (kind === 'Tuple' && Array.isArray(record.value)) return { Tuple: record.value };
  }
  return record;
};

const describeTypeExpr = (
  value: unknown,
  typeRegistry?: TypeRegistryLookup,
  visited: Set<string> = new Set()
): TypeDescription | null => {
  const record = normalizeTypeExprRecord(value);
  if (!record) return null;

  const opaque = normalizedString(record.Opaque);
  if (opaque) {
    if (opaque === 'image' || opaque.startsWith('image:')) {
      const flavor = opaque.startsWith('image:') ? opaque.slice('image:'.length) : null;
      return {
        key: 'image',
        label: flavor ? `Image (${flavor})` : 'Image',
        kind: 'image'
      };
    }
    if (opaque === 'cv:binary_image') {
      return { key: 'image', label: 'Image (binary)', kind: 'image' };
    }
    if (opaque.startsWith('rust:')) {
      const raw = opaque.slice('rust:'.length);
      if (typeRegistry && !visited.has(raw)) {
        visited.add(raw);
        const resolved = typeRegistry.get(raw);
        const desc = resolved ? describeTypeExpr(resolved, typeRegistry, visited) : null;
        if (desc) return desc;
      }
      const match = raw.match(/(?:^|::)Payload<(.+)>$/);
      if (match?.[1]) {
        const innerRaw = match[1];
        const resolvedInner = typeRegistry && !visited.has(innerRaw) ? typeRegistry.get(innerRaw) : null;
        if (typeRegistry && resolvedInner) visited.add(innerRaw);
        const inner = resolvedInner && typeRegistry ? describeTypeExpr(resolvedInner, typeRegistry, visited) : null;
        if (inner?.kind === 'image') {
          return inner;
        }
        const fallbackInner: TypeDescription = inner ?? {
          key: `rust:${innerRaw}`,
          label: simplifyRustType(innerRaw),
          kind: 'opaque'
        };
        return {
          key: `payload<${fallbackInner.key}>`,
          label: `payload<${fallbackInner.label}>`,
          kind: 'payload',
          element: fallbackInner
        };
      }
      return { key: opaque, label: simplifyRustType(raw), kind: 'opaque' };
    }
    return { key: opaque, label: opaque, kind: 'opaque' };
  }

  const scalar = record.Scalar;
  if (typeof scalar === 'string') {
    const lower = scalar.trim().toLowerCase();
    return { key: lower || scalar.trim(), label: lower || scalar.trim(), kind: 'scalar' };
  }

  if ('Optional' in record) {
    const inner = describeTypeExpr((record as { Optional?: unknown }).Optional, typeRegistry, visited);
    if (!inner) return { key: 'optional', label: 'optional', kind: 'optional' };
    return { key: `optional<${inner.key}>`, label: `${inner.label}?`, kind: 'optional', element: inner };
  }

  if ('List' in record) {
    const inner = describeTypeExpr((record as { List?: unknown }).List, typeRegistry, visited);
    if (!inner) return { key: 'list', label: 'list', kind: 'list' };
    return { key: `list<${inner.key}>`, label: `${inner.label}[]`, kind: 'list', element: inner };
  }

  if ('Map' in record) {
    const pair = (record as { Map?: unknown }).Map;
    if (Array.isArray(pair) && pair.length === 2) {
      const key = describeTypeExpr(pair[0], typeRegistry, visited);
      const value = describeTypeExpr(pair[1], typeRegistry, visited);
      if (key && value) {
        return {
          key: `map<${key.key},${value.key}>`,
          label: `map<${key.label}, ${value.label}>`,
          kind: 'map'
        };
      }
    }
    return { key: 'map', label: 'map', kind: 'map' };
  }

  if ('Tuple' in record) {
    const items = (record as { Tuple?: unknown }).Tuple;
    if (Array.isArray(items) && items.length) {
      const parts = items
        .map((item) => describeTypeExpr(item, typeRegistry, visited))
        .filter(Boolean) as TypeDescription[];
      return {
        key: `tuple<${parts.map((p) => p.key).join(',')}>`,
        label: `(${parts.map((p) => p.label).join(', ')})`,
        kind: 'tuple'
      };
    }
    return { key: 'tuple', label: 'tuple', kind: 'tuple' };
  }

  if ('Struct' in record) {
    if (looksLikeImageStruct(record)) return { key: 'image', label: 'Image', kind: 'image' };
    const fieldsRaw = (record as { Struct?: unknown }).Struct;
    const fields = Array.isArray(fieldsRaw) ? (fieldsRaw as Array<Record<string, unknown>>) : [];
    const descriptors = fields
      .map((field) => {
        const name = normalizedString(field?.name);
        const inner = describeTypeExpr(field?.ty, typeRegistry, visited);
        return name && inner ? { name, inner } : null;
      })
      .filter(Boolean) as Array<{ name: string; inner: TypeDescription }>;
    const keyParts = descriptors
      .slice()
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((f) => `${f.name}:${f.inner.key}`);
    const key = keyParts.length ? `struct{${keyParts.join(',')}}` : 'struct';

    const lowerNames = new Set(descriptors.map((d) => d.name.toLowerCase()));
    if (lowerNames.size === 2 && lowerNames.has('x') && lowerNames.has('y')) {
      return { key: 'point', label: 'Point', kind: 'struct' };
    }
    if (lowerNames.size === 4 && lowerNames.has('r') && lowerNames.has('g') && lowerNames.has('b') && lowerNames.has('a')) {
      return { key: 'pixel', label: 'Pixel', kind: 'struct' };
    }
    if (descriptors.length > 0 && descriptors.length <= 4) {
      return { key, label: `Struct(${descriptors.map((d) => d.name).join(', ')})`, kind: 'struct' };
    }
    return { key, label: descriptors.length ? `Struct(${descriptors.length} fields)` : 'Struct', kind: 'struct' };
  }

  if ('Enum' in record) {
    const variants = enumVariantsFromTypeExpr(record);
    return {
      key: variants.length ? `enum<${variants.join('|')}>` : 'enum',
      label: 'Enum',
      kind: 'enum',
      variants
    };
  }

  return null;
};

const enumVariantsFromTypeExpr = (value: unknown): string[] => {
  if (!value || typeof value !== 'object') return [];
  const record = value as Record<string, unknown>;
  const variants =
    (Array.isArray(record.Enum) ? record.Enum : null) ??
    (record.type === 'Enum' && Array.isArray(record.value) ? record.value : null);
  if (!Array.isArray(variants)) return [];
  return variants
    .map((variant) => {
      if (!variant || typeof variant !== 'object') return null;
      const name = normalizedString((variant as Record<string, unknown>).name);
      return name;
    })
    .filter((name): name is string => Boolean(name));
};

export const extractPortDescriptions = (
  metadata: Record<string, unknown>
): { inputs: Record<string, string>; outputs: Record<string, string> } => {
  const out = { inputs: {}, outputs: {} } as { inputs: Record<string, string>; outputs: Record<string, string> };
  for (const [key, value] of Object.entries(metadata ?? {})) {
    const text = normalizedString(value);
    if (!text) continue;
    const inputMatch = key.match(/^inputs\.([^.]*)\.description$/);
    if (inputMatch?.[1]) out.inputs[inputMatch[1]] = text;
    const outputMatch = key.match(/^outputs\.([^.]*)\.description$/);
    if (outputMatch?.[1]) out.outputs[outputMatch[1]] = text;
  }
  return out;
};

const decodeDaedalusValue = (raw: unknown): unknown => {
  if (raw == null) return null;
  if (typeof raw === 'string' || typeof raw === 'number' || typeof raw === 'boolean') return raw;
  if (Array.isArray(raw)) return raw.map((entry) => decodeDaedalusValue(entry));
  if (typeof raw !== 'object') return null;
  const record = raw as Record<string, unknown>;
  const ty = normalizedString(record.type);
  if (!ty) return raw;
  switch (ty) {
    case 'Unit':
      return null;
    case 'Bool':
      return Boolean(record.value);
    case 'Int':
    case 'Float': {
      const numeric = typeof record.value === 'number' ? record.value : Number(record.value);
      return Number.isFinite(numeric) ? numeric : null;
    }
    case 'String':
      return typeof record.value === 'string' ? record.value : record.value == null ? '' : String(record.value);
    case 'Enum': {
      const value = record.value;
      if (!value || typeof value !== 'object') return null;
      return normalizedString((value as Record<string, unknown>).name);
    }
    case 'List':
    case 'Tuple':
      return Array.isArray(record.value) ? (record.value as unknown[]).map((entry) => decodeDaedalusValue(entry)) : [];
    case 'Struct': {
      const fields = Array.isArray(record.value) ? (record.value as Array<Record<string, unknown>>) : [];
      const out: Record<string, unknown> = {};
      for (const field of fields) {
        const name = normalizedString(field?.name);
        if (!name) continue;
        out[name] = decodeDaedalusValue(field.value);
      }
      return out;
    }
    default:
      return raw;
  }
};

const decodeNumberLike = (raw: unknown): number | null => {
  const decoded = decodeDaedalusValue(raw);
  if (typeof decoded === 'number' && Number.isFinite(decoded)) return decoded;
  if (typeof raw === 'number' && Number.isFinite(raw)) return raw;
  if (typeof decoded === 'string') {
    const parsed = Number(decoded);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
};

const decodeStringLike = (raw: unknown): string | null => {
  const decoded = decodeDaedalusValue(raw);
  if (typeof decoded === 'string') return decoded.trim() ? decoded.trim() : null;
  if (typeof raw === 'string') return raw.trim() ? raw.trim() : null;
  return null;
};

const decodeStringArrayLike = (raw: unknown): string[] | null => {
  const decoded = decodeDaedalusValue(raw);
  const list = Array.isArray(decoded) ? decoded : Array.isArray(raw) ? raw : null;
  if (!list) return null;
  const values = list
    .map((entry) => (typeof entry === 'string' ? entry : typeof entry === 'number' ? String(entry) : decodeStringLike(entry)))
    .filter((entry): entry is string => Boolean(entry && entry.trim().length > 0))
    .map((entry) => entry.trim());
  return values.length > 0 ? values : null;
};

export const extractPortMetadata = (
  metadata: Record<string, unknown>
): { inputs: Record<string, PipelinePortMetadata>; outputs: Record<string, PipelinePortMetadata> } => {
  const out = { inputs: {}, outputs: {} } as {
    inputs: Record<string, PipelinePortMetadata>;
    outputs: Record<string, PipelinePortMetadata>;
  };

  for (const [key, value] of Object.entries(metadata ?? {})) {
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
      const desc = decodeStringLike(value);
      if (desc) entry.description = desc;
      continue;
    }
    if (field === 'min') {
      const n = decodeNumberLike(value);
      if (n != null) entry.min = n;
      continue;
    }
    if (field === 'max') {
      const n = decodeNumberLike(value);
      if (n != null) entry.max = n;
      continue;
    }
    if (field === 'step') {
      const n = decodeNumberLike(value);
      if (n != null) entry.step = n;
      continue;
    }
    if (field === 'ui_min') {
      const n = decodeNumberLike(value);
      if (n != null) entry.uiMin = n;
      continue;
    }
    if (field === 'ui_max') {
      const n = decodeNumberLike(value);
      if (n != null) entry.uiMax = n;
      continue;
    }
    if (field === 'ui_step') {
      const n = decodeNumberLike(value);
      if (n != null) entry.uiStep = n;
      continue;
    }
    if (field === 'ui_control' || field === 'ui') {
      const desc = decodeStringLike(value);
      if (desc) entry.uiControl = desc;
      continue;
    }

    const allowed = decodeStringArrayLike(value);
    if (allowed) entry.allowedValues = allowed;
  }

  return out;
};

export const normalizeFanInPorts = (
  ports: DaedalusRegistryFanInPort[] | DaedalusFanInPort[] | null | undefined,
  typeRegistry?: TypeRegistryLookup
): PipelineFanInPort[] => {
  if (!Array.isArray(ports)) return [];
  return ports
    .map((port) => {
      if (!port || typeof port !== 'object') return null;
      const prefix = normalizedString((port as DaedalusFanInPort).prefix);
      if (!prefix) return null;
      const typeDesc = describeTypeExpr((port as DaedalusFanInPort).ty, typeRegistry);
      const dataType = typeDesc ? ({ kind: typeDesc.key, label: typeDesc.label } as PipelineDataType) : 'Generic';
      const start = typeof (port as DaedalusFanInPort).start === 'number' ? (port as DaedalusFanInPort).start : undefined;
      return { prefix, dataType, ...(start != null ? { start } : {}) };
    })
    .filter((port): port is PipelineFanInPort => Boolean(port));
};

export const applyPortDefaults = (target: Record<string, PipelinePortMetadata>, ports: Array<string | DaedalusRegistryPort>) => {
  for (const entry of ports ?? []) {
    if (!entry || typeof entry !== 'object' || typeof entry === 'string') continue;
    const name = normalizedString((entry as DaedalusRegistryPort).name);
    if (!name) continue;
    const rawDefault = (entry as DaedalusRegistryPort).const_value;
    if (rawDefault === undefined) continue;
    const decoded = decodeDaedalusValue(rawDefault);
    (target[name] ??= {}).defaultValue = decoded;
  }
};

const normalizeGpuPreference = (value: unknown): GpuPreference | undefined => {
  if (typeof value !== 'string') return undefined;
  const normalized = value.trim().toLowerCase();
  if (normalized === 'supported') return 'supported';
  if (normalized === 'preferred') return 'preferred';
  if (normalized === 'unsupported') return 'unsupported';
  return undefined;
};

const parseGpuPorts = (value: unknown): Array<{ port?: string; kind?: string; format?: string }> | undefined => {
  if (!Array.isArray(value)) return undefined;
  const entries = value
    .map((entry) => {
      if (!entry || typeof entry !== 'object') return null;
      const record = entry as Record<string, unknown>;
      const port = normalizedString(record.port);
      const kind = normalizedString(record.kind);
      const format = normalizedString(record.format);
      if (!port && !kind && !format) return null;
      return {
        ...(port ? { port } : {}),
        ...(kind ? { kind } : {}),
        ...(format ? { format } : {})
      };
    })
    .filter(Boolean) as Array<{ port?: string; kind?: string; format?: string }>;
  return entries.length > 0 ? entries : undefined;
};

export const extractGpuMetadata = (metadata: Record<string, unknown>): PipelineNodeMetadata['gpu'] | undefined => {
  const raw = metadata?.gpu;
  if (!raw || typeof raw !== 'object') return undefined;
  const record = raw as Record<string, unknown>;
  const preference = normalizeGpuPreference(record.preference);
  const inputs = parseGpuPorts(record.inputs);
  const outputs = parseGpuPorts(record.outputs);
  const preservesCpuLayout =
    typeof record.preserves_cpu_layout === 'boolean' ? record.preserves_cpu_layout : undefined;
  const sideEffects = Array.isArray(record.side_effects)
    ? record.side_effects
        .map((entry) => normalizedString(entry))
        .filter((entry): entry is string => Boolean(entry))
    : undefined;
  if (!preference && !inputs && !outputs && preservesCpuLayout === undefined && !sideEffects) {
    return undefined;
  }
  return {
    ...(preference ? { preference } : {}),
    ...(inputs ? { inputs } : {}),
    ...(outputs ? { outputs } : {}),
    ...(preservesCpuLayout !== undefined ? { preserves_cpu_layout: preservesCpuLayout } : {}),
    ...(sideEffects ? { side_effects: sideEffects } : {})
  };
};

const typeForPort = (
  port: DaedalusRegistryPort,
  portDescription: string | null | undefined,
  typeRegistry?: TypeRegistryLookup
): PipelineDataType => {
  const desc = describeTypeExpr(port.ty, typeRegistry);
  const kind = desc?.key ?? 'Generic';
  const source = normalizedString(port.source);
  if (desc?.kind === 'image') {
    return {
      kind: 'image',
      label: source ?? desc.label,
      color: colorFromKey('image'),
      summary: portDescription ?? undefined
    };
  }
  if (desc?.kind === 'enum') {
    const variants = desc.variants ?? enumVariantsFromTypeExpr(port.ty);
    return {
      kind: 'enum',
      label: source ?? 'Enum',
      variants,
      settable: variants.length > 0,
      color: colorFromKey('enum'),
      summary: portDescription ?? undefined
    };
  }
  if (source && source !== kind) {
    return { kind, label: source, color: colorFromKey(kind), summary: portDescription ?? undefined };
  }
  if (desc && desc.kind !== 'scalar') {
    return { kind, label: desc.label, color: colorFromKey(kind), summary: portDescription ?? undefined };
  }
  if (portDescription) {
    return { kind, label: desc?.label ?? kind, color: colorFromKey(kind), summary: portDescription };
  }
  return desc?.label ?? kind;
};

export const toPortMap = (
  ports: Array<string | DaedalusRegistryPort>,
  descriptions: Record<string, string>,
  typeRegistry?: TypeRegistryLookup
): Record<string, PipelineDataType> =>
  Object.fromEntries(
    (ports ?? [])
      .map((entry) => {
        if (typeof entry === 'string') {
          const name = normalizedString(entry);
          return name ? [name, 'Generic'] : null;
        }
        const name = normalizedString(entry?.name);
        if (!name) return null;
        return [name, typeForPort(entry, descriptions[name] ?? null, typeRegistry)];
      })
      .filter(Boolean)
  ) as Record<string, PipelineDataType>;
