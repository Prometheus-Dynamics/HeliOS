import type { StreamInfo } from '$lib/api/client';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineNodeValue,
  PipelinePortMetadata,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
import { fromApiGraphPlan } from '$lib/features/pipelines/graphConverters';
import { decodeDaedalusValue } from '$lib/features/pipelines/daedalusGraph/valueCodec';
import { getDataTypeVariants } from '$lib/features/pipelines/valueFormatting';
import { serializeGraphPlan } from '$lib/features/pipelines/graph';
import { normalizeDaedalusRegistry } from '$lib/features/pipelines/controller/daedalusRegistry/normalization';
import { createPipelineTuningApply } from './tuningApply';
import { createPipelineTuningDrafts } from './tuningDrafts';
import { clamp, createPipelineTuningPointerHandlers } from './tuningPointers';
import { nodeValueSignature, pipelineOverrideSignature } from './tuningSignatures';
import { PIPELINE_UI_METADATA_KEY } from './cameraPageStateTypes';
import { parseMetadataValue } from './cameraStateUtils';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  PIPELINE_UI_STORAGE_PREFIX,
  RAW_LOOPBACK_GRAPH,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds,
  normalizePipelineOutputMap,
  setRawPipelineUuid
} from './cameraPipelineShared';

export { clamp, nodeValueSignature, pipelineOverrideSignature };
export {
  PIPELINE_OUTPUT_CELL_KEY,
  PIPELINE_UI_STORAGE_PREFIX,
  RAW_LOOPBACK_GRAPH,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds,
  normalizePipelineOutputMap,
  setRawPipelineUuid
};

export type PipelineNodeValueDescriptor = {
  nodeId: string;
  backendId?: string | null;
  sourceId?: string | null;
  nodeLabel: string;
  portKey: string;
  dataType: PipelineDataType | null;
  defaultValue: PipelineNodeValue | null;
  overrideValue: PipelineNodeValue | null;
  metadata: PipelinePortMetadata | null;
};

type PipelineTuningDragState = {
  startX: number;
  startY: number;
  originX: number;
  originY: number;
};

type PipelineTuningResizeState = {
  startX: number;
  startY: number;
  originW: number;
  originH: number;
};

type PipelineTuningPosition = {
  x: number;
  y: number;
};

type PipelineTuningSize = {
  width: number;
  height: number;
};

type StreamUpdatesSocket = {
  send?: (payload: Record<string, unknown>) => boolean | void;
} | null;

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

const asTrimmedString = (value: unknown): string =>
  typeof value === 'string' ? value.trim() : '';

export function normalizeNodePortKey(value: string): string {
  return value.trim().toLowerCase();
}

export function extractNodeOverrides(graph: unknown): Record<string, Record<string, PipelineNodeValue>> {
  if (!graph || typeof graph !== 'object') return {};
  const plan = graph as {
    nodeValueOverrides?: Record<string, Record<string, PipelineNodeValue>>;
    node_value_overrides?: Record<string, Record<string, PipelineNodeValue>>;
    overrides?: Record<string, Record<string, PipelineNodeValue>>;
  };
  const raw = plan.nodeValueOverrides ?? plan.node_value_overrides ?? plan.overrides ?? null;
  if (!raw || typeof raw !== 'object') return {};
  return raw as Record<string, Record<string, PipelineNodeValue>>;
}

export function mergeNodeOverrides(
  base: Record<string, Record<string, PipelineNodeValue>>,
  extra: Record<string, Record<string, PipelineNodeValue>>
): Record<string, Record<string, PipelineNodeValue>> {
  const merged: Record<string, Record<string, PipelineNodeValue>> = { ...(base ?? {}) };
  for (const [nodeId, overrides] of Object.entries(extra ?? {})) {
    merged[nodeId] = { ...(merged[nodeId] ?? {}), ...(overrides ?? {}) };
  }
  return merged;
}

export function normalizePortMetadataMap(
  input: Record<string, PipelinePortMetadata> | undefined
): Record<string, PipelinePortMetadata> | null {
  if (!input || typeof input !== 'object') return null;
  const entries = Object.entries(input).map(([key, value]) => [normalizeNodePortKey(key), value] as const);
  return Object.fromEntries(entries);
}

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
  if (typeof decoded === 'string') {
    const trimmed = decoded.trim();
    return trimmed.length ? trimmed : null;
  }
  if (typeof raw === 'string') {
    const trimmed = raw.trim();
    return trimmed.length ? trimmed : null;
  }
  return null;
};

const decodeStringArrayLike = (raw: unknown): string[] | null => {
  const decoded = decodeDaedalusValue(raw);
  const list = Array.isArray(decoded) ? decoded : Array.isArray(raw) ? raw : null;
  if (!list) return null;
  const values = list
    .map((entry) => {
      if (typeof entry === 'string') return entry;
      if (typeof entry === 'number') return String(entry);
      return decodeStringLike(entry);
    })
    .filter((entry): entry is string => Boolean(entry && entry.trim().length > 0))
    .map((entry) => entry.trim());
  return values.length > 0 ? values : null;
};

