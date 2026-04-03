import type {
  ChannelPolicy,
  PipelineDataType,
  PipelineExternalRef,
  PipelineGraphPlan,
  PipelineGraphNode,
  PipelineInputQueueConfig,
  PipelineMissingDataPolicy,
  PipelineNodeMetadata,
  PipelineNodeSyncConfig,
  PipelineNodeValue,
  PipelineOutputSinkConfig,
  PipelinePortMetadata,
  PipelineSignature,
  PipelineStalenessPolicy,
  PipelineSyncGroupConfig,
  PipelineTickSource,
  PipelineTypeDescriptor
} from '$lib/types/pipeline';
import type {
  ApiGraphConnection,
  ApiGraphNode,
  ApiGraphPlan,
  ApiExternalPipelineRef,
  ApiNodeEndpoint,
  ApiNodeValue,
  ApiPipelineSignature,
  ApiPortDescriptor,
  ApiPortOverrides,
  MissingDataPolicy,
  NodeMetadata,
  NodeSyncConfig,
  PortMetadata,
  StalenessPolicy,
  SyncGroupConfig,
  TickSource,
  NodeSchedule
} from '$lib/types/pipeline-api';
import { cloneDataType, clonePortMappings, normalizeNodeStyle } from './model';
import { refreshPipelineIoCaches } from './boundary';
import { toDaedalusGraph } from './daedalusGraph';

export function emptyPipelineGraphPlan(): PipelineGraphPlan {
  return {
    format: 'daedalus',
    daedalus: { metadata: {} },
    nodes: {},
    connections: [],
    pipelineInputs: {},
    pipelineOutputs: {},
    pipelineInputValues: {},
    nodeValueOverrides: {},
    pipelineInputConfigs: {},
    pipelineOutputConfigs: {}
  };
}

type SerializeOptions = {
  minimal?: boolean;
};

const toApiStalenessPolicy = (policy: PipelineStalenessPolicy): StalenessPolicy => {
  switch (policy.kind) {
    case 'allowAny':
    case 'requireExact':
      return policy.kind;
    case 'maxLagCount':
      return { maxLagCount: { maxDistance: Math.max(0, policy.maxDistance) } };
    case 'maxLagDuration':
      return { maxLagDuration: { maxLagMs: Math.max(0, policy.maxLagMs) } };
    default:
      return 'allowAny';
  }
};

const toApiMissingPolicy = (policy: PipelineMissingDataPolicy): MissingDataPolicy => {
  switch (policy.kind) {
    case 'allowNone':
      return 'allowNone';
    case 'skipTick':
      return 'skipTick';
    case 'wait':
      return { wait: { timeout_ms: policy.timeoutMs ?? null } };
    default:
      return 'allowNone';
  }
};

const toApiTickSource = (source?: PipelineTickSource | null): TickSource | undefined => {
  if (!source || source.kind === 'ports') {
    return 'ports';
  }
  const interval = Number.isFinite(source.intervalMs) ? Math.max(1, Math.round(source.intervalMs)) : 1;
  return { timer: { interval_ms: interval } };
};

const toApiSyncGroup = (group: PipelineSyncGroupConfig): SyncGroupConfig => ({
  id: group.id,
  ports: group.ports.slice(),
  matchKey: group.matchKey,
  readiness: group.readiness,
  staleness: toApiStalenessPolicy(group.staleness),
  drop: group.drop,
  missing: toApiMissingPolicy(group.missing)
});

const toApiNodeSyncConfig = (
  config: PipelineNodeSyncConfig | null | undefined
): NodeSyncConfig | null | undefined => {
  if (config === undefined) return undefined;
  if (config === null) return null;
  return {
    groups: config.groups.map(toApiSyncGroup),
    tickPolicy: {
      mode: config.tickPolicy.mode,
      requiredGroups: config.tickPolicy.requiredGroups.slice()
    },
    tickSource: toApiTickSource(config.tickSource)
  };
};

