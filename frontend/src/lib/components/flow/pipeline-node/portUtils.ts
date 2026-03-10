import {
  formatPipelineValue,
  getDataTypeVariants,
  isDataTypeSettable,
  parseEnumVariant,
  resolveDataTypeKey
} from '$lib/features/pipelines/valueFormatting';
import type { ApiGraphNode, ApiPortDescriptor } from '$lib/types/pipeline-api';
import {
  normalizeColor,
  resolveThemeColor
} from '../nodePalette';
import { clampChannel } from '../nodeColors';
import type {
  PipelineDataType,
  PipelineGraphNode,
  PipelineNodeValue,
  PipelinePortMetadata,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import type {
  BasePortRenderInfo,
  CompactPortHandle,
  EnumVariantOption,
  PortRenderInfo
} from './types';

export const DEFAULT_PORT_COLOR = 'var(--color-primary-300, #38bdf8)';
const BOOLEAN_TYPE_KEYS = new Set(['bool', 'boolean']);
const PIXEL_KEY_TOKEN = 'pixel';
const PIXEL_TYPE_KEYS = new Set(['pixel', 'rgba', 'rgb', 'color']);
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

const normalizeTypeKey = (raw: string | null | undefined): string | null => {
  if (!raw) return null;
  const trimmed = raw.trim().toLowerCase();
  if (!trimmed) return null;
  const colonIndex = trimmed.lastIndexOf(':');
  return colonIndex >= 0 ? trimmed.slice(colonIndex + 1) : trimmed;
};

export const ENUM_INLINE_PATTERN = /^enum\s*<.*>$/iu;

export const isBooleanTypeKey = (key: string | null | undefined): boolean => {
  if (!key) return false;
  return BOOLEAN_TYPE_KEYS.has(normalizeTypeKey(key) ?? '');
};

export const isPixelTypeKey = (key: string | null | undefined): boolean => {
  const normalized = normalizeTypeKey(key);
  if (!normalized) return false;
  return normalized.endsWith(PIXEL_KEY_TOKEN) || PIXEL_TYPE_KEYS.has(normalized);
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

const componentHex = (value: number): string => value.toString(16).padStart(2, '0');

const extractPixelConstant = (
  value: PipelineNodeValue | undefined
): { hex: string; alpha: number } | null => {
  if (!value || !value.value || typeof value.value !== 'object') return null;
  const record = value.value as Record<string, unknown>;
  const r = clampChannel(record.r);
  const g = clampChannel(record.g);
  const b = clampChannel(record.b);
  if (r == null || g == null || b == null) return null;
  const a = clampChannel(record.a) ?? 255;
  return {
    hex: `#${componentHex(r)}${componentHex(g)}${componentHex(b)}`,
    alpha: a
  };
};

export const simplifyTypeString = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const trimmed = value.trim();
  if (!trimmed || trimmed === 'Generic') return null;
  if (ENUM_INLINE_PATTERN.test(trimmed)) {
    return 'Enum';
  }
  return trimmed;
};

export const presentTypeString = (value: string | null): string | null => {
  if (!value) return null;
  return simplifyTypeString(value) ?? value;
};

export const resolvePortKind = (type: PipelineDataType | undefined, descriptor?: ApiPortDescriptor): string | null => {
  if (!type) return normalizeColor(descriptor?.kind);
  if (typeof type === 'string') {
    const kind = normalizeColor(type);
    if (kind && kind !== 'Generic') return kind;
    const descriptorKind = normalizeColor(descriptor?.kind);
    if (descriptorKind) return descriptorKind;
    return kind;
  }
  const kind = normalizeColor((type as { kind?: string }).kind);
  if (kind) return kind;
  const descriptorKind = normalizeColor(descriptor?.kind);
  if (descriptorKind) return descriptorKind;
  return null;
};

export const resolvePortColor = (
  type: PipelineDataType | undefined,
  descriptor?: ApiPortDescriptor
): { color: string | null; source: 'override' | 'direct' | 'descriptor' | 'fallback' } => {
  const overrideColor = normalizeColor(descriptor?.overrides?.color);
  if (overrideColor) {
    return {
      color: resolveThemeColor(overrideColor) ?? overrideColor,
      source: 'override'
    };
  }
  if (type && typeof type === 'string') {
    const trimmed = type.trim();
    if (trimmed) {
      const lower = trimmed.toLowerCase();
      const parts = lower
        .split(':')
        .map((part) => part.trim())
        .filter(Boolean);
      const key =
        parts.includes('image')
          ? 'image'
          : ENUM_INLINE_PATTERN.test(trimmed) || parts.includes('enum')
            ? 'enum'
            : parts.includes('json')
              ? 'json'
              : parts[parts.length - 1] ?? lower;
      if (key && key !== 'generic') {
        return { color: colorFromKey(key), source: 'direct' };
      }
    }
  }
  if (type && typeof type === 'object') {
    const directColor = normalizeColor((type as { color?: string | null }).color);
    if (directColor) {
      return {
        color: resolveThemeColor(directColor) ?? directColor,
        source: 'direct'
      };
    }
    const descriptorColor = normalizeColor((type as { descriptor?: { color?: string | null } }).descriptor?.color);
    if (descriptorColor) {
      return {
        color: resolveThemeColor(descriptorColor) ?? descriptorColor,
        source: 'descriptor'
      };
    }
  }
  return { color: null, source: 'fallback' };
};

export const normalizeString = (value: unknown): string | null => {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
};

export const normalizePortName = (value: string | null | undefined): string | null => {
  const normalized = normalizeString(value);
  return normalized ? normalized.toLowerCase() : null;
};

export const resolveTypeLabel = (
  type: PipelineDataType | undefined,
  descriptor?: ApiPortDescriptor
): string | null => {
  const descriptorLabel = normalizeString(descriptor?.overrides?.label);
  if (descriptorLabel) return presentTypeString(descriptorLabel);
  if (type == null) {
    const descriptorKind = normalizeString(descriptor?.kind);
    return descriptorKind && descriptorKind !== 'Generic' ? presentTypeString(descriptorKind) : null;
  }
  if (typeof type === 'string') {
    const imageFlavor = type.trim().match(/^image:(.+)$/i)?.[1]?.trim() ?? null;
    if (imageFlavor) return `Image (${imageFlavor})`;
    const simplified = presentTypeString(type);
    return simplified && simplified !== 'Generic' ? simplified : null;
  }
  const directLabel = normalizeString(type.label);
  if (directLabel) return presentTypeString(directLabel);
  const paletteLabel = normalizeString((type as { descriptor?: { label?: string } }).descriptor?.label);
  if (paletteLabel) return presentTypeString(paletteLabel);
  const kindLabel = normalizeString(type.kind);
  if (kindLabel && kindLabel !== 'Generic') return presentTypeString(kindLabel);
  const descriptorKind = normalizeString(descriptor?.kind);
  if (descriptorKind && descriptorKind !== 'Generic') return presentTypeString(descriptorKind);
  return null;
};

export const resolveTypeDetail = (
  type: PipelineDataType | undefined,
  descriptor?: ApiPortDescriptor
): string | null => {
  const descriptorSummary = normalizeString(descriptor?.overrides?.summary);
  if (descriptorSummary) return descriptorSummary;
  const descriptorFormat = normalizeString(descriptor?.overrides?.format);
  if (descriptorFormat) return descriptorFormat;
  if (!type) return null;
  if (typeof type === 'string') {
    return null;
  }
  const summary = normalizeString(type.summary);
  if (summary) return summary;
  const format = normalizeString((type as { format?: string }).format);
  if (format) return format;
  const paletteSummary = normalizeString((type as { descriptor?: { summary?: string } }).descriptor?.summary);
  if (paletteSummary) return paletteSummary;
  return null;
};

const resolveConstantTypeLabel = (dataType: PipelineDataType | string): string | null => {
  if (typeof dataType === 'string') {
    return presentTypeString(dataType);
  }
  if (dataType.label && dataType.label.trim()) {
    return presentTypeString(dataType.label.trim());
  }
  if (dataType.descriptor?.label && dataType.descriptor.label.trim()) {
    return presentTypeString(dataType.descriptor.label.trim());
  }
  if (dataType.kind && dataType.kind.trim()) {
    return presentTypeString(dataType.kind.trim());
  }
  return null;
};

const formatConstantDisplay = (existing: PipelineNodeValue | undefined) => {
  if (!existing) return null;
  const formatted = formatPipelineValue(existing);
  let display = formatted;
  if (display.length === 0) {
    const raw = existing.value;
    if (raw === null) {
      display = 'null';
    } else if (raw === undefined) {
      display = 'undefined';
    } else if (typeof raw === 'string' && raw.length === 0) {
      display = '""';
    } else if (Array.isArray(raw) && raw.length === 0) {
      display = '[]';
    } else if (typeof raw === 'object' && raw && Object.keys(raw).length === 0) {
      display = '{}';
    } else {
      display = String(raw);
    }
  }
  const raw = typeof existing.value === 'string' ? existing.value : null;
  const typeKey = resolveDataTypeKey(existing.dataType);
  const typeLabel = resolveConstantTypeLabel(existing.dataType);
  const tooltipPrefix = typeLabel ? `Constant value (${typeLabel})` : 'Constant value';
  let tooltipDisplay = display;
  if (raw && typeKey?.toLowerCase() === 'enum') {
    const parsed = parseEnumVariant(raw);
    tooltipDisplay =
      parsed.value && parsed.value !== parsed.label ? `${parsed.label} (${parsed.value})` : parsed.label;
  }
  const tooltip = `${tooltipPrefix}: ${tooltipDisplay}`;
  return { display, tooltip, raw };
};

export type BuildPortListOptions = {
  direction: 'input' | 'output';
  node: PipelineGraphNode | null | undefined;
  apiNode: ApiGraphNode | null;
  registryEntry?: PipelineRegistryEntry | null;
  issuePortSet: Set<string>;
  issuePortMessages?: Record<string, string[]> | null;
  highlightedPortName: string | null;
  searchTokens?: string[];
  order?: string[];
  nodeBaseId: string;
  syncAssignments: Map<string, { groupId: string; color: string }>;
  syncModeActive: boolean;
};

type BasePortListContext = Pick<
  BuildPortListOptions,
  'direction' | 'node' | 'apiNode' | 'order'
>;

const normalizeSearchValue = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const trimmed = value.trim().toLowerCase();
  return trimmed.length > 0 ? trimmed : null;
};