const normalizePortMetadata = (meta: PipelinePortMetadata | null | undefined): PipelinePortMetadata | null => {
  if (!meta || typeof meta !== 'object') return null;
  const record = meta as Record<string, unknown>;
  const description = decodeStringLike(record.description ?? meta.description);
  const allowedValues =
    decodeStringArrayLike(record.allowedValues ?? record.allowed_values ?? record.allowed ?? record.options ?? record.variants) ??
    (Array.isArray(meta.allowedValues) ? meta.allowedValues : null);
  const min = decodeNumberLike(record.min ?? meta.min);
  const max = decodeNumberLike(record.max ?? meta.max);
  const step = decodeNumberLike(record.step ?? meta.step);
  const uiMin = decodeNumberLike(record.uiMin ?? record.ui_min ?? meta.uiMin);
  const uiMax = decodeNumberLike(record.uiMax ?? record.ui_max ?? meta.uiMax);
  const uiStep = decodeNumberLike(record.uiStep ?? record.ui_step ?? meta.uiStep);
  const uiControl = decodeStringLike(record.uiControl ?? record.ui_control ?? record.ui ?? meta.uiControl);
  const normalized: PipelinePortMetadata = {};
  if (description) normalized.description = description;
  if (allowedValues && allowedValues.length > 0) normalized.allowedValues = allowedValues;
  if (min != null) normalized.min = min;
  if (max != null) normalized.max = max;
  if (step != null) normalized.step = step;
  if (uiMin != null) normalized.uiMin = uiMin;
  if (uiMax != null) normalized.uiMax = uiMax;
  if (uiStep != null) normalized.uiStep = uiStep;
  if (uiControl) normalized.uiControl = uiControl;
  if ('defaultValue' in meta && meta.defaultValue !== undefined) {
    normalized.defaultValue = (meta as PipelinePortMetadata).defaultValue;
  }
  return Object.keys(normalized).length > 0 ? normalized : null;
};

export function findPortMetadata(
  node: { metadata?: { inputPorts?: Record<string, PipelinePortMetadata>; input_ports?: Record<string, PipelinePortMetadata> } },
  portKey: string
): PipelinePortMetadata | null {
  const normalized = normalizeNodePortKey(portKey);
  const metadata = node?.metadata ?? {};
  const direct = metadata.inputPorts ?? metadata.input_ports;
  if (!direct) return null;
  const normalizedMap = normalizePortMetadataMap(direct);
  if (!normalizedMap) return null;
  return normalizePortMetadata(normalizedMap[normalized] ?? null);
}

export function numberFromRegistryValue(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  if (value && typeof value === 'object') {
    const raw = (value as { value?: unknown }).value;
    if (typeof raw === 'number' && Number.isFinite(raw)) return raw;
  }
  return null;
}

export function resolveRegistrySnapshotNodeId(snapshot: unknown, backendId: string | null | undefined): string | null {
  const snapshotRecord = asRecord(snapshot);
  if (!snapshotRecord || !backendId) return null;
  const nodes = Array.isArray(snapshotRecord.nodes) ? snapshotRecord.nodes : [];
  if (!nodes.length) return null;
  const normalize = (value: string) => {
    const lower = value.toLowerCase();
    const atIndex = lower.indexOf('@');
    return atIndex >= 0 ? lower.slice(0, atIndex) : lower;
  };
  const normalized = normalize(String(backendId));
  let best: { id: string; score: number } | null = null;
  for (const node of nodes) {
    const rawId = typeof node?.id === 'string' ? node.id : String(node?.id ?? '');
    if (!rawId) continue;
    const idLower = rawId.toLowerCase();
    if (idLower === normalized) return rawId;
    const normalizedId = normalize(rawId);
    if (normalizedId === normalized) return rawId;
    let score = 0;
    if (normalized && idLower.includes(normalized)) {
      score = normalized.length / Math.max(idLower.length, 1);
    } else if (normalized && normalized.includes(idLower)) {
      score = idLower.length;
    } else {
      continue;
    }
    if (!best || score > best.score) {
      best = { id: rawId, score };
    }
  }
  return best?.id ?? null;
}

type RegistryLookup = {
  entries: PipelineRegistryEntry[];
  byId: Map<string, PipelineRegistryEntry>;
};

const registryLookupCache = new WeakMap<object, RegistryLookup>();

const getRegistryLookup = (snapshot: unknown): RegistryLookup | null => {
  const snapshotRecord = asRecord(snapshot);
  if (!snapshotRecord) return null;
  if (registryLookupCache.has(snapshotRecord)) {
    return registryLookupCache.get(snapshotRecord) ?? null;
  }
  const nodes = Array.isArray(snapshotRecord.nodes) ? snapshotRecord.nodes : null;
  if (!nodes) return null;
  const entries = normalizeDaedalusRegistry(nodes, Array.isArray(snapshotRecord.types) ? snapshotRecord.types : undefined);
  const byId = new Map(entries.map((entry) => [entry.id, entry]));
  const lookup = { entries, byId };
  registryLookupCache.set(snapshotRecord, lookup);
  return lookup;
};