export function serializeGraphPlan(plan: PipelineGraphPlan, options?: SerializeOptions): unknown {
  if (plan.format === 'daedalus') {
    return toDaedalusGraph(plan);
  }

  const minimal = Boolean(options?.minimal);
  refreshPipelineIoCaches(plan);

  const serializePorts = (
    ports: Record<string, PipelineDataType> | undefined
  ): Record<string, ApiPortDescriptor> | undefined => {
    if (!ports) return undefined;
    const entries = Object.entries(ports)
      .map(([key, value]) => [key, toApiPortDescriptor(value, minimal)] as const);
    return entries.length ? Object.fromEntries(entries) : undefined;
  };

  const serializeValues = (
    values: Record<string, PipelineNodeValue> | undefined
  ): Record<string, ApiNodeValue> | undefined => {
    if (!values) return undefined;
    const entries = Object.entries(values)
      .map(([key, entry]) => {
        const value = toApiNodeValue(entry, minimal);
        return value ? [key, value] : null;
      })
      .filter((entry): entry is [string, ApiNodeValue] => Boolean(entry));
    return entries.length ? Object.fromEntries(entries) : undefined;
  };

  const serializeNodeValueOverrides = (
    overrides: Record<string, Record<string, PipelineNodeValue>> | undefined
  ): Record<string, Record<string, ApiNodeValue>> | undefined => {
    if (!overrides) return undefined;
    const entries = Object.entries(overrides)
      .map(([nodeId, values]) => {
        const encoded = serializeValues(values);
        return encoded ? [nodeId, encoded] : null;
      })
      .filter((entry): entry is [string, Record<string, ApiNodeValue>] => Boolean(entry));
    return entries.length ? Object.fromEntries(entries) : undefined;
  };

  const serializeSignature = (
    signature: PipelineSignature | null | undefined
  ): ApiPipelineSignature | undefined => {
    if (!signature) return undefined;
    const inputs = serializePorts(signature.inputs);
    const outputs = serializePorts(signature.outputs);
    if (!inputs && !outputs) return undefined;
    return {
      ...(inputs ? { inputs } : {}),
      ...(outputs ? { outputs } : {})
    };
  };

  const serializeExternal = (
    external: PipelineExternalRef | null | undefined
  ): ApiExternalPipelineRef | undefined => {
    if (!external) return undefined;
    const pipelineId = typeof external.pipelineId === 'string' ? external.pipelineId.trim() : '';
    if (!pipelineId) return undefined;
    const signature = serializeSignature(external.signature);
    return {
      pipelineId,
      revision: external.revision ?? undefined,
      alias: external.alias ?? undefined,
      ...(signature ? { signature } : {})
    };
  };

  const serializePortMappings = (
    mappings: PipelineGraphNode['portMappings']
  ): ApiGraphNode['portMappings'] | undefined => {
    if (!mappings) return undefined;
    const normalize = (record: Record<string, string> | undefined) => {
      if (!record) return undefined;
      const entries = Object.entries(record)
        .map(([key, value]) => {
          const parent = (key ?? '').toString().trim().toLowerCase();
          const child = (value ?? '').toString().trim().toLowerCase();
          if (!parent || !child) return null;
          return [parent, child] as const;
        })
        .filter((entry): entry is [string, string] => Boolean(entry));
      return entries.length ? Object.fromEntries(entries) : undefined;
    };
    const inputs = normalize(mappings.inputs);
    const outputs = normalize(mappings.outputs);
    if (!inputs && !outputs) return undefined;
    return {
      ...(inputs ? { inputs } : {}),
      ...(outputs ? { outputs } : {})
    };
  };

  const nodes: Record<string, ApiGraphNode> = Object.fromEntries(
    Object.entries(plan.nodes ?? {}).map(([id, node]) => {
      const info: ApiGraphNode['info'] = {
        id: node.info.id,
        location: { ...node.info.location },
        values: serializeValues(node.info.values)
      };

      const serialized: ApiGraphNode = {
        id: node.id,
        backendId: node.backendId,
        info,
        ...(node.embedded ? { embedded: serializeGraphPlan(node.embedded, options) as ApiGraphPlan } : {})
      };

      const syncCopy = toApiNodeSyncConfig(node.sync);
      if (syncCopy !== undefined) {
        serialized.sync = syncCopy;
      }
      const mappingCopy = serializePortMappings(node.portMappings);
      if (mappingCopy) {
        serialized.portMappings = mappingCopy;
      }

      if (node.external !== undefined) {
        const encodedExternal = serializeExternal(node.external);
        if (encodedExternal) {
          serialized.external = encodedExternal;
        } else if (node.external === null) {
          serialized.external = null;
        }
      }

      if (!minimal) {
        serialized.metadata = toApiNodeMetadata(node.metadata);
        const inputs = serializePorts(node.inputs);
        if (inputs && Object.keys(inputs).length > 0) {
          serialized.inputs = inputs;
        }
        const outputs = serializePorts(node.outputs);
        if (outputs && Object.keys(outputs).length > 0) {
          serialized.outputs = outputs;
        }
      }

      return [id, serialized];
    })
  );

  const connections: ApiGraphConnection[] = Array.isArray(plan.connections)
    ? plan.connections.map((connection) => {
        const apiConnection: ApiGraphConnection = {
          from: toApiEndpoint(connection.from),
          to: toApiEndpoint(connection.to)
        };
        if (connection.policy) {
          apiConnection.policy = connection.policy;
        }
        return apiConnection;
      })
    : [];

  const encoded: ApiGraphPlan = {
    nodes,
    connections,
    pipelineInputs: serializePorts(plan.pipelineInputs),
    pipelineOutputs: serializePorts(plan.pipelineOutputs),
    pipelineInputValues: serializeValues(plan.pipelineInputValues),
    nodeValueOverrides: serializeNodeValueOverrides(plan.nodeValueOverrides),
    pipelineInputConfigs: serializeInputConfigs(plan.pipelineInputConfigs),
    pipelineOutputConfigs: serializeOutputConfigs(plan.pipelineOutputConfigs)
  };
  return encoded;
}

