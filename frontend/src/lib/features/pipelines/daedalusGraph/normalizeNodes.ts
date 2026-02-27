import type { PipelineGraphNode, PipelineGraphPlan, PipelinePortMetadata } from '$lib/types/pipeline';
import { PIPELINE_INPUT_BACKEND_ID, PIPELINE_OUTPUT_BACKEND_ID } from '../boundary';
import type { DaedalusGraph, DaedalusNodeInstance, DaedalusValue } from '../daedalusTypes';
import { HOST_BRIDGE_NODE_ID, HOST_OUTPUT_NODE_ID, UI_NESTED_PLAN_KEY, UI_NODE_ID_KEY } from '../daedalusTypes';
import { encodeFloatValue, extractPortMetadata, isRecord } from './valueCodec';

const UI_X_KEYS = ['helios.ui.x', 'ui.x', 'x'];
const UI_Y_KEYS = ['helios.ui.y', 'ui.y', 'y'];
export const UI_ORDER_KEY = 'helios.ui.node_order';
export const EMBEDDED_GRAPH_KEY = 'daedalus.embedded_graph';

export function sanitizeDaedalusMetadata(metadata: Record<string, string> | null | undefined): Record<string, string> {
  const out: Record<string, string> = { ...(metadata ?? {}) };
  delete out[UI_NESTED_PLAN_KEY];
  return out;
}

export function deepCloneJson<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

export function hasEmbeddedGroups(plan: PipelineGraphPlan | null | undefined): boolean {
  if (!plan?.nodes) return false;
  for (const node of Object.values(plan.nodes)) {
    if (!node) continue;
    if (typeof node.backendId === 'string' && node.backendId.trim().toLowerCase() === 'pipeline:child' && node.embedded) {
      return true;
    }
    if (node.embedded && hasEmbeddedGroups(node.embedded)) {
      return true;
    }
  }
  return false;
}

export function decodeNestedPlan(graph: DaedalusGraph): PipelineGraphPlan | null {
  const raw = graph.metadata?.[UI_NESTED_PLAN_KEY];
  if (typeof raw !== 'string' || !raw.trim()) {
    return null;
  }
  try {
    const parsed = JSON.parse(raw) as unknown;
    const payload = isRecord(parsed) && parsed.version === 1 ? (parsed as Record<string, unknown>).plan : parsed;
    if (!isRecord(payload)) return null;
    const nodes = isRecord(payload.nodes) ? (payload.nodes as Record<string, unknown>) : null;
    const connections = Array.isArray(payload.connections) ? payload.connections : null;
    if (!nodes || !connections) return null;

    const normalizedMeta = sanitizeDaedalusMetadata(graph.metadata ?? {});
    return {
      ...(payload as unknown as PipelineGraphPlan),
      format: 'daedalus',
      daedalus: { metadata: normalizedMeta },
      nodes: nodes as unknown as PipelineGraphPlan['nodes'],
      connections: connections as PipelineGraphPlan['connections']
    };
  } catch {
    return null;
  }
}

export const decodeMetadataString = (value: unknown): string | null => {
  if (typeof value === 'string') return value;
  if (!value || typeof value !== 'object') return null;
  const record = value as Record<string, unknown>;
  if (record.type === 'String' && typeof record.value === 'string') {
    return record.value;
  }
  return null;
};

const decodeNumber = (value: unknown): number | null => {
  if (!value || typeof value !== 'object') return null;
  const record = value as Record<string, unknown>;
  const type = record.type;
  if (type !== 'Int' && type !== 'Float') return null;
  const numeric = typeof record.value === 'number' ? record.value : Number(record.value);
  return Number.isFinite(numeric) ? numeric : null;
};