const findRegistryEntryFor = (snapshot: unknown, nodeId: string): PipelineRegistryEntry | null => {
  const lookup = getRegistryLookup(snapshot);
  if (!lookup) return null;
  const resolvedNodeId = resolveRegistrySnapshotNodeId(snapshot, nodeId);
  if (!resolvedNodeId) return null;
  return lookup.byId.get(resolvedNodeId) ?? null;
};

const resolveRegistryPort = <T>(
  record: Record<string, T> | null | undefined,
  portKey: string
): T | null => {
  if (!record) return null;
  if (Object.prototype.hasOwnProperty.call(record, portKey)) {
    return record[portKey] ?? null;
  }
  const normalized = normalizeNodePortKey(portKey);
  if (!normalized) return null;
  if (Object.prototype.hasOwnProperty.call(record, normalized)) {
    return record[normalized] ?? null;
  }
  const fallbackKey = Object.keys(record).find((key) => normalizeNodePortKey(key) === normalized);
  return fallbackKey ? record[fallbackKey] ?? null : null;
};

export function registryPortMetadataFor(snapshot: unknown, nodeId: string, portKey: string): PipelinePortMetadata | null {
  const entry = findRegistryEntryFor(snapshot, nodeId);
  if (entry?.metadata?.inputPorts) {
    const resolved = resolveRegistryPort(entry.metadata.inputPorts, portKey);
    return normalizePortMetadata(resolved ?? null);
  }
  const snapshotRecord = asRecord(snapshot);
  const registryNodes = Array.isArray(snapshotRecord?.nodes) ? snapshotRecord.nodes : [];
  const resolvedNodeId = resolveRegistrySnapshotNodeId(snapshot, nodeId);
  const registryNode = resolvedNodeId
    ? registryNodes.find((node) => String(asRecord(node)?.id ?? '') === resolvedNodeId) ?? null
    : null;
  const metadata = asRecord(asRecord(registryNode)?.metadata);
  if (!metadata) return null;
  const normalizedPort = normalizeNodePortKey(portKey);
  const directPorts =
    (asRecord(metadata.inputPorts) as Record<string, PipelinePortMetadata> | null) ??
    (asRecord(metadata.input_ports) as Record<string, PipelinePortMetadata> | null);
  if (directPorts) {
    const normalizedMap = normalizePortMetadataMap(directPorts);
    if (normalizedMap && normalizedMap[normalizedPort]) {
      return normalizePortMetadata(normalizedMap[normalizedPort] ?? null);
    }
  }
  const portMeta: PipelinePortMetadata = {};
  for (const [key, value] of Object.entries(metadata)) {
    const match = key.match(
      /^inputs\.([^.]*)\.(description|allowed_values|allowedValues|allowed|options|variants|min|max|step|ui_min|ui_max|ui_step|ui_control|ui)$/
    );
    if (!match?.[1] || !match[2]) continue;
    if (normalizeNodePortKey(match[1]) !== normalizedPort) continue;
    switch (match[2]) {
      case 'description': {
        const desc = decodeStringLike(value);
        if (desc) portMeta.description = desc;
        break;
      }
      case 'allowed_values':
      case 'allowedValues': {
        const allowed = decodeStringArrayLike(value);
        if (allowed) portMeta.allowedValues = allowed;
        break;
      }
      case 'allowed':
      case 'options':
      case 'variants': {
        const allowed = decodeStringArrayLike(value);
        if (allowed) portMeta.allowedValues = allowed;
        break;
      }
      case 'min': {
        const parsed = decodeNumberLike(value);
        if (parsed != null) portMeta.min = parsed;
        break;
      }
      case 'max': {
        const parsed = decodeNumberLike(value);
        if (parsed != null) portMeta.max = parsed;
        break;
      }
      case 'step': {
        const parsed = decodeNumberLike(value);
        if (parsed != null) portMeta.step = parsed;
        break;
      }
      case 'ui_min': {
        const parsed = decodeNumberLike(value);
        if (parsed != null) portMeta.uiMin = parsed;
        break;
      }
      case 'ui_max': {
        const parsed = decodeNumberLike(value);
        if (parsed != null) portMeta.uiMax = parsed;
        break;
      }
      case 'ui_step': {
        const parsed = decodeNumberLike(value);
        if (parsed != null) portMeta.uiStep = parsed;
        break;
      }
      case 'ui_control':
      case 'ui': {
        const uiControl = decodeStringLike(value);
        if (uiControl) portMeta.uiControl = uiControl;
        break;
      }
    }
  }
  return Object.keys(portMeta).length > 0 ? portMeta : null;
}

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

