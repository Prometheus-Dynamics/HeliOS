import type { PipelineDataType, PipelineGraphNode, PipelineGraphPlan } from '$lib/types/pipeline';
import { PIPELINE_INPUT_BACKEND_ID, PIPELINE_OUTPUT_BACKEND_ID, normalizePipelinePortName } from '../boundary';
import type { DaedalusGraph } from '../daedalusTypes';
import { HOST_BRIDGE_NODE_ID, HOST_OUTPUT_NODE_ID, UI_BOUNDARY_TYPES_KEY } from '../daedalusTypes';
import { decodeMetadataString } from './normalizeNodes';

export function buildPortMaps(graph: DaedalusGraph): {
  inputs: Array<Set<string>>;
  outputs: Array<Set<string>>;
} {
  const inputSets = graph.nodes.map(() => new Set<string>());
  const outputSets = graph.nodes.map(() => new Set<string>());

  graph.nodes.forEach((node, index) => {
    (node.inputs ?? []).forEach((port) => inputSets[index]?.add(port));
    (node.outputs ?? []).forEach((port) => outputSets[index]?.add(port));
  });

  graph.edges.forEach((edge) => {
    const fromNode = edge?.from?.node;
    const toNode = edge?.to?.node;
    const fromPort = edge?.from?.port;
    const toPort = edge?.to?.port;
    if (typeof fromNode === 'number' && typeof fromPort === 'string') {
      outputSets[fromNode]?.add(fromPort);
    }
    if (typeof toNode === 'number' && typeof toPort === 'string') {
      inputSets[toNode]?.add(toPort);
    }
  });

  return { inputs: inputSets, outputs: outputSets };
}

export const parseHostBridgeTypes = (value: unknown): Record<string, string> | null => {
  const raw = decodeMetadataString(value);
  if (!raw) return null;
  const entries = raw
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
  if (entries.length === 0) return null;
  const out: Record<string, string> = {};
  for (const entry of entries) {
    const sep = entry.indexOf(':');
    if (sep <= 0) continue;
    const name = entry.slice(0, sep).trim();
    const type = entry.slice(sep + 1).trim();
    if (!name || !type) continue;
    out[name] = type;
  }
  return Object.keys(out).length > 0 ? out : null;
};

export const parseHostBridgeDisplayMap = (value: unknown): Record<string, string> | null => {
  const raw = decodeMetadataString(value);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return null;
    const entries = Object.entries(parsed as Record<string, unknown>)
      .map(([key, val]) => {
        const port = normalizePipelinePortName(key);
        if (!port) return null;
        const label = typeof val === 'string' ? val.trim() : '';
        if (!label) return null;
        return [port, label] as const;
      })
      .filter((entry): entry is [string, string] => Boolean(entry));
    return entries.length ? Object.fromEntries(entries) : null;
  } catch {
    return null;
  }
};

export const mergeHostBridgeTypeMaps = (
  primary: Record<string, string> | null,
  fallback: Record<string, string> | null
): Record<string, string> | null => {
  if (!primary) return fallback;
  if (!fallback) return primary;
  return { ...fallback, ...primary };
};

export const parseBoundaryTypesMetadata = (
  metadata: Record<string, unknown> | null | undefined
): {
  inputs: Record<string, PipelineDataType>;
  outputs: Record<string, PipelineDataType>;
} | null => {
  const raw = decodeMetadataString(metadata?.[UI_BOUNDARY_TYPES_KEY]);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    const inputsRaw = parsed?.inputs;
    const outputsRaw = parsed?.outputs;
    const normalize = (record: unknown): Record<string, PipelineDataType> => {
      if (!record || typeof record !== 'object') return {};
      const entries = Object.entries(record as Record<string, unknown>)
        .map(([key, value]) => {
          const port = normalizePipelinePortName(key);
          if (!port) return null;
          if (typeof value === 'string' && value.trim()) {
            return [port, value.trim()] as const;
          }
          return null;
        })
        .filter((entry): entry is [string, string] => Boolean(entry));
      return Object.fromEntries(entries) as Record<string, PipelineDataType>;
    };
    return {
      inputs: normalize(inputsRaw),
      outputs: normalize(outputsRaw)
    };
  } catch {
    return null;
  }
};

const resolveHostBridgeTypeLabel = (dataType: PipelineDataType | null | undefined): string => {
  if (!dataType) return 'Generic';
  if (typeof dataType === 'string') {
    const trimmed = dataType.trim();
    if (!trimmed.length) return 'Generic';
    const imageMatch = trimmed.match(/^image\s*\(([^)]+)\)\s*$/iu);
    if (imageMatch?.[1]) {
      return `image:${imageMatch[1].trim().toLowerCase()}`;
    }
    return trimmed;
  }
  const labelCandidate =
    (typeof dataType.label === 'string' ? dataType.label.trim() : '') ||
    (typeof (dataType.descriptor as { label?: string } | undefined)?.label === 'string'
      ? (dataType.descriptor as { label?: string }).label!.trim()
      : '');
  if (labelCandidate) {
    const imageMatch = labelCandidate.match(/^image\s*\(([^)]+)\)\s*$/iu);
    if (imageMatch?.[1]) {
      return `image:${imageMatch[1].trim().toLowerCase()}`;
    }
    if (labelCandidate.toLowerCase() === 'image') {
      return 'image';
    }
  }
  const descriptorId = (dataType.descriptor as { id?: string } | undefined)?.id;
  if (typeof descriptorId === 'string' && descriptorId.trim()) {
    return descriptorId.trim();
  }
  const kind = typeof dataType.kind === 'string' ? dataType.kind.trim() : '';
  const format = typeof dataType.format === 'string' ? dataType.format.trim() : '';
  if (kind && format) {
    return kind.includes(':') ? kind : `${kind}:${format}`;
  }
  if (kind) {
    return kind;
  }
  const descriptorKind = (dataType.descriptor as { kind?: string } | undefined)?.kind;
  if (typeof descriptorKind === 'string' && descriptorKind.trim()) {
    return descriptorKind.trim();
  }
  return 'Generic';
};