const serializeInputConfigs = (
  configs: Record<string, PipelineInputQueueConfig> | undefined
): Record<string, { policy: ChannelPolicy; capacity: number }> | undefined => {
  if (!configs) return undefined;
  const entries = Object.entries(configs).map(([name, config]) => [name, { policy: config.policy, capacity: Math.max(1, Number(config.capacity) || 1) }] as const);
  return entries.length ? Object.fromEntries(entries) : undefined;
};

const serializeOutputConfigs = (
  configs: Record<string, PipelineOutputSinkConfig> | undefined
): Record<string, { capacity: number }> | undefined => {
  if (!configs) return undefined;
  const entries = Object.entries(configs).map(([name, config]) => [name, { capacity: Math.max(1, Number(config.capacity) || 1) }] as const);
  return entries.length ? Object.fromEntries(entries) : undefined;
};

export function applyPaletteToGraphPlan(
  plan: PipelineGraphPlan,
  palette: Record<string, PipelineTypeDescriptor>
): PipelineGraphPlan {
  const decorate = (value: PipelineDataType): PipelineDataType => cloneDataType(value, palette);

  const decoratePorts = (ports: Record<string, PipelineDataType> | undefined) => {
    if (!ports) return undefined;
    return Object.fromEntries(Object.entries(ports).map(([port, dataType]) => [port, decorate(dataType)]));
  };

  const decorateSignature = (signature: PipelineSignature | null | undefined): PipelineSignature | undefined => {
    if (!signature) return undefined;
    const inputs = decoratePorts(signature.inputs);
    const outputs = decoratePorts(signature.outputs);
    if (!inputs && !outputs) return undefined;
    return {
      ...(inputs ? { inputs } : {}),
      ...(outputs ? { outputs } : {})
    };
  };

  const decoratedNodes: Record<string, PipelineGraphNode> = Object.fromEntries(
    Object.entries(plan.nodes ?? {}).map(([id, node]) => [
      id,
      {
        ...node,
        inputs: decoratePorts(node.inputs),
        outputs: decoratePorts(node.outputs),
        portMappings: clonePortMappings(node.portMappings),
        external:
          node.external === undefined
            ? undefined
            : node.external === null
              ? null
              : {
                  ...node.external,
                  signature: decorateSignature(node.external.signature)
                },
        embedded: node.embedded ? applyPaletteToGraphPlan(node.embedded, palette) : node.embedded
      }
    ])
  );

  const cloneValues = (values: Record<string, PipelineNodeValue> | undefined) =>
    values
      ? Object.fromEntries(
          Object.entries(values).map(([key, entry]) => [
            key,
            {
              dataType: entry.dataType,
              value: entry.value
            }
          ])
        )
      : undefined;

  const cloneNodeOverrideRecord = (
    overrides: Record<string, Record<string, PipelineNodeValue>> | undefined
  ): Record<string, Record<string, PipelineNodeValue>> | undefined => {
    if (!overrides) return undefined;
    const entries = Object.entries(overrides)
      .map(([nodeId, values]) => {
        const cloned = cloneValues(values);
        return cloned ? [nodeId, cloned] : null;
      })
      .filter((entry): entry is [string, Record<string, PipelineNodeValue>] => Boolean(entry));
    return entries.length > 0 ? Object.fromEntries(entries) : undefined;
  };

  return {
    format: 'daedalus',
    ...(plan.daedalus ? { daedalus: { ...(plan.daedalus ?? {}) } } : {}),
    nodes: decoratedNodes,
    connections: plan.connections ?? [],
    pipelineInputs: decoratePorts(plan.pipelineInputs),
    pipelineOutputs: decoratePorts(plan.pipelineOutputs),
    pipelineInputValues: cloneValues(plan.pipelineInputValues),
    nodeValueOverrides: cloneNodeOverrideRecord(plan.nodeValueOverrides),
    pipelineInputConfigs: { ...(plan.pipelineInputConfigs ?? {}) },
    pipelineOutputConfigs: { ...(plan.pipelineOutputConfigs ?? {}) }
  };
}