export function resolveNodeLocation(
  node: DaedalusNodeInstance | null | undefined,
  index: number
): { x: number; y: number } {
  const metadata = node?.metadata ?? {};
  const pick = (keys: string[]) => {
    for (const key of keys) {
      const decoded = decodeNumber((metadata as Record<string, unknown>)[key]);
      if (decoded != null) return decoded;
    }
    return null;
  };
  const x = pick(UI_X_KEYS);
  const y = pick(UI_Y_KEYS);
  if (x != null && y != null) {
    return { x, y };
  }
  const spacingX = 280;
  const spacingY = 220;
  return { x: (index % 6) * spacingX, y: Math.floor(index / 6) * spacingY };
}

export function normalizeName(node: DaedalusNodeInstance): string {
  const label = typeof node.label === 'string' ? node.label.trim() : '';
  if (label) return label;
  const id = typeof node.id === 'string' ? node.id.trim() : '';
  return id || 'Daedalus node';
}

export function isHostBridgeNode(node: DaedalusNodeInstance): boolean {
  const id = typeof node?.id === 'string' ? node.id.trim().toLowerCase() : '';
  if (!id) return false;
  if (id === HOST_BRIDGE_NODE_ID || id === HOST_OUTPUT_NODE_ID) return true;
  return id.endsWith(`:${HOST_BRIDGE_NODE_ID}`) || id.endsWith(`:${HOST_OUTPUT_NODE_ID}`);
}

export const isHostBridgeIdOnly = (raw: string | null | undefined): boolean => {
  if (!raw) return false;
  const lowered = raw.trim().toLowerCase();
  return lowered === HOST_BRIDGE_NODE_ID || lowered.endsWith(`:${HOST_BRIDGE_NODE_ID}`);
};

export const isHostOutputIdOnly = (raw: string | null | undefined): boolean => {
  if (!raw) return false;
  const lowered = raw.trim().toLowerCase();
  return lowered === HOST_OUTPUT_NODE_ID || lowered.endsWith(`:${HOST_OUTPUT_NODE_ID}`);
};

export function isHostBridgeId(id: string | null | undefined): boolean {
  if (!id) return false;
  return isHostBridgeNode({
    id,
    inputs: [],
    outputs: []
  });
}

export function resolveNodeOrder(plan: PipelineGraphPlan): string[] {
  const declared = plan.daedalus?.metadata?.[UI_ORDER_KEY];
  if (typeof declared === 'string' && declared.trim()) {
    try {
      const parsed = JSON.parse(declared);
      if (Array.isArray(parsed) && parsed.every((entry) => typeof entry === 'string')) {
        const unique = parsed.filter((id, index, array) => array.indexOf(id) === index);
        if (unique.length > 0) {
          const existing = new Set(Object.keys(plan.nodes ?? {}));
          const filtered = unique.filter((id) => existing.has(id));
          const remainder = Object.keys(plan.nodes ?? {}).filter((id) => !filtered.includes(id));
          return [...filtered, ...remainder];
        }
      }
    } catch {
      // ignore invalid metadata
    }
  }

  const ids = Object.keys(plan.nodes ?? {});
  const toNum = (id: string) => {
    const value = Number.parseInt(id, 10);
    return Number.isFinite(value) ? value : null;
  };
  return ids.sort((a, b) => {
    const an = toNum(a);
    const bn = toNum(b);
    if (an != null && bn != null) return an - bn;
    if (an != null) return -1;
    if (bn != null) return 1;
    return a.localeCompare(b);
  });
}

export function cloneNodeInstance(value: unknown): DaedalusNodeInstance | null {
  if (!isRecord(value)) return null;
  const id = typeof value.id === 'string' ? value.id : null;
  if (!id) return null;
  const inputs = Array.isArray(value.inputs) ? value.inputs.filter((p): p is string => typeof p === 'string') : [];
  const outputs = Array.isArray(value.outputs) ? value.outputs.filter((p): p is string => typeof p === 'string') : [];
  return {
    ...(value as unknown as DaedalusNodeInstance),
    id,
    inputs,
    outputs
  };
}