export const serializeHostBridgeTypes = (ports: Record<string, PipelineDataType> | undefined): string | null => {
  if (!ports) return null;
  const entries = Object.entries(ports)
    .map(([name, dataType]) => {
      const normalized = name.trim();
      if (!normalized) return null;
      const typeLabel = resolveHostBridgeTypeLabel(dataType);
      return `${normalized}:${typeLabel}`;
    })
    .filter((entry): entry is string => Boolean(entry));
  return entries.length > 0 ? entries.join(',') : null;
};

export function buildBoundaryOverridesFromGraph(
  nodes: Record<string, PipelineGraphNode>,
  graphMetadata: Record<string, string> | null | undefined
): { inputs: Record<string, PipelineDataType>; outputs: Record<string, PipelineDataType> } {
  const boundaryInputs: Record<string, PipelineDataType> = {};
  const boundaryOutputs: Record<string, PipelineDataType> = {};
  Object.values(nodes).forEach((node) => {
    const backendId = (node.backendId ?? '').toLowerCase();
    const isHostBridge =
      backendId === HOST_BRIDGE_NODE_ID || backendId.endsWith(`:${HOST_BRIDGE_NODE_ID}`) || backendId === PIPELINE_INPUT_BACKEND_ID;
    const isHostOutput =
      backendId === HOST_OUTPUT_NODE_ID || backendId.endsWith(`:${HOST_OUTPUT_NODE_ID}`) || backendId === PIPELINE_OUTPUT_BACKEND_ID;
    if (isHostBridge) {
      Object.entries(node.outputs ?? {}).forEach(([port, dataType]) => {
        const normalized = normalizePipelinePortName(port);
        if (normalized) {
          boundaryInputs[normalized] = dataType;
        }
      });
    }
    if (isHostOutput) {
      Object.entries(node.inputs ?? {}).forEach(([port, dataType]) => {
        const normalized = normalizePipelinePortName(port);
        if (normalized) {
          boundaryOutputs[normalized] = dataType;
        }
      });
    }
  });
  const boundaryOverrides = parseBoundaryTypesMetadata(graphMetadata ?? null);
  if (boundaryOverrides) {
    Object.assign(boundaryInputs, boundaryOverrides.inputs);
    Object.assign(boundaryOutputs, boundaryOverrides.outputs);
  }
  return { inputs: boundaryInputs, outputs: boundaryOutputs };
}

export function buildHostBridgeMetadata(
  node: PipelineGraphNode,
  plan: PipelineGraphPlan,
  isBoundaryInput: boolean,
  isBoundaryOutput: boolean
): { hostBridgeInputs: Record<string, PipelineDataType>; hostBridgeOutputs: Record<string, PipelineDataType> } {
  const mergePortTypes = (
    record: Record<string, PipelineDataType> | null | undefined,
    overrides: Record<string, PipelineDataType> | null | undefined
  ): Record<string, PipelineDataType> => {
    const out: Record<string, PipelineDataType> = {};
    Object.entries(record ?? {}).forEach(([name, dataType]) => {
      const normalized = normalizePipelinePortName(name);
      const override = normalized ? overrides?.[normalized] : undefined;
      out[name] = override ?? dataType ?? 'Generic';
    });
    Object.entries(overrides ?? {}).forEach(([name, dataType]) => {
      const normalized = normalizePipelinePortName(name);
      if (!normalized) return;
      const existing = Object.keys(out).find((key) => normalizePipelinePortName(key) === normalized);
      if (!existing) {
        out[normalized] = dataType;
      }
    });
    return out;
  };

  let hostBridgeInputs = node?.inputs ?? {};
  let hostBridgeOutputs = node?.outputs ?? {};
  if (isBoundaryInput) {
    hostBridgeOutputs = mergePortTypes(node?.outputs ?? {}, plan.pipelineInputs ?? {});
  }
  if (isBoundaryOutput) {
    hostBridgeInputs = mergePortTypes(node?.inputs ?? {}, plan.pipelineOutputs ?? {});
  }
  return { hostBridgeInputs, hostBridgeOutputs };
}

export function encodeBoundaryTypesMetadata(
  metadata: Record<string, string>,
  plan: PipelineGraphPlan
) {
  const boundaryTypes: Record<string, unknown> = {};
  if (plan.pipelineInputs && Object.keys(plan.pipelineInputs).length > 0) {
    boundaryTypes.inputs = plan.pipelineInputs;
  }
  if (plan.pipelineOutputs && Object.keys(plan.pipelineOutputs).length > 0) {
    boundaryTypes.outputs = plan.pipelineOutputs;
  }
  if (Object.keys(boundaryTypes).length > 0) {
    metadata[UI_BOUNDARY_TYPES_KEY] = JSON.stringify(boundaryTypes);
  } else {
    delete metadata[UI_BOUNDARY_TYPES_KEY];
  }
}