const enumVariantsFromTypeExpr = (value: unknown): string[] => {
  const record = normalizeTypeExprRecord(value);
  if (!record) return [];
  const variants =
    (Array.isArray(record.Enum) ? record.Enum : null) ??
    (record.type === 'Enum' && Array.isArray(record.value) ? record.value : null);
  if (!Array.isArray(variants)) return [];
  return variants
    .map((variant) => {
      if (typeof variant === 'string') return variant.trim();
      if (!variant || typeof variant !== 'object') return null;
      const name = (variant as { name?: unknown }).name;
      return typeof name === 'string' ? name.trim() : null;
    })
    .filter((name): name is string => Boolean(name && name.length > 0));
};

export function pipelineDataTypeFromTypeExpr(ty: unknown): PipelineDataType {
  if (typeof ty === 'string') {
    const trimmed = ty.trim();
    return trimmed.length ? pipelineDataTypeFromTypeExpr({ Scalar: trimmed }) : 'Generic';
  }
  const record = normalizeTypeExprRecord(ty);
  if (!record) return 'Generic';
  const opaque = typeof record.Opaque === 'string' ? record.Opaque : null;
  if (opaque) {
    const lower = opaque.toLowerCase();
    if (lower === 'image' || lower.startsWith('image:')) {
      const flavor = lower.startsWith('image:') ? opaque.slice('image:'.length) : null;
      return { kind: 'image', label: flavor ? `Image (${flavor})` : 'Image' };
    }
    if (lower === 'cv:binary_image') {
      return { kind: 'image', label: 'Image (binary)' };
    }
    return opaque;
  }
  const scalar = typeof record.Scalar === 'string' ? record.Scalar : null;
  if (scalar) {
    const lower = scalar.trim().toLowerCase();
    if (lower === 'bool' || lower === 'boolean') return 'bool';
    if (lower === 'int' || lower === 'i32' || lower === 'i64') return 'int';
    if (lower === 'float' || lower === 'f32' || lower === 'f64') return 'float';
    if (lower === 'string') return 'string';
    return lower || scalar;
  }
  const variants = enumVariantsFromTypeExpr(record);
  const hasEnum = Boolean((record as { Enum?: unknown }).Enum) || (record as { type?: unknown }).type === 'Enum';
  if (hasEnum) {
    return { kind: 'enum', label: 'Enum', variants, settable: variants.length > 0 };
  }
  return 'Generic';
}

export function registryPortTypeFor(snapshot: unknown, nodeId: string, portKey: string): PipelineDataType | null {
  const registryEntry = findRegistryEntryFor(snapshot, nodeId);
  if (registryEntry?.inputs) {
    const resolved = resolveRegistryPort(registryEntry.inputs as Record<string, PipelineDataType>, portKey);
    if (resolved) return resolved;
  }
  const snapshotRecord = asRecord(snapshot);
  const registryNodes = Array.isArray(snapshotRecord?.nodes) ? snapshotRecord.nodes : [];
  const resolvedNodeId = resolveRegistrySnapshotNodeId(snapshot, nodeId);
  const registryNode = resolvedNodeId
    ? registryNodes.find((node) => String(asRecord(node)?.id ?? '') === resolvedNodeId) ?? null
    : null;
  const registryNodeRecord = asRecord(registryNode);
  const ports = Array.isArray(registryNodeRecord?.input_ports)
    ? registryNodeRecord.input_ports
    : Array.isArray(registryNodeRecord?.inputPorts)
      ? registryNodeRecord.inputPorts
      : [];
  const normalizedPort = normalizeNodePortKey(portKey);
  const portEntry =
    ports.find((p) => normalizeNodePortKey(String(asRecord(p)?.name ?? '')) === normalizedPort) ?? null;
  const portRecord = asRecord(portEntry);
  if (!portRecord?.ty) return null;
  return pipelineDataTypeFromTypeExpr(portRecord.ty);
}

