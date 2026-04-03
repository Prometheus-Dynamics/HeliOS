import { refreshPipelineIoCaches } from './boundary';
import { cloneDiagnostics } from './diagnostics';
import { cloneUnknown, trimmedString, normalizeOverrides, resolveDescriptorFromPalette } from './normalizeHelpers';
import type {
  PipelineAttachmentSummary,
  PipelineDataType,
  PipelineExternalRef,
  PipelineGraphNode,
  PipelineGraphPlan,
  PipelineMissingDataPolicy,
  PipelineNodeMetadata,
  PipelineNodeSyncConfig,
  PipelineNodeValue,
  PipelineOverviewPipeline,
  PipelinePortMetadata,
  PipelineRegistryEntry,
  PipelineSignature,
  PipelineChildPortMapping,
  PipelineSyncGroupConfig,
  PipelineStalenessPolicy,
  PipelineTickMode,
  PipelineTickPolicy,
  PipelineTickSource,
  PipelineTypeDescriptor
} from '$lib/types/pipeline';

type Palette = Record<string, PipelineTypeDescriptor>;

function cloneDataTypeObject(
  original: Exclude<PipelineDataType, string>,
  palette: Palette
): Exclude<PipelineDataType, string> {
  const cloned: Exclude<PipelineDataType, string> = { ...original };

  if ('descriptor' in cloned) {
    delete (cloned as Record<string, unknown>).descriptor;
  }

  if (original.element !== undefined) {
    const clonedElement = cloneDataType(original.element as PipelineDataType, palette);
    if (clonedElement !== undefined) {
      (cloned as { element?: PipelineDataType }).element = clonedElement;
    } else {
      delete (cloned as { element?: PipelineDataType }).element;
    }
  }

  const kindCandidate =
    typeof original.kind === 'string' && original.kind.trim()
      ? original.kind.trim()
      : undefined;
  if (kindCandidate) {
    cloned.kind = kindCandidate;
  }

  const labelCandidate =
    typeof original.label === 'string' && original.label.trim() ? original.label.trim() : undefined;
  const descriptor =
    (kindCandidate ? resolveDescriptorFromPalette(kindCandidate, palette) : undefined) ??
    (labelCandidate ? resolveDescriptorFromPalette(labelCandidate, palette) : undefined);

  return normalizeOverrides(cloned, descriptor);
}

export function cloneDataType(value: PipelineDataType, palette: Palette): PipelineDataType {
  if (typeof value === 'string') {
    const kind = value.trim() || value;
    const descriptor = resolveDescriptorFromPalette(kind, palette);
    const base: Exclude<PipelineDataType, string> = { kind };
    return normalizeOverrides(base, descriptor);
  }
  if (value && typeof value === 'object') {
    return cloneDataTypeObject(value as Exclude<PipelineDataType, string>, palette);
  }
  return value;
}

function clonePortRecord(
  ports: Record<string, PipelineDataType> | undefined,
  palette: Palette
): Record<string, PipelineDataType> | undefined {
  if (!ports) return undefined;
  return Object.fromEntries(Object.entries(ports).map(([key, dataType]) => [key, cloneDataType(dataType, palette)]));
}

export const clonePortMappings = (
  mapping: PipelineChildPortMapping | undefined
): PipelineChildPortMapping | undefined => {
  if (!mapping) return undefined;
  const inputs = mapping.inputs ? { ...mapping.inputs } : undefined;
  const outputs = mapping.outputs ? { ...mapping.outputs } : undefined;
  if (!inputs && !outputs) return undefined;
  return {
    ...(inputs ? { inputs } : {}),
    ...(outputs ? { outputs } : {})
  };
};

const cloneSignature = (
  signature: PipelineSignature | null | undefined,
  palette: Palette
): PipelineSignature | undefined => {
  if (!signature) return undefined;
  const inputs = clonePortRecord(signature.inputs, palette);
  const outputs = clonePortRecord(signature.outputs, palette);
  if (!inputs && !outputs) return undefined;
  return {
    ...(inputs ? { inputs } : {}),
    ...(outputs ? { outputs } : {})
  };
};

const cloneExternalRef = (
  external: PipelineExternalRef | null | undefined,
  palette: Palette
): PipelineExternalRef | null | undefined => {
  if (external === undefined) return undefined;
  if (external === null) return null;
  const signature = cloneSignature(external.signature, palette);
  return {
    pipelineId: external.pipelineId,
    ...(external.revision ? { revision: external.revision } : {}),
    ...(external.resolvedRevision ? { resolvedRevision: external.resolvedRevision } : {}),
    ...(external.alias ? { alias: external.alias } : {}),
    ...(signature ? { signature } : {})
  };
};