const matchesSearchTokens = (tokens: string[], values: Array<string | null | undefined>): boolean => {
  if (tokens.length === 0) return false;
  const haystack = values
    .map((value) => normalizeSearchValue(value))
    .filter(Boolean)
    .join(' ');
  if (!haystack) return false;
  return tokens.every((token) => haystack.includes(token));
};

const resolveOrderedPortNames = ({ direction, node, apiNode, order }: BasePortListContext): string[] => {
  const record = (direction === 'input' ? node?.inputs : node?.outputs) ?? {};
  const descriptorRecord = (
    direction === 'input' ? apiNode?.inputs : apiNode?.outputs
  ) as Record<string, ApiPortDescriptor> | undefined;
  const fallbackSet = new Set<string>([
    ...Object.keys(record),
    ...(descriptorRecord ? Object.keys(descriptorRecord) : [])
  ]);
  const fallback = Array.from(fallbackSet);
  const baseOrder = order && order.length ? order : fallback;
  return baseOrder.filter(
    (name: string | null | undefined): name is string => typeof name === 'string' && name.length > 0
  );
};

const resolveHandleId = (
  nodeBaseId: string,
  name: string,
  duplicateCount: number,
  occurrenceIndex: number
): string => {
  const baseId = `${nodeBaseId}__${name}`;
  return duplicateCount > 1 ? `${baseId}-${occurrenceIndex}` : baseId;
};