export function readBoundaryPort(node: PipelineGraphNode, direction: 'input' | 'output'): string | null {
  const record = direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
  const port = Object.keys(record)[0] ?? null;
  return typeof port === 'string' && port.trim() ? port : null;
}

type FlattenResult = {
  plan: PipelineGraphPlan;
  hadGroups: boolean;
};

export function flattenPipelineChildGroups(plan: PipelineGraphPlan): FlattenResult {
  const base: PipelineGraphPlan = {
    ...(plan.format ? { format: plan.format } : {}),
    ...(plan.daedalus ? { daedalus: { ...(plan.daedalus ?? {}) } } : {}),
    nodes: { ...(plan.nodes ?? {}) },
    connections: Array.isArray(plan.connections) ? plan.connections.map((c) => ({ ...c, from: { ...c.from }, to: { ...c.to }, ...(c.style ? { style: { ...c.style } } : {}) })) : [],
    pipelineInputs: plan.pipelineInputs ? { ...plan.pipelineInputs } : undefined,
    pipelineOutputs: plan.pipelineOutputs ? { ...plan.pipelineOutputs } : undefined,
    pipelineInputValues: plan.pipelineInputValues ? deepCloneJson(plan.pipelineInputValues) : undefined,
    nodeValueOverrides: plan.nodeValueOverrides ? deepCloneJson(plan.nodeValueOverrides) : undefined,
    pipelineInputConfigs: plan.pipelineInputConfigs ? { ...plan.pipelineInputConfigs } : undefined,
    pipelineOutputConfigs: plan.pipelineOutputConfigs ? { ...plan.pipelineOutputConfigs } : undefined
  };

  let hadGroups = false;
  let changed = true;
  while (changed) {
    changed = false;
    for (const [groupId, node] of Object.entries(base.nodes ?? {})) {
      if (!node || typeof node.backendId !== 'string' || node.backendId.trim().toLowerCase() !== 'pipeline:child') {
        continue;
      }
      if (!node.embedded) {
        continue;
      }

      const embeddedFlattened = flattenPipelineChildGroups(node.embedded);
      hadGroups = true;

      const embedded = embeddedFlattened.plan;
      const embeddedNodes = embedded.nodes ?? {};
      const embeddedConnections = embedded.connections ?? [];

      const boundaryInputIds = new Set<string>();
      const boundaryOutputIds = new Set<string>();
      const boundaryInputPortById = new Map<string, string>();
      const boundaryOutputPortById = new Map<string, string>();

      for (const [childId, childNode] of Object.entries(embeddedNodes)) {
        const backendId = typeof childNode?.backendId === 'string' ? childNode.backendId.trim().toLowerCase() : '';
        if (backendId === PIPELINE_INPUT_BACKEND_ID) {
          boundaryInputIds.add(childId);
          const port = readBoundaryPort(childNode, 'input');
          if (port) boundaryInputPortById.set(childId, port);
        } else if (backendId === PIPELINE_OUTPUT_BACKEND_ID) {
          boundaryOutputIds.add(childId);
          const port = readBoundaryPort(childNode, 'output');
          if (port) boundaryOutputPortById.set(childId, port);
        }
      }

      const boundaryInputTargets: Record<
        string,
        Array<{ node: string; port: string; policy?: PipelineGraphPlan['connections'][number]['policy'] }>
      > = {};
      const boundaryOutputSources: Record<
        string,
        Array<{ node: string; port: string; policy?: PipelineGraphPlan['connections'][number]['policy'] }>
      > = {};
      const internalConnections: PipelineGraphPlan['connections'] = [];

      for (const conn of embeddedConnections) {
        const fromId = conn.from.node;
        const toId = conn.to.node;
        if (boundaryInputIds.has(fromId)) {
          const port = boundaryInputPortById.get(fromId) ?? conn.from.port;
          (boundaryInputTargets[port] ??= []).push({ node: toId, port: conn.to.port, policy: conn.policy });
          continue;
        }
        if (boundaryOutputIds.has(toId)) {
          const port = boundaryOutputPortById.get(toId) ?? conn.to.port;
          (boundaryOutputSources[port] ??= []).push({ node: fromId, port: conn.from.port, policy: conn.policy });
          continue;
        }
        if (boundaryInputIds.has(toId) || boundaryOutputIds.has(fromId)) {
          continue;
        }
        internalConnections.push(conn);
      }

      const remainingConnections = (base.connections ?? []).filter((conn) => conn.from.node !== groupId && conn.to.node !== groupId);
      const incomingParent = (base.connections ?? []).filter((conn) => conn.to.node === groupId);
      const outgoingParent = (base.connections ?? []).filter((conn) => conn.from.node === groupId);

      const rewired: PipelineGraphPlan['connections'] = [...remainingConnections, ...internalConnections];

      for (const incoming of incomingParent) {
        const targets = boundaryInputTargets[incoming.to.port] ?? [];
        for (const target of targets) {
          rewired.push({
            from: { ...incoming.from },
            to: { node: target.node, port: target.port },
            policy: incoming.policy ?? target.policy
          });
        }
      }
      for (const outgoing of outgoingParent) {
        const sources = boundaryOutputSources[outgoing.from.port] ?? [];
        for (const source of sources) {
          rewired.push({
            from: { node: source.node, port: source.port },
            to: { ...outgoing.to },
            policy: outgoing.policy ?? source.policy
          });
        }
      }

      const nextNodes: Record<string, PipelineGraphNode> = { ...(base.nodes ?? {}) };
      delete nextNodes[groupId];
      for (const [childId, childNode] of Object.entries(embeddedNodes)) {
        if (boundaryInputIds.has(childId) || boundaryOutputIds.has(childId)) {
          continue;
        }
        nextNodes[childId] = childNode;
      }

      base.nodes = nextNodes;
      base.connections = rewired;
      changed = true;
      break;
    }
  }

  return { plan: base, hadGroups };
}