export function cloneNodeValue(value: PipelineNodeValue, palette: Palette): PipelineNodeValue {
  return {
    dataType: cloneDataType(value.dataType as PipelineDataType, palette),
    value: cloneUnknown(value.value)
  };
}

function cloneValueRecord(
  record: Record<string, PipelineNodeValue> | undefined,
  palette: Palette
): Record<string, PipelineNodeValue> {
  if (!record) return {};
  return Object.fromEntries(Object.entries(record).map(([key, value]) => [key, cloneNodeValue(value, palette)]));
}

function cloneNodeOverrideRecord(
  record: Record<string, Record<string, PipelineNodeValue>> | undefined,
  palette: Palette
): Record<string, Record<string, PipelineNodeValue>> | undefined {
  if (!record) return undefined;
  const entries = Object.entries(record).map(([nodeId, values]) => [nodeId, cloneValueRecord(values, palette)] as const);
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

export function cloneNodeMetadata(
  metadata: PipelineNodeMetadata | undefined | null,
  fallbackName?: string
): PipelineNodeMetadata {
  const name = trimmedString(metadata?.name) ?? fallbackName ?? 'Pipeline node';
  const clone: PipelineNodeMetadata = {
    name
  };

  if (metadata?.summary) clone.summary = metadata.summary;
  if (metadata?.description) clone.description = metadata.description;
  if (metadata?.documentation) clone.documentation = metadata.documentation;
  if (metadata?.doc) clone.doc = metadata.doc;
  if (metadata?.constraints) clone.constraints = metadata.constraints;
  if (metadata?.state) clone.state = metadata.state;
  if (metadata?.style) clone.style = { ...metadata.style };
  const tags = metadata?.tags?.map((tag) => tag?.trim()).filter((tag): tag is string => Boolean(tag));
  if (tags && tags.length > 0) {
    clone.tags = tags;
  }
  const categories = metadata?.categories?.map((path) => path.slice());
  if (categories && categories.length > 0) {
    clone.categories = categories;
  }
  if (metadata?.latency_hint) clone.latency_hint = metadata.latency_hint;
  if (metadata?.schedule) clone.schedule = { ...metadata.schedule };
  if (metadata?.provider) clone.provider = metadata.provider;
  const metaRecord = metadata as unknown as { input_ports?: Record<string, unknown>; output_ports?: Record<string, unknown>; aligned_inputs?: unknown };
  clone.inputPorts = clonePortMetadataMap(metadata?.inputPorts ?? metaRecord?.input_ports);
  clone.outputPorts = clonePortMetadataMap(metadata?.outputPorts ?? metaRecord?.output_ports);
  const alignedInputsRaw = metadata?.alignedInputs ?? metaRecord?.aligned_inputs;
  const alignedInputs = Array.isArray(alignedInputsRaw)
    ? alignedInputsRaw
        .map((entry) => trimmedString(entry))
        .filter((entry, index, array): entry is string => Boolean(entry) && array.indexOf(entry) === index)
    : null;
  if (alignedInputs && alignedInputs.length > 0) {
    clone.alignedInputs = alignedInputs;
  }
  if (metadata && 'gpu' in metadata) {
    if (metadata.gpu === null) {
      clone.gpu = null;
    } else if (metadata.gpu) {
      const copyFormats = (
        formats: { port?: string; kind?: string; format?: string }[] | undefined
      ) =>
        formats
          ?.map((fmt) => {
            if (!fmt) return undefined;
            const entry: { port?: string; kind?: string; format?: string } = {};
            if (fmt.port) entry.port = fmt.port;
            if (fmt.kind) entry.kind = fmt.kind;
            if (fmt.format) entry.format = fmt.format;
            return entry;
          })
          .filter(
            (entry): entry is { port?: string; kind?: string; format?: string } =>
              entry !== undefined
          );
      clone.gpu = {
        preference: metadata.gpu.preference,
        preserves_cpu_layout: metadata.gpu.preserves_cpu_layout,
        side_effects: metadata.gpu.side_effects ? metadata.gpu.side_effects.slice() : undefined,
        inputs: copyFormats(metadata.gpu.inputs),
        outputs: copyFormats(metadata.gpu.outputs)
      };
    }
  }

  return clone;
}

function clonePortMetadataMap(
  ports: Record<string, PipelinePortMetadata> | Record<string, unknown> | undefined
): Record<string, PipelinePortMetadata> | undefined {
  if (!ports) return undefined;
  const entries = Object.entries(ports).map(([key, metadata]) => {
    const meta = metadata as {
      description?: unknown;
      allowedValues?: unknown;
      allowed_values?: unknown;
      min?: unknown;
      max?: unknown;
      step?: unknown;
      uiMin?: unknown;
      uiMax?: unknown;
      uiStep?: unknown;
      uiControl?: unknown;
      ui_min?: unknown;
      ui_max?: unknown;
      ui_step?: unknown;
      ui_control?: unknown;
      defaultValue?: unknown;
      default_value?: unknown;
    };
    const normalizeNumber = (value: unknown): number | undefined => (typeof value === 'number' && Number.isFinite(value) ? value : undefined);
    const normalizeAllowedValues = (raw: unknown): string[] | undefined => {
      const list = Array.isArray(raw) ? raw : undefined;
      if (!list) return undefined;
      const values = list
        .map((entry) => (typeof entry === 'string' ? entry.trim() : typeof entry === 'number' && Number.isFinite(entry) ? String(entry) : ''))
        .filter((entry, index, array): entry is string => entry.length > 0 && array.indexOf(entry) === index);
      return values.length > 0 ? values : undefined;
    };
    const allowedValues = normalizeAllowedValues(meta.allowedValues ?? meta.allowed_values);
    const copy: PipelinePortMetadata = {
      description: typeof meta.description === 'string' && meta.description.trim().length > 0 ? meta.description.trim() : undefined,
      allowedValues,
      min: normalizeNumber(meta.min),
      max: normalizeNumber(meta.max),
      step: normalizeNumber(meta.step),
      uiMin: normalizeNumber(meta.uiMin ?? meta.ui_min),
      uiMax: normalizeNumber(meta.uiMax ?? meta.ui_max),
      uiStep: normalizeNumber(meta.uiStep ?? meta.ui_step),
      uiControl:
        typeof meta.uiControl === 'string'
          ? meta.uiControl
          : typeof meta.ui_control === 'string'
            ? meta.ui_control
            : undefined,
      defaultValue: meta.defaultValue ?? meta.default_value
    };
    return [key, copy] as const;
  });
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

const cloneStalenessPolicy = (policy: PipelineStalenessPolicy): PipelineStalenessPolicy => {
  switch (policy.kind) {
    case 'allowAny':
    case 'requireExact':
      return { kind: policy.kind };
    case 'maxLagCount':
      return { kind: 'maxLagCount', maxDistance: policy.maxDistance };
    case 'maxLagDuration':
      return { kind: 'maxLagDuration', maxLagMs: policy.maxLagMs };
    default:
      return policy;
  }
};

const cloneMissingPolicy = (policy: PipelineMissingDataPolicy): PipelineMissingDataPolicy => {
  switch (policy.kind) {
    case 'allowNone':
    case 'skipTick':
      return { kind: policy.kind };
    case 'wait':
      return { kind: 'wait', timeoutMs: policy.timeoutMs };
    default:
      return policy;
  }
};

const cloneSyncGroup = (group: PipelineSyncGroupConfig): PipelineSyncGroupConfig => ({
  id: group.id,
  ports: group.ports.slice(),
  matchKey: group.matchKey,
  readiness: group.readiness,
  staleness: cloneStalenessPolicy(group.staleness),
  drop: group.drop,
  missing: cloneMissingPolicy(group.missing)
});

const cloneTickSource = (source?: PipelineTickSource | null): PipelineTickSource => {
  if (!source || source.kind === 'ports') {
    return { kind: 'ports' };
  }
  return { kind: 'timer', intervalMs: source.intervalMs };
};

const cloneTickMode = (mode: PipelineTickMode): PipelineTickMode => {
  if (mode === 'allGroups' || mode === 'anyGroup') {
    return mode;
  }
  return { primaryGroup: mode.primaryGroup };
};

const cloneTickPolicy = (policy: PipelineTickPolicy): PipelineTickPolicy => ({
  requiredGroups: policy.requiredGroups.slice(),
  mode: cloneTickMode(policy.mode)
});

function cloneSyncConfigValue(
  config: PipelineNodeSyncConfig | undefined | null
): PipelineNodeSyncConfig | null | undefined {
  if (config === undefined) return undefined;
  if (config === null) return null;
  return {
    groups: config.groups.map(cloneSyncGroup),
    tickPolicy: cloneTickPolicy(config.tickPolicy),
    tickSource: cloneTickSource(config.tickSource)
  };
}

const cloneSyncConfig = (config: PipelineNodeSyncConfig | undefined | null) =>
  cloneSyncConfigValue(config);

export function cloneGraphNode(node: PipelineGraphNode, palette: Palette): PipelineGraphNode {
  const valuesRecord = node.info?.values ? cloneValueRecord(node.info.values, palette) : undefined;
  const values = valuesRecord && Object.keys(valuesRecord).length > 0 ? valuesRecord : undefined;
  return {
    id: node.id,
    backendId: node.backendId,
    metadata: cloneNodeMetadata(node.metadata, node.backendId ?? node.id ?? 'Pipeline node'),
    inputs: clonePortRecord(node.inputs, palette),
    outputs: clonePortRecord(node.outputs, palette),
    info: {
      ...node.info,
      location: node.info?.location ? { ...node.info.location } : { x: 0, y: 0 },
      values
    },
    sync: cloneSyncConfig(node.sync),
    embedded:
      node.embedded === undefined ? undefined : node.embedded === null ? null : cloneGraphPlan(node.embedded, palette),
    external: cloneExternalRef(node.external, palette),
    source: node.source ? cloneUnknown(node.source) : null
  };
}

export function cloneNodeSyncConfig(
  config: PipelineNodeSyncConfig | null | undefined
): PipelineNodeSyncConfig | null | undefined {
  return cloneSyncConfigValue(config);
}

export function cloneGraphPlan(plan: PipelineGraphPlan, palette: Palette): PipelineGraphPlan {
  const cloned: PipelineGraphPlan = {
    format: 'daedalus',
    ...(plan.daedalus ? { daedalus: { ...(plan.daedalus ?? {}) } } : {}),
    nodes: Object.fromEntries(Object.entries(plan.nodes ?? {}).map(([id, node]) => [id, cloneGraphNode(node, palette)])),
    connections: Array.isArray(plan.connections)
      ? plan.connections.map((connection) => ({
          from: { ...connection.from },
          to: { ...connection.to },
          ...(connection.policy ? { policy: connection.policy } : {}),
          ...(connection.style ? { style: { ...connection.style } } : {})
        }))
      : [],
    pipelineInputs: clonePortRecord(plan.pipelineInputs, palette),
    pipelineOutputs: clonePortRecord(plan.pipelineOutputs, palette),
    pipelineInputValues: cloneValueRecord(plan.pipelineInputValues, palette),
    nodeValueOverrides: cloneNodeOverrideRecord(plan.nodeValueOverrides, palette),
    pipelineInputConfigs: { ...(plan.pipelineInputConfigs ?? {}) },
    pipelineOutputConfigs: { ...(plan.pipelineOutputConfigs ?? {}) }
  };
  refreshPipelineIoCaches(cloned);
  return cloned;
}

export function clonePipeline(
  pipeline: PipelineOverviewPipeline,
  palette: Palette
): PipelineOverviewPipeline {
  return {
    ...pipeline,
    diagnostics: cloneDiagnostics(pipeline.diagnostics),
    graph: cloneGraphPlan(pipeline.graph, palette),
    attachments: pipeline.attachments.map((attachment) => ({ ...attachment }))
  };
}

function cloneRegistryMetadata(
  entry: PipelineRegistryEntry
): PipelineRegistryEntry['metadata'] {
  return {
    name: entry.metadata.name,
    summary: entry.metadata.summary,
    categories: entry.metadata.categories ? entry.metadata.categories.map((path) => path.slice()) : undefined,
    tags: entry.metadata.tags ? entry.metadata.tags.slice() : undefined,
    provider: entry.metadata.provider,
    style: entry.metadata.style ? { ...entry.metadata.style } : undefined,
    gpu: entry.metadata.gpu ? { ...entry.metadata.gpu } : undefined,
    inputPorts: entry.metadata.inputPorts
      ? Object.fromEntries(Object.entries(entry.metadata.inputPorts).map(([key, value]) => [key, { ...value }]))
      : undefined,
    outputPorts: entry.metadata.outputPorts
      ? Object.fromEntries(Object.entries(entry.metadata.outputPorts).map(([key, value]) => [key, { ...value }]))
      : undefined
  };
}

export function cloneRegistryEntry(
  entry: PipelineRegistryEntry,
  palette: Palette
): PipelineRegistryEntry {
  const inputs = clonePortRecord(entry.inputs, palette);
  const outputs = clonePortRecord(entry.outputs, palette);
  const faninInputs = entry.faninInputs
    ? entry.faninInputs.map((fanin) => ({
        prefix: fanin.prefix,
        start: fanin.start,
        dataType: cloneDataType(fanin.dataType ?? 'Generic', palette)
      }))
    : undefined;
  const cloned: PipelineRegistryEntry = {
    id: entry.id,
    metadata: cloneRegistryMetadata(entry),
    ...(inputs ? { inputs } : {}),
    ...(outputs ? { outputs } : {}),
    ...(faninInputs ? { faninInputs } : {})
  };
  return cloned;
}

export function cloneAttachments(
  attachments: PipelineAttachmentSummary[]
): PipelineAttachmentSummary[] {
  return attachments.map((attachment) => ({ ...attachment }));
}