export function extractNodeValueDescriptors(
  graph: unknown,
  overrides: Record<string, Record<string, PipelineNodeValue>>,
  registrySnapshot: unknown
): PipelineNodeValueDescriptor[] {
  if (!graph || typeof graph !== 'object') return [];
  const plan = graph as {
    nodes?: Record<string, unknown> | unknown[];
  };
  const rawNodes = plan.nodes;
  const nodeEntries: Array<{ nodeId: string; backendId: string | null; sourceId: string | null; node: unknown }> = Array.isArray(rawNodes)
    ? rawNodes
        .map((node, index): { nodeId: string; backendId: string | null; sourceId: string | null; node: unknown } | null => {
          if (!node || typeof node !== 'object') return null;
          const record = node as Record<string, unknown>;
          const backendId = typeof record.id === 'string' ? record.id : null;
          return { nodeId: String(index), backendId, sourceId: backendId, node };
        })
        .filter((entry): entry is { nodeId: string; backendId: string | null; sourceId: string | null; node: unknown } => Boolean(entry))
    : rawNodes && typeof rawNodes === 'object'
      ? Object.entries(rawNodes as Record<string, unknown>).map(([nodeId, node]) => {
          const record = node && typeof node === 'object' ? (node as Record<string, unknown>) : null;
          const backendId =
            typeof record?.backendId === 'string'
              ? record.backendId
              : typeof record?.id === 'string'
                ? record.id
                : null;
          const sourceId =
            typeof (record as { source?: { id?: unknown } } | null)?.source?.id === 'string'
              ? (record as { source?: { id?: string } }).source?.id ?? null
              : typeof (record as { info?: { id?: unknown } } | null)?.info?.id === 'string'
                ? (record as { info?: { id?: string } }).info?.id ?? null
                : typeof record?.id === 'string'
                  ? record.id
                  : null;
          return { nodeId, backendId, sourceId, node };
        })
      : [];
  if (!nodeEntries.length) return [];
  const entries: PipelineNodeValueDescriptor[] = [];
  for (const { nodeId, backendId, sourceId, node } of nodeEntries) {
    if (!node || typeof node !== 'object') continue;
    const nodeRecord = node as {
      label?: string | null;
      metadata?: { name?: string | null };
      info?: { values?: Record<string, PipelineNodeValue> };
      values?: Record<string, PipelineNodeValue>;
      const_inputs?: Array<[string, { type?: string; value?: unknown }]>;
    };
    const values = nodeRecord.info?.values ?? nodeRecord.values ?? null;
    const constInputs = Array.isArray(nodeRecord.const_inputs) ? nodeRecord.const_inputs : [];
    const constValues =
      values ??
      (constInputs.length
        ? Object.fromEntries(
            constInputs
              .map(([name, payload]) => {
                const key = typeof name === 'string' ? name : '';
                if (!key.trim()) return null;
                const lookupId = sourceId ?? backendId ?? nodeId;
                const dataType = registryPortTypeFor(registrySnapshot, lookupId, key) ?? 'Generic';
                const variants = getDataTypeVariants(dataType ?? undefined);
                let value = payload?.value;
                if (typeof value === 'number' && Number.isFinite(value) && variants.length > 0) {
                  const idx = Math.trunc(value);
                  if (idx >= 0 && idx < variants.length) {
                    value = variants[idx] ?? value;
                  }
                }
                return [key, { dataType, value }] as [string, PipelineNodeValue];
              })
              .filter((entry): entry is [string, PipelineNodeValue] => Boolean(entry))
          )
        : null);
    const resolvedValues = values ?? constValues;
    if (!resolvedValues || typeof resolvedValues !== 'object') continue;
    const nodeLabel = nodeRecord.metadata?.name ?? nodeRecord.label ?? nodeId;
    const overrideRecord = overrides?.[nodeId] ?? {};
    const lookupId = sourceId ?? backendId ?? nodeId;
    for (const [portKey, value] of Object.entries(resolvedValues)) {
      const normalizedPort = normalizeNodePortKey(portKey);
      if (!normalizedPort) continue;
      const defaultValue = value ?? null;
      const dataType = defaultValue?.dataType ?? null;
      const registryType = registryPortTypeFor(registrySnapshot, lookupId, portKey);
      const dataVariants = getDataTypeVariants(dataType ?? undefined);
      const registryVariants = getDataTypeVariants(registryType ?? undefined);
      const resolvedType =
        registryType && (dataType == null || (registryVariants.length > 0 && dataVariants.length === 0))
          ? registryType
          : dataType ?? registryType ?? null;
      const overrideValue = overrideRecord?.[normalizedPort] ?? null;
      const metadata =
        findPortMetadata(nodeRecord as unknown as Record<string, unknown>, portKey) ??
        registryPortMetadataFor(registrySnapshot, lookupId, portKey);
      entries.push({
        nodeId,
        backendId,
        sourceId,
        nodeLabel: nodeLabel ?? nodeId,
        portKey,
        dataType: resolvedType,
        defaultValue: defaultValue ?? null,
        overrideValue: overrideValue ?? null,
        metadata
      });
    }
  }
  return entries.sort((a, b) => {
    const nodeOrder = a.nodeLabel.localeCompare(b.nodeLabel);
    return nodeOrder !== 0 ? nodeOrder : a.portKey.localeCompare(b.portKey);
  });
}

export function safeCloneGraph<T>(value: T): T {
  if (typeof structuredClone === 'function') {
    try {
      return structuredClone(value);
    } catch (error) {
      if (error instanceof DOMException && error.name === 'DataCloneError') {
        // Fall back to JSON cloning below.
      } else {
        throw error;
      }
    }
  }
  return JSON.parse(JSON.stringify(value)) as T;
}

export const isDaedalusPlan = (plan: PipelineGraphPlan | null | undefined): boolean =>
  Boolean(plan && (plan.format === 'daedalus' || plan.daedalus));