export function isDaedalusGraph(value: unknown): value is DaedalusGraph {
  if (!isRecord(value)) return false;
  if (!Array.isArray(value.nodes) || !Array.isArray(value.edges)) return false;
  return true;
}

export function extractPortMetadataFromNode(
  node: DaedalusNodeInstance
): { inputs: Record<string, PipelinePortMetadata>; outputs: Record<string, PipelinePortMetadata> } {
  return extractPortMetadata(node.metadata ?? null);
}

export function appendLocationMetadata(
  metadata: Record<string, DaedalusValue>,
  location: { x: number; y: number } | null | undefined
) {
  if (!location) return;
  if (typeof location.x === 'number' && typeof location.y === 'number') {
    metadata['helios.ui.x'] = encodeFloatValue(location.x);
    metadata['helios.ui.y'] = encodeFloatValue(location.y);
  }
}

export function appendNodeIdMetadata(metadata: Record<string, DaedalusValue>, nodeId: string) {
  metadata[UI_NODE_ID_KEY] = { type: 'String', value: nodeId };
}

export function appendDocMetadata(metadata: Record<string, DaedalusValue>, docSlug: string | null) {
  if (docSlug) {
    metadata['doc'] = { type: 'String', value: docSlug };
  }
}

export function appendHostBridgeMetadata(metadata: Record<string, DaedalusValue>, backendId: string) {
  if (isHostBridgeId(backendId)) {
    metadata['host_bridge'] = { type: 'Bool', value: true };
  }
}

export function createEmptyDaedalusNode(): DaedalusNodeInstance {
  return {
    id: 'unknown',
    bundle: null,
    label: null,
    inputs: [],
    outputs: [],
    const_inputs: [],
    sync_groups: [],
    metadata: {}
  };
}