export const buildCompactPortHandleList = ({
  direction,
  node,
  apiNode,
  order,
  nodeBaseId,
  issuePortNames = [],
  highlightedPortName = null
}: BasePortListContext & {
  nodeBaseId: string;
  issuePortNames?: string[];
  highlightedPortName?: string | null;
}): CompactPortHandle[] => {
  const orderedPorts = resolveOrderedPortNames({ direction, node, apiNode, order });
  if (orderedPorts.length === 0) {
    return [];
  }
  const duplicateCounts: Record<string, number> = {};
  for (const name of orderedPorts) {
    duplicateCounts[name] = (duplicateCounts[name] ?? 0) + 1;
  }
  const issues = new Set(issuePortNames);
  const occurrence = new Map<string, number>();
  return orderedPorts.map((name) => {
    const occurrenceIndex = occurrence.get(name) ?? 0;
    occurrence.set(name, occurrenceIndex + 1);
    const normalized = normalizePortName(name);
    return {
      name,
      handleId: resolveHandleId(nodeBaseId, name, duplicateCounts[name] ?? 1, occurrenceIndex),
      hasIssue: issues.has(name),
      isHighlighted: Boolean(highlightedPortName && normalized && normalized === highlightedPortName)
    };
  });
};

export function buildPortList(options: BuildPortListOptions): PortRenderInfo[] {
  const {
    direction,
    node,
    apiNode,
    registryEntry,
    issuePortSet,
    issuePortMessages,
    highlightedPortName,
    searchTokens,
    order,
    nodeBaseId,
    syncAssignments,
    syncModeActive
  } = options;
  const activeSearchTokens = Array.isArray(searchTokens) ? searchTokens : [];
  const searchActive = activeSearchTokens.length > 0;
  const record = (direction === 'input' ? node?.inputs : node?.outputs) ?? {};
  const descriptorRecord = (
    direction === 'input' ? apiNode?.inputs : apiNode?.outputs
  ) as Record<string, ApiPortDescriptor> | undefined;
  const portMetadataRecord =
    direction === 'input'
      ? (registryEntry?.metadata?.inputPorts ?? node?.metadata?.inputPorts ?? {})
      : (registryEntry?.metadata?.outputPorts ?? node?.metadata?.outputPorts ?? {});
  const constantValues = node?.info?.values;
  const orderedPorts = resolveOrderedPortNames({ direction, node, apiNode, order });
  const shouldHighlightSync = syncModeActive && direction === 'input';
  const resolveSyncState = (portName: string | null) => {
    if (!shouldHighlightSync || !portName) {
      return null;
    }
    const assignment = syncAssignments.get(portName);
    if (assignment) {
      return { role: 'group' as const, groupId: assignment.groupId, color: assignment.color };
    }
    return { role: 'unsynced' as const, groupId: null, color: null };
  };
  if (orderedPorts.length === 0) {
    return [];
  }
  const duplicateCounts: Record<string, number> = {};
  for (const name of orderedPorts) {
    duplicateCounts[name] = (duplicateCounts[name] ?? 0) + 1;
  }
  const occurrence = new Map<string, number>();
  return orderedPorts.map((name: string) => {
    const index = occurrence.get(name) ?? 0;
    occurrence.set(name, index + 1);
    const info = createPortInfo(
      name,
      nodeBaseId,
      record,
      descriptorRecord,
      portMetadataRecord,
      constantValues,
      issuePortMessages ?? undefined,
      duplicateCounts[name] ?? 1,
      index
    );
    const normalized = normalizePortName(info.name);
    const isHighlighted = Boolean(highlightedPortName && normalized && normalized === highlightedPortName);
    const syncState = resolveSyncState(normalized);
    const isSearchMatch = searchActive
      ? matchesSearchTokens(activeSearchTokens, [
          info.name,
          info.label,
          info.detail,
          info.dataTypeKey,
          info.kind,
          info.constantDisplay,
          info.constantRaw
        ])
      : false;
    return { ...info, hasIssue: issuePortSet.has(info.name), isHighlighted, syncState, isSearchMatch };
  });
}