function toApiEndpoint(endpoint: PipelineGraphPlan['connections'][number]['from']): ApiNodeEndpoint {
  return {
    node: endpoint.node,
    port: endpoint.port
  };
}

function toApiNodeValue(value: PipelineNodeValue | undefined, minimal: boolean): ApiNodeValue | null {
  if (!value) return null;
  const dataType = toApiPortDescriptor(value.dataType, minimal);
  return {
    dataType,
    value: value.value ?? null
  };
}

function toApiPortDescriptor(value: PipelineDataType | undefined, minimal: boolean): ApiPortDescriptor {
  if (!value) {
    return { kind: 'generic' };
  }
  if (typeof value === 'string') {
    const trimmed = value.trim();
    return { kind: trimmed || 'generic' };
  }
  const kind = value.kind ?? value.descriptor?.id ?? 'generic';
  if (minimal) {
    return { kind };
  }
  const overrides: ApiPortOverrides = {};
  if (value.format) overrides.format = value.format;
  if (value.label) overrides.label = value.label;
  if (value.color) overrides.color = value.color;
  if (value.summary) overrides.summary = value.summary;
  if (typeof value.settable === 'boolean') overrides.settable = value.settable;
  const element = value.element ? toApiPortDescriptor(value.element, minimal) : undefined;
  const variants = Array.isArray(value.variants) && value.variants.length > 0 ? value.variants.slice() : undefined;

  const result: ApiPortDescriptor = { kind };
  if (Object.keys(overrides).length > 0) {
    result.overrides = overrides;
  }
  if (element) {
    result.element = element;
  }
  if (variants) {
    result.variants = variants;
  }
  return result;
}