export function applyDaedalusNodeOverrides(
  plan: PipelineGraphPlan,
  overrides: Record<string, Record<string, PipelineNodeValue>>
): void {
  for (const [nodeId, nodeOverrides] of Object.entries(overrides ?? {})) {
    const node = plan.nodes?.[nodeId];
    if (!node) continue;
    const existingValues = node.info?.values ?? {};
    const valueKeyByNormalized = new Map<string, string>();
    for (const key of Object.keys(existingValues)) {
      const normalized = normalizeNodePortKey(key);
      if (!normalized) continue;
      if (!valueKeyByNormalized.has(normalized)) {
        valueKeyByNormalized.set(normalized, key);
      }
    }
    const nextValues: Record<string, PipelineNodeValue> = { ...existingValues };
    for (const [portKey, value] of Object.entries(nodeOverrides ?? {})) {
      const normalized = normalizeNodePortKey(portKey);
      if (!normalized) continue;
      const existingKey = valueKeyByNormalized.get(normalized) ?? portKey;
      if (value) {
        nextValues[existingKey] = value;
      } else {
        delete nextValues[existingKey];
      }
    }
    node.info = { ...node.info, values: nextValues };
  }
}

export function applyPipelineOverridesToGraph(
  pipelineId: string,
  graph: unknown,
  inputOverridesById: Record<string, Record<string, PipelineNodeValue>>,
  nodeOverridesById: Record<string, Record<string, Record<string, PipelineNodeValue>>>
): unknown {
  if (!graph || typeof graph !== 'object') return graph;
  const inputOverrides = inputOverridesById[pipelineId] ?? {};
  const nodeOverrides = nodeOverridesById[pipelineId] ?? {};
  const hasInputs = Object.keys(inputOverrides).length > 0;
  const hasNodes = Object.keys(nodeOverrides).length > 0;
  if (!hasInputs && !hasNodes) return graph;
  let basePlan: PipelineGraphPlan | null = null;
  const nodes = (graph as { nodes?: unknown }).nodes;
  const connections = (graph as { connections?: unknown }).connections;
  if (nodes && typeof nodes === 'object' && !Array.isArray(nodes) && Array.isArray(connections)) {
    basePlan = graph as PipelineGraphPlan;
  } else {
    try {
      basePlan = fromApiGraphPlan(graph);
    } catch (error) {
      console.warn('Failed to parse pipeline graph for overrides', error);
      return graph;
    }
  }
  if (!basePlan) return graph;
  const plan = safeCloneGraph(basePlan);
  if (hasInputs) {
    plan.pipelineInputValues = { ...(plan.pipelineInputValues ?? {}), ...inputOverrides };
  }
  if (hasNodes) {
    if (isDaedalusPlan(plan)) {
      applyDaedalusNodeOverrides(plan, nodeOverrides);
    } else {
      plan.nodeValueOverrides = mergeNodeOverrides(plan.nodeValueOverrides ?? {}, nodeOverrides);
    }
  }
  return serializeGraphPlan(plan);
}

type PipelineTuningState = {
  get stream(): StreamInfo | null;
  get streamId(): string;
  get pipelineTuningPlan(): PipelineGraphPlan | null;
  get pipelineTuningGraph(): unknown;
  get pipelineTuningPanelOpen(): boolean;
  set pipelineTuningPanelOpen(value: boolean);
  get pipelineTuningPipelineId(): string | null;
  set pipelineTuningPipelineId(value: string | null);
  get pipelineTuningEngineConfigOpen(): boolean;
  set pipelineTuningEngineConfigOpen(value: boolean);
  get pipelineTuningError(): string | null;
  set pipelineTuningError(value: string | null);
  get pipelineTuningLoading(): boolean;
  set pipelineTuningLoading(value: boolean);
  get pipelineTuningDragState(): PipelineTuningDragState | null;
  set pipelineTuningDragState(value: PipelineTuningDragState | null);
  get pipelineTuningResizeState(): PipelineTuningResizeState | null;
  set pipelineTuningResizeState(value: PipelineTuningResizeState | null);
  get pipelineTuningPosition(): PipelineTuningPosition;
  set pipelineTuningPosition(value: PipelineTuningPosition);
  get pipelineTuningSize(): PipelineTuningSize;
  set pipelineTuningSize(value: PipelineTuningSize);
  get pipelineTuningUiOverride(): PipelineUi | null;
  set pipelineTuningUiOverride(value: PipelineUi | null);
  get pipelineTuningGraphOverride(): unknown;
  set pipelineTuningGraphOverride(value: unknown);
  get pipelineTuningLiveGraph(): unknown;
  set pipelineTuningLiveGraph(value: unknown);
  get pipelineInputOverridesById(): Record<string, Record<string, PipelineNodeValue>>;
  set pipelineInputOverridesById(value: Record<string, Record<string, PipelineNodeValue>>);
  get pipelineNodeOverridesById(): Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  set pipelineNodeOverridesById(value: Record<string, Record<string, Record<string, PipelineNodeValue>>>);
  get pipelineInputDraftsById(): Record<string, Record<string, string>>;
  set pipelineInputDraftsById(value: Record<string, Record<string, string>>);
  get pipelineNodeDraftsById(): Record<string, Record<string, Record<string, string>>>;
  set pipelineNodeDraftsById(value: Record<string, Record<string, Record<string, string>>>);
  get pipelineInputErrorsById(): Record<string, Record<string, string>>;
  set pipelineInputErrorsById(value: Record<string, Record<string, string>>);
  get pipelineNodeErrorsById(): Record<string, Record<string, Record<string, string>>>;
  set pipelineNodeErrorsById(value: Record<string, Record<string, Record<string, string>>>);
  get pipelineTuningApplyBusy(): boolean;
  set pipelineTuningApplyBusy(value: boolean);
  get pipelineTuningApplyQueuedById(): Record<string, boolean>;
  set pipelineTuningApplyQueuedById(value: Record<string, boolean>);
  get pipelineTuningLastAppliedSignatureById(): Record<string, string>;
  set pipelineTuningLastAppliedSignatureById(value: Record<string, string>);
  get pipelineTuningLastAppliedNodeOverridesById(): Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  set pipelineTuningLastAppliedNodeOverridesById(value: Record<string, Record<string, Record<string, PipelineNodeValue>>>);
  get pipelineTuningApplyRafById(): Map<string, number>;
  get pipelineGraphCache(): Record<string, unknown>;
  get pipelineRegistrySnapshot(): unknown;
};