function createPortInfo(
  name: string,
  nodeBaseId: string,
  record: Record<string, PipelineDataType>,
  descriptorRecord: Record<string, ApiPortDescriptor> | undefined,
  portMetadataRecord: Record<string, PipelinePortMetadata> | undefined,
  constantValues: Record<string, PipelineNodeValue> | undefined,
  issuePortMessages: Record<string, string[]> | undefined,
  duplicateCount = 1,
  occurrenceIndex = 0
): BasePortRenderInfo {
  const handleId = resolveHandleId(nodeBaseId, name, duplicateCount, occurrenceIndex);
  const type = record?.[name] ?? 'Generic';
  const descriptor = descriptorRecord?.[name];
  const portMetadata = portMetadataRecord?.[name] ?? null;
  const existingConstant =
    constantValues && Object.prototype.hasOwnProperty.call(constantValues, name)
      ? constantValues[name]
      : undefined;
  const constantInfo = formatConstantDisplay(existingConstant);
  const colorInfo = resolvePortColor(type, descriptor);
  const color = colorInfo.color ?? DEFAULT_PORT_COLOR;
  const kind = resolvePortKind(type, descriptor);
  const dataTypeKey = resolveDataTypeKey(type);
  const isBooleanPort = isBooleanTypeKey(dataTypeKey);
  const isNumericPort = isNumericTypeKey(dataTypeKey);
  const booleanValue =
    isBooleanPort && existingConstant && typeof existingConstant.value === 'boolean'
      ? existingConstant.value
      : false;
  const pixelInfoDetected = extractPixelConstant(existingConstant);
  const isPixelPort = isPixelTypeKey(dataTypeKey) || pixelInfoDetected != null;
  const pixelInfo = isPixelPort ? pixelInfoDetected : null;
  const numericValue =
    isNumericPort && existingConstant && typeof existingConstant.value === 'number'
      ? existingConstant.value
      : null;
  const numericDisplay = numericValue != null ? String(numericValue) : null;
  const numericMin = typeof portMetadata?.min === 'number' ? portMetadata.min : null;
  const numericMax = typeof portMetadata?.max === 'number' ? portMetadata.max : null;
  const numericStep =
    typeof portMetadata?.step === 'number'
      ? String(portMetadata.step)
      : isIntegerNumericTypeKey(dataTypeKey)
        ? '1'
        : 'any';
  const numericRange =
    numericMin != null && numericMax != null && Number.isFinite(numericMin) && Number.isFinite(numericMax)
      ? numericMax - numericMin
      : null;
  const numericHasSlider = Boolean(numericRange != null && numericRange > 0 && numericRange <= 5000);
  const numericSliderStep =
    numericStep !== 'any'
      ? numericStep
      : typeof portMetadata?.step === 'number'
        ? String(portMetadata.step)
        : numericHasSlider && numericRange != null
          ? String(numericRange / 100)
          : '0.01';
  const settable = isDataTypeSettable(type);
  const variantValues = getDataTypeVariants(type);
  const mergedVariants = [
    ...variantValues,
    ...((portMetadata?.allowedValues ?? []).filter((entry) => !variantValues.includes(entry)) as string[])
  ];
  const enumVariants: EnumVariantOption[] = mergedVariants.map((raw) => {
    const { label, value } = parseEnumVariant(raw);
    return { raw, label, value };
  });
  const isEnumPort = enumVariants.length > 0;
  const defaultDisplay =
    !existingConstant && portMetadata && portMetadata.defaultValue !== undefined && portMetadata.defaultValue !== null
      ? String(portMetadata.defaultValue)
      : null;
  const defaultTooltip = defaultDisplay ? `Default: ${defaultDisplay}` : null;
  const issueMessages = issuePortMessages?.[name] ?? null;
  return {
    name,
    handleId,
    type,
    label: resolveTypeLabel(type, descriptor),
    detail: resolveTypeDetail(type, descriptor),
    hasConstant: constantInfo != null,
    constantDisplay: constantInfo?.display ?? null,
    constantTooltip: constantInfo?.tooltip ?? null,
    constantRaw: constantInfo?.raw ?? null,
    issueMessages,
    color,
    hasCustomColor: colorInfo.source === 'override' || colorInfo.source === 'direct',
    kind,
    variants: enumVariants,
    dataTypeKey,
    isBoolean: isBooleanPort && settable,
    booleanValue,
    settable,
    isPixel: isPixelPort,
    pixelHex: pixelInfo?.hex ?? null,
    pixelAlpha: pixelInfo?.alpha ?? null,
    isEnum: isEnumPort && settable,
    isNumeric: isNumericPort && settable,
    numericValue,
    numericDisplay,
    numericStep,
    numericMin,
    numericMax,
    numericHasSlider,
    numericSliderStep,
    defaultDisplay,
    defaultTooltip
  };
}