function toApiNodeMetadata(metadata: PipelineNodeMetadata): NodeMetadata {
  const schedule: NodeSchedule = {
    priority: metadata.schedule?.priority ?? 128,
    rate_limit: null
  };

  const normalizeGroup = (group: unknown): string[] => {
    if (!Array.isArray(group)) return [];
    return group
      .filter((item): item is string => typeof item === 'string' && item.trim().length > 0)
      .map((item) => item.trim());
  };

  const latencyHint =
    metadata.latency_hint && typeof metadata.latency_hint === 'object'
      ? (metadata.latency_hint as NodeMetadata['latency_hint'])
      : null;
  const alignedInputs = Array.isArray(metadata.alignedInputs)
    ? metadata.alignedInputs
        .map((value) => (typeof value === 'string' ? value.trim() : ''))
        .filter((value, index, array) => value.length > 0 && array.indexOf(value) === index)
    : [];
  const style = normalizeNodeStyle(metadata.style);

  return {
    name: metadata.name,
    summary: metadata.summary ?? null,
    description: metadata.description ?? null,
    documentation: metadata.documentation ?? null,
    doc: metadata.doc ?? null,
    provider: metadata.provider ?? null,
    aligned_inputs: alignedInputs.length > 0 ? alignedInputs : undefined,
    latency_hint: latencyHint,
    tags: Array.isArray(metadata.tags)
      ? metadata.tags
          .filter((tag): tag is string => typeof tag === 'string' && tag.trim().length > 0)
          .map((tag) => tag.trim())
      : [],
    categories: Array.isArray(metadata.categories)
      ? metadata.categories.map(normalizeGroup).filter((group) => group.length > 0)
      : [],
    schedule,
    state: Array.isArray(metadata.state) ? (metadata.state as NodeMetadata['state']) : [],
    input_ports: mapPortMetadataRecord(metadata.inputPorts),
    output_ports: mapPortMetadataRecord(metadata.outputPorts),
    ...(style ? { style } : {})
  };
}

function mapPortMetadataRecord(
  ports: Record<string, PipelinePortMetadata> | undefined
): Record<string, PortMetadata> | undefined {
  if (!ports) return undefined;
  const entries = Object.entries(ports)
    .map(([key, metadata]) => {
      const normalized = normalizePortMetadata(metadata);
      return normalized ? [key, normalized] : null;
    })
    .filter((entry): entry is [string, PortMetadata] => Boolean(entry));
  return entries.length ? Object.fromEntries(entries) : undefined;
}

function normalizePortMetadata(constraint?: PipelinePortMetadata | null): PortMetadata | null {
  if (!constraint) return null;
  const description = constraint.description?.trim();
  const allowed = Array.isArray(constraint.allowedValues)
    ? constraint.allowedValues.map((value) => value.trim()).filter((value) => value.length > 0)
    : undefined;
  const min = typeof constraint.min === 'number' ? constraint.min : undefined;
  const max = typeof constraint.max === 'number' ? constraint.max : undefined;
  const step = typeof constraint.step === 'number' ? constraint.step : undefined;
  const uiMin = typeof constraint.uiMin === 'number' ? constraint.uiMin : undefined;
  const uiMax = typeof constraint.uiMax === 'number' ? constraint.uiMax : undefined;
  const uiStep = typeof constraint.uiStep === 'number' ? constraint.uiStep : undefined;
  const uiControl = typeof constraint.uiControl === 'string' && constraint.uiControl.trim().length > 0 ? constraint.uiControl.trim() : undefined;
  if (
    !description &&
    (!allowed || allowed.length === 0) &&
    min === undefined &&
    max === undefined &&
    step === undefined &&
    uiMin === undefined &&
    uiMax === undefined &&
    uiStep === undefined &&
    uiControl === undefined
  ) {
    return null;
  }
  const result: PortMetadata = {};
  if (description) result.description = description;
  if (allowed && allowed.length > 0) result.allowed_values = allowed;
  if (min !== undefined) result.min = min;
  if (max !== undefined) result.max = max;
  if (step !== undefined) result.step = step;
  if (uiMin !== undefined) result.ui_min = uiMin;
  if (uiMax !== undefined) result.ui_max = uiMax;
  if (uiStep !== undefined) result.ui_step = uiStep;
  if (uiControl !== undefined) result.ui_control = uiControl;
  return result;
}