type PipelineTuningDeps = {
  ensurePipelineRegistry: () => Promise<void>;
  ensurePipelineGraphAndOutputs: (
    pipelineId: string,
    forceRefresh?: boolean
  ) => Promise<{ graphJson: unknown; filtered: string[]; types: Record<string, PipelineDataType | null | undefined> }>;
  awaitStreamUpdatesSocket: (streamId: string, timeoutMs?: number) => Promise<StreamUpdatesSocket>;
};

export function createPipelineTuningController(state: PipelineTuningState, deps: PipelineTuningDeps) {
  const tuningApply = createPipelineTuningApply(state, {
    awaitStreamUpdatesSocket: deps.awaitStreamUpdatesSocket,
    applyPipelineOverridesToGraph,
    isDaedalusPlan
  });
  const tuningDrafts = createPipelineTuningDrafts(state, {
    normalizeNodePortKey,
    schedulePipelineTuningApply: tuningApply.schedulePipelineTuningApply
  });
  const pointerHandlers = createPipelineTuningPointerHandlers(state);

  const {
    readPipelineInputDraft,
    readPipelineNodeDraft,
    setPipelineInputDraft,
    clearPipelineInputDraft,
    setPipelineNodeDraft,
    clearPipelineNodeDraft,
    setPipelineInputError,
    setPipelineNodeError,
    setPipelineInputOverride,
    setPipelineNodeOverride,
    updatePipelineInputDraft,
    updatePipelineNodeDraft,
    updatePipelineNodeValue
  } = tuningDrafts;

  const { schedulePipelineTuningApply, applyPipelineTuningOverrides } = tuningApply;

  const {
    stopPipelineTuningPointerTracking,
    startPipelineTuningPointerTracking,
    onPipelineTuningPointerMove,
    onPipelineTuningPointerUp,
    startPipelineTuningDrag,
    startPipelineTuningResize
  } = pointerHandlers;

  const extractPipelineUi = (graph: unknown): PipelineUi | null => {
    const parseMaybeNestedJson = (value: string): unknown => {
      const parsed = parseMetadataValue(value);
      if (typeof parsed !== 'string') return parsed;
      const trimmed = parsed.trim();
      if (!trimmed) return parsed;
      if ((trimmed.startsWith('{') && trimmed.endsWith('}')) || (trimmed.startsWith('[') && trimmed.endsWith(']'))) {
        return parseMetadataValue(trimmed);
      }
      return parsed;
    };

    const graphRecord = asRecord(graph);
    if (!graphRecord) return null;
    const metadata =
      graphRecord.metadata ??
      asRecord(graphRecord.daedalus)?.metadata ??
      asRecord(graphRecord.graph)?.metadata ??
      asRecord(graphRecord.pipeline_graph)?.metadata ??
      null;
    const metadataRecord = asRecord(metadata);
    if (!metadataRecord) return null;
    const raw =
      metadataRecord[PIPELINE_UI_METADATA_KEY] ??
      metadataRecord['helios.pipeline_ui'] ??
      metadataRecord['pipeline.ui'] ??
      null;
    if (!raw) return null;
    if (typeof raw === 'string') {
      return (parseMaybeNestedJson(raw) as PipelineUi | null) ?? null;
    }
    if (raw && typeof raw === 'object' && 'value' in (raw as Record<string, unknown>)) {
      const value = (raw as { value?: unknown }).value;
      if (typeof value === 'string') {
        return (parseMaybeNestedJson(value) as PipelineUi | null) ?? null;
      }
      return (value as PipelineUi | null) ?? null;
    }
    return raw as PipelineUi;
  };

  const extractStreamGraphForPipeline = (pipelineId: string): unknown | null => {
    const stream = state.stream;
    const manifest = stream?.manifest ?? null;
    if (!manifest) return null;
    const normalized = String(pipelineId ?? '').trim();
    if (!normalized.length) return null;
    const manifestRecord = asRecord(manifest);
    const readGraph = (value: unknown) => {
      const record = asRecord(value);
      return record?.pipeline_graph ?? null;
    };
    const activeId = asTrimmedString(manifestRecord?.active_pipeline_id);
    if (activeId && activeId === normalized) return null;
    if (Array.isArray(manifestRecord?.pipelines)) {
      for (const entry of manifestRecord.pipelines) {
        const entryRecord = asRecord(entry);
        if (!entryRecord) continue;
        const entryId = typeof entryRecord.pipeline_id === 'string' ? entryRecord.pipeline_id.trim() : '';
        if (entryId && entryId === normalized) {
          return readGraph(entry);
        }
      }
    }
    return null;
  };

  async function openPipelineTuningPanel(pipelineId: string): Promise<void> {
    state.pipelineTuningPipelineId = pipelineId;
    state.pipelineTuningPanelOpen = true;
    state.pipelineTuningEngineConfigOpen = false;
    state.pipelineTuningError = null;
    state.pipelineTuningLoading = false;
    state.pipelineTuningUiOverride = null;
    state.pipelineTuningGraphOverride = null;
    state.pipelineTuningLiveGraph = null;
    if (typeof window !== 'undefined') {
      const margin = 12;
      const minWidth = 460;
      const minHeight = 420;
      const maxWidth = Math.max(minWidth, window.innerWidth - margin * 2);
      const maxHeight = Math.max(minHeight, window.innerHeight - margin * 2);
      const nextWidth = clamp(state.pipelineTuningSize.width, minWidth, maxWidth);
      const nextHeight = clamp(state.pipelineTuningSize.height, minHeight, maxHeight);
      const maxX = Math.max(margin, window.innerWidth - nextWidth - margin);
      const maxY = Math.max(margin, window.innerHeight - nextHeight - margin);
      state.pipelineTuningSize = { width: nextWidth, height: nextHeight };
      state.pipelineTuningPosition = {
        x: clamp(state.pipelineTuningPosition.x, margin, maxX),
        y: clamp(state.pipelineTuningPosition.y, margin, maxY)
      };
    }
    if (pipelineId === RAW_PIPELINE_ID) return;
    state.pipelineTuningLoading = true;
    try {
      const [, graphResult] = await Promise.all([
        deps.ensurePipelineRegistry(),
        deps.ensurePipelineGraphAndOutputs(pipelineId, true)
      ]);
      const graphJson = graphResult?.graphJson ?? state.pipelineGraphCache?.[pipelineId] ?? null;
      const uiOverride = extractPipelineUi(graphJson);
      state.pipelineTuningUiOverride = uiOverride;
      state.pipelineTuningGraphOverride = graphJson ?? null;
      state.pipelineTuningLiveGraph = extractStreamGraphForPipeline(pipelineId);
    } catch (err) {
      console.warn('Failed to load pipeline graph for tuning panel', err);
      state.pipelineTuningError = 'Unable to load pipeline graph.';
    } finally {
      state.pipelineTuningLoading = false;
    }
  }

  function closePipelineTuningPanel(): void {
    state.pipelineTuningPanelOpen = false;
    state.pipelineTuningPipelineId = null;
    state.pipelineTuningDragState = null;
    state.pipelineTuningResizeState = null;
    state.pipelineTuningEngineConfigOpen = false;
    state.pipelineTuningUiOverride = null;
    state.pipelineTuningGraphOverride = null;
    state.pipelineTuningLiveGraph = null;
    stopPipelineTuningPointerTracking();
  }

  function applyPipelineOverridesToGraphForState(pipelineId: string, graph: unknown): unknown {
    return applyPipelineOverridesToGraph(
      pipelineId,
      graph,
      state.pipelineInputOverridesById,
      state.pipelineNodeOverridesById
    );
  }

  return {
    readPipelineInputDraft,
    readPipelineNodeDraft,
    setPipelineInputDraft,
    clearPipelineInputDraft,
    setPipelineNodeDraft,
    clearPipelineNodeDraft,
    setPipelineInputError,
    setPipelineNodeError,
    setPipelineInputOverride,
    setPipelineNodeOverride,
    updatePipelineInputDraft,
    updatePipelineNodeDraft,
    updatePipelineNodeValue,
    schedulePipelineTuningApply,
    applyPipelineTuningOverrides,
    openPipelineTuningPanel,
    closePipelineTuningPanel,
    stopPipelineTuningPointerTracking,
    startPipelineTuningPointerTracking,
    onPipelineTuningPointerMove,
    onPipelineTuningPointerUp,
    startPipelineTuningDrag,
    startPipelineTuningResize,
    applyPipelineOverridesToGraph: applyPipelineOverridesToGraphForState,
    nodeValueSignature,
    pipelineOverrideSignature,
    clamp
  };
}
