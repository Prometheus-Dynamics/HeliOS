import { emptyPipelineGraphPlan } from '$lib/features/pipelines/graph';
import { fromDaedalusGraph, isDaedalusGraph } from '$lib/features/pipelines/daedalusGraph';
import { cloneUnknown, trimmedString, isRecord, parseStringArray, parseWorkKey, parseReadinessPolicy, parseStalenessPolicy, parseDropPolicy, parseMissingPolicy, parseTickPolicy, parseTickSource } from './normalizeHelpers';
import { cloneNodeMetadata } from './cloneHelpers';
import type {
  PipelineChildPortMapping,
  PipelineDataType,
  PipelineExternalRef,
  PipelineGraphNode,
  PipelineGraphPlan,
  PipelineInputQueueConfig,
  PipelineNodeMetadata,
  PipelineNodeSyncConfig,
  PipelineNodeValue,
  PipelineOutputSinkConfig,
  PipelineSignature,
  PipelineSyncGroupConfig
} from '$lib/types/pipeline';
import type {
  ApiGraphPlan,
  ApiGraphNode,
  ApiNodeValue,
  ApiPipelineInputConfig,
  ApiPipelineOutputConfig,
  ApiExternalPipelineRef,
  ApiPipelineSignature,
  ApiPortDescriptor
} from '$lib/types/pipeline-api';

const fromApiSyncGroup = (value: unknown): PipelineSyncGroupConfig | undefined => {
  if (!isRecord(value)) return undefined;
  const id = trimmedString(value.id);
  const ports = parseStringArray(value.ports) ?? [];
  const matchKey = parseWorkKey(value.matchKey);
  const readiness = parseReadinessPolicy(value.readiness);
  const staleness = parseStalenessPolicy(value.staleness);
  const drop = parseDropPolicy(value.drop);
  const missing = parseMissingPolicy(value.missing);
  if (!id || ports.length === 0 || !matchKey || !readiness || !staleness || !drop || !missing) {
    return undefined;
  }
  return {
    id,
    ports,
    matchKey,
    readiness,
    staleness,
    drop,
    missing
  };
};

function fromApiSyncConfig(value: unknown): PipelineNodeSyncConfig | undefined {
  if (!isRecord(value)) return undefined;
  if (!Array.isArray(value.groups) || value.groups.length === 0) return undefined;
  const groups = value.groups
    .map((entry) => fromApiSyncGroup(entry))
    .filter((entry): entry is PipelineSyncGroupConfig => Boolean(entry));
  if (groups.length === 0) return undefined;
  const tickPolicy = parseTickPolicy(value.tickPolicy);
  if (!tickPolicy) return undefined;
  const tickSource = parseTickSource(value.tickSource ?? value.tick_source ?? value.ticksource);
  if (!tickSource) return undefined;
  return {
    groups,
    tickPolicy,
    tickSource
  };
}

export function fromApiPortDescriptor(
  descriptor: ApiPortDescriptor | null | undefined
): PipelineDataType | null {
  if (!descriptor) return null;
  const kind = trimmedString(descriptor.kind);
  if (!kind) return null;

  const overrides = descriptor.overrides ?? undefined;
  const extras: Partial<Exclude<PipelineDataType, string>> = {};

  if (overrides) {
    const format = trimmedString(overrides.format);
    if (format) extras.format = format;
    const label = trimmedString(overrides.label);
    if (label) extras.label = label;
    const color = trimmedString(overrides.color);
    if (color) extras.color = color;
    const summary = trimmedString(overrides.summary);
    if (summary) extras.summary = summary;
    if (typeof overrides.settable === 'boolean') extras.settable = overrides.settable;
  }

  const element = descriptor.element ? fromApiPortDescriptor(descriptor.element) : null;
  if (element) {
    (extras as { element?: PipelineDataType }).element = element;
  }

  const variants = Array.isArray(descriptor.variants)
    ? descriptor.variants
        .map((value: any) => (typeof value === 'string' ? value.trim() : ''))
        .filter((value: any): value is string => value.length > 0)
    : [];
  if (variants.length > 0) {
    (extras as { variants?: string[] }).variants = variants;
  }

  const hasExtras = Object.keys(extras).length > 0;
  if (!hasExtras) {
    return kind;
  }

  return {
    kind,
    ...extras
  };
}

function fromApiPortRecord(
  record: Record<string, ApiPortDescriptor> | undefined | null
): Record<string, PipelineDataType> | undefined {
  if (!record) return undefined;
  const entries = Object.entries(record)
    .map(([port, descriptor]) => {
      const normalized = fromApiPortDescriptor(descriptor);
      return normalized ? [port, normalized] : null;
    })
    .filter((entry): entry is [string, PipelineDataType] => Boolean(entry));
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

function fromApiNodeValues(
  values: Record<string, ApiNodeValue> | undefined | null
): Record<string, PipelineNodeValue> | undefined {
  if (!values) return undefined;
  const entries = Object.entries(values)
    .map(([port, value]) => {
      const dataType = fromApiPortDescriptor(value.dataType);
      if (!dataType) return null;
      return [
        port,
        {
          dataType,
          value: cloneUnknown(value.value)
        }
      ] as const;
    })
    .filter((entry): entry is [string, PipelineNodeValue] => Boolean(entry));
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

function fromApiSignature(
  signature: ApiPipelineSignature | undefined | null
): PipelineSignature | undefined {
  if (!signature) return undefined;
  const inputs = fromApiPortRecord(signature.inputs);
  const outputs = fromApiPortRecord(signature.outputs);
  if (!inputs && !outputs) return undefined;
  return {
    ...(inputs ? { inputs } : {}),
    ...(outputs ? { outputs } : {})
  };
}

function fromApiExternalRef(
  reference: ApiExternalPipelineRef | undefined | null
): PipelineExternalRef | undefined {
  if (!reference) return undefined;
  const pipelineId = trimmedString(reference.pipelineId);
  if (!pipelineId) return undefined;
  const signature = fromApiSignature(reference.signature ?? undefined);
  return {
    pipelineId,
    ...(reference.revision ? { revision: reference.revision } : {}),
    ...(reference.resolvedRevision ? { resolvedRevision: reference.resolvedRevision } : {}),
    ...(reference.alias ? { alias: reference.alias } : {}),
    ...(signature ? { signature } : {})
  };
}

const normalizePortMap = (
  record: Record<string, unknown> | undefined
): Record<string, string> | undefined => {
  if (!record) return undefined;
  const entries = Object.entries(record)
    .map(([key, value]) => {
      const parent = trimmedString(key) ?? key?.toString?.();
      const child = typeof value === 'string' ? value : value == null ? undefined : String(value);
      const parentKey = parent?.toLowerCase().trim();
      const childKey = child?.toLowerCase().trim();
      if (!parentKey || !childKey) return null;
      return [parentKey, childKey] as const;
    })
    .filter((entry): entry is [string, string] => Boolean(entry));
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
};

function fromApiPortMappings(mapping: unknown): PipelineChildPortMapping | undefined {
  if (!isRecord(mapping)) return undefined;
  const inputs = normalizePortMap(mapping.inputs as Record<string, unknown> | undefined);
  const outputs = normalizePortMap(mapping.outputs as Record<string, unknown> | undefined);
  if (!inputs && !outputs) return undefined;
  return {
    ...(inputs ? { inputs } : {}),
    ...(outputs ? { outputs } : {})
  };
}

function fromApiGraphNode(node: ApiGraphNode): PipelineGraphNode {
  const inputs = fromApiPortRecord(node.inputs);
  const outputs = fromApiPortRecord(node.outputs);
  const portMappings = fromApiPortMappings((node as unknown as { portMappings?: unknown }).portMappings);
  const values = fromApiNodeValues(node.info?.values);
  const embedded = node.embedded ? fromApiGraphPlan(node.embedded) : null;
  const sync = fromApiSyncConfig(node.sync);
  const external =
    node.external === undefined
      ? undefined
      : node.external === null
        ? null
        : fromApiExternalRef(node.external as ApiExternalPipelineRef | undefined | null);
  const fallbackName = trimmedString(node.metadata?.name) ?? trimmedString(node.backendId) ?? trimmedString(node.info?.id) ?? 'Pipeline node';
  return {
    id: node.id,
    backendId: node.backendId,
    metadata: cloneNodeMetadata(node.metadata as unknown as PipelineNodeMetadata | undefined, fallbackName),
    inputs,
    outputs,
    info: {
      id: node.info.id,
      location: { ...node.info.location },
      ...(values ? { values } : {})
    },
    sync,
    embedded,
    ...(portMappings ? { portMappings } : {}),
    ...(external !== undefined ? { external } : {}),
    source: cloneUnknown(node)
  };
}

export function fromApiGraphPlan(plan: unknown): PipelineGraphPlan {
  if (!plan) return emptyPipelineGraphPlan();
  if (isDaedalusGraph(plan)) return fromDaedalusGraph(plan);
  console.warn('Legacy pipeline graph format is no longer supported; expected a Daedalus graph.');
  return emptyPipelineGraphPlan();
}

function fromApiInputConfigs(
  record: Record<string, ApiPipelineInputConfig> | undefined
): Record<string, PipelineInputQueueConfig> | undefined {
  if (!record) return undefined;
  const entries = Object.entries(record).map(
    ([name, config]) =>
      [
        name.toLowerCase(),
        {
          policy: (config.policy as PipelineInputQueueConfig['policy']) ?? 'NewestWins',
          capacity: Math.max(1, Number(config.capacity) || 1)
        }
      ] as const
  );
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

function fromApiOutputConfigs(
  record: Record<string, ApiPipelineOutputConfig> | undefined
): Record<string, PipelineOutputSinkConfig> | undefined {
  if (!record) return undefined;
  const entries = Object.entries(record).map(([name, config]) => [name.toLowerCase(), { capacity: Math.max(1, Number(config.capacity) || 1) }] as const);
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}
