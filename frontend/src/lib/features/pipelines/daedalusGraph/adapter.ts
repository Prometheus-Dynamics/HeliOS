import type { PipelineDataType, PipelineGraphNode, PipelineGraphPlan, PipelineNodeValue } from '$lib/types/pipeline';
import { PIPELINE_INPUT_BACKEND_ID, PIPELINE_OUTPUT_BACKEND_ID } from '../boundary';
import type { DaedalusEdge, DaedalusGraph, DaedalusNodeInstance, DaedalusValue } from '../daedalusTypes';
import {
  HOST_BRIDGE_NODE_ID,
  HOST_OUTPUT_NODE_ID,
  UI_BOUNDARY_TYPES_KEY,
  UI_EDGE_STYLE_KEY,
  UI_EDGE_STYLES_KEY,
  UI_NESTED_PLAN_KEY,
} from '../daedalusTypes';
import {
  decodeDaedalusValue,
  decodeString,
  encodeConstInputs,
  extractPortMetadata,
  mapEnumIndexToValue,
  dataTypeFromDaedalusValue,
  resolvePortEntry
} from './valueCodec';
import {
  appendDocMetadata,
  appendHostBridgeMetadata,
  appendLocationMetadata,
  appendNodeIdMetadata,
  buildBoundaryOverridesFromGraph,
  buildHostBridgeMetadata,
  buildPortMaps,
  cloneNodeInstance,
  decodeEdgeStyleForConnection,
  decodeEdgeStyles,
  decodeMetadataString,
  decodeNestedPlan,
  EMBEDDED_GRAPH_KEY,
  encodeEdgeStyle,
  flattenPipelineChildGroups,
  hasEmbeddedGroups,
  isDaedalusGraph,
  isHostBridgeId,
  isHostBridgeIdOnly,
  isHostOutputIdOnly,
  isHostBridgeNode,
  mergeHostBridgeTypeMaps,
  normalizeName,
  parseHostBridgeDisplayMap,
  parseHostBridgeTypes,
  resolveNodeLocation,
  resolveNodeOrder,
  sanitizeDaedalusMetadata,
  serializeHostBridgeTypes,
  UI_ORDER_KEY
} from './graphNormalization';

export { isDaedalusGraph } from './graphNormalization';

const decodeEmbeddedGraph = (node: DaedalusNodeInstance): PipelineGraphPlan | null => {
  const raw = decodeMetadataString(node.metadata?.[EMBEDDED_GRAPH_KEY]);
  if (!raw || !raw.trim()) {
    return null;
  }
  try {
    const parsed = JSON.parse(raw);
    if (!isDaedalusGraph(parsed)) return null;
    return fromDaedalusGraph(parsed);
  } catch {
    return null;
  }
};

export function fromDaedalusGraph(graph: DaedalusGraph): PipelineGraphPlan {
  const nested = decodeNestedPlan(graph);
  if (nested) {
    return nested;
  }
  const ports = buildPortMaps(graph);
  const hasHostOutput = graph.nodes.some((node) => isHostOutputIdOnly(node?.id ?? null));

  const nodes: Record<string, PipelineGraphNode> = {};
  const splitHostOutputIds = new Map<number, string>();
  graph.nodes.forEach((node, index) => {
    const id = String(index);
    const location = resolveNodeLocation(node, index);
    const inputPorts = Array.from(ports.inputs[index] ?? []);
    const outputPorts = Array.from(ports.outputs[index] ?? []);
    const isBridge = isHostBridgeNode(node);
    const isHostBridge = isHostBridgeIdOnly(node.id);
    const shouldSplitHostBridge =
      isHostBridge && !hasHostOutput && inputPorts.length > 0 && outputPorts.length > 0;
    const hostBridgeInputs = isBridge
      ? mergeHostBridgeTypeMaps(
          parseHostBridgeDisplayMap(node.metadata?.host_bridge_inputs_display),
          parseHostBridgeTypes(node.metadata?.host_bridge_inputs)
        )
      : null;
    const hostBridgeOutputs = isBridge
      ? mergeHostBridgeTypeMaps(
          parseHostBridgeDisplayMap(node.metadata?.host_bridge_outputs_display),
          parseHostBridgeTypes(node.metadata?.host_bridge_outputs)
        )
      : null;
    const backendId = (() => {
      const raw = node.id;
      const lowered = raw.trim().toLowerCase();
      if (lowered === HOST_BRIDGE_NODE_ID || lowered.endsWith(`:${HOST_BRIDGE_NODE_ID}`)) {
        return PIPELINE_INPUT_BACKEND_ID;
      }
      if (lowered === HOST_OUTPUT_NODE_ID || lowered.endsWith(`:${HOST_OUTPUT_NODE_ID}`)) {
        return PIPELINE_OUTPUT_BACKEND_ID;
      }
      return raw;
    })();
    const docSlug = decodeString(node.metadata?.doc);
    const portMetadata = extractPortMetadata(node.metadata ?? null);
    const inputPortMeta = Object.keys(portMetadata.inputs).length > 0 ? { inputPorts: portMetadata.inputs } : {};
    const outputPortMeta = Object.keys(portMetadata.outputs).length > 0 ? { outputPorts: portMetadata.outputs } : {};
    const constValues = Array.isArray(node.const_inputs)
      ? Object.fromEntries(
          node.const_inputs
            .filter((entry): entry is [string, DaedalusValue] => Array.isArray(entry) && typeof entry[0] === 'string' && Boolean(entry[0]))
            .map(([key, value]) => {
              const decoded = decodeDaedalusValue(value);
              const portMeta = resolvePortEntry(portMetadata.inputs, key);
              const mapped = mapEnumIndexToValue(decoded, portMeta);
              return [
                key,
                {
                  dataType: mapped ? 'enum' : dataTypeFromDaedalusValue(value),
                  value: mapped ?? decoded
                } satisfies PipelineNodeValue
              ];
            })
        )
      : {};

    const embedded = decodeEmbeddedGraph(node);
    if (shouldSplitHostBridge) {
      const outputNodeIdBase = `${id}:host_output`;
      let outputNodeId = outputNodeIdBase;
      let suffix = 1;
      while (nodes[outputNodeId]) {
        outputNodeId = `${outputNodeIdBase}:${suffix}`;
        suffix += 1;
      }
      splitHostOutputIds.set(index, outputNodeId);

      const outputNodeLocation = {
        x: location.x + 280,
        y: location.y
      };

      const inputSource = JSON.parse(JSON.stringify(node)) as unknown as any;
      if (inputSource && typeof inputSource === 'object') {
        inputSource.inputs = [];
      }
      const outputSource = JSON.parse(JSON.stringify(node)) as unknown as any;
      if (outputSource && typeof outputSource === 'object') {
        outputSource.outputs = [];
      }

      nodes[id] = {
        id,
        backendId: PIPELINE_INPUT_BACKEND_ID,
        metadata: { name: 'Pipeline Input', ...(docSlug ? { doc: docSlug } : {}), ...outputPortMeta },
        inputs: {},
        outputs: Object.fromEntries(
          outputPorts.map((port) => [port, hostBridgeOutputs?.[port] ?? 'Generic'] as const)
        ),
        info: { id, location, values: constValues },
        ...(embedded ? { embedded } : {}),
        source: inputSource
      };

      nodes[outputNodeId] = {
        id: outputNodeId,
        backendId: PIPELINE_OUTPUT_BACKEND_ID,
        metadata: { name: 'Pipeline Output', ...(docSlug ? { doc: docSlug } : {}), ...inputPortMeta },
        inputs: Object.fromEntries(
          inputPorts.map((port) => [port, hostBridgeInputs?.[port] ?? 'Generic'] as const)
        ),
        outputs: {},
        info: { id: outputNodeId, location: outputNodeLocation },
        embedded: null,
        source: outputSource
      };
      return;
    }

    const displayName = (() => {
      const rawName = normalizeName(node);
      const lowered = rawName.trim().toLowerCase();
      if (backendId === PIPELINE_INPUT_BACKEND_ID) {
        if (lowered === HOST_BRIDGE_NODE_ID || lowered.endsWith(`:${HOST_BRIDGE_NODE_ID}`)) {
          return 'Pipeline Input';
        }
      }
      if (backendId === PIPELINE_OUTPUT_BACKEND_ID) {
        if (lowered === HOST_OUTPUT_NODE_ID || lowered.endsWith(`:${HOST_OUTPUT_NODE_ID}`)) {
          return 'Pipeline Output';
        }
      }
      return rawName;
    })();

    nodes[id] = {
      id,
      backendId,
      metadata: { name: displayName, ...(docSlug ? { doc: docSlug } : {}), ...inputPortMeta, ...outputPortMeta },
      inputs: Object.fromEntries(
        inputPorts.map((port) => [port, hostBridgeInputs?.[port] ?? 'Generic'] as const)
      ),
      outputs: Object.fromEntries(
        outputPorts.map((port) => [port, hostBridgeOutputs?.[port] ?? 'Generic'] as const)
      ),
      info: { id, location, values: constValues },
      ...(embedded ? { embedded } : {}),
      source: JSON.parse(JSON.stringify(node)) as unknown as any
    };
  });

  const edgeStyles = decodeEdgeStyles(graph.metadata ?? null);
  let connections = graph.edges
    .map((edge) => {
      const fromNode = edge?.from?.node;
      const toNode = edge?.to?.node;
      const fromPort = edge?.from?.port;
      const toPort = edge?.to?.port;
      if (typeof fromNode !== 'number' || typeof toNode !== 'number') return null;
      if (typeof fromPort !== 'string' || typeof toPort !== 'string') return null;
      const mappedToNode = splitHostOutputIds.get(toNode);
      const base = {
        from: { node: String(fromNode), port: fromPort },
        to: { node: mappedToNode ?? String(toNode), port: toPort }
      };
      const style = decodeEdgeStyleForConnection(edge, edgeStyles);
      return style ? { ...base, style } : base;
    })
    .filter((entry): entry is NonNullable<(typeof connections)[number]> => Boolean(entry));

  const boundaryOverrides = buildBoundaryOverridesFromGraph(nodes, graph.metadata ?? null);

  return {
    format: 'daedalus',
    daedalus: { metadata: sanitizeDaedalusMetadata(graph.metadata ?? {}) },
    nodes,
    connections,
    pipelineInputs: boundaryOverrides.inputs,
    pipelineOutputs: boundaryOverrides.outputs,
    pipelineInputValues: {},
    nodeValueOverrides: {},
    pipelineInputConfigs: {},
    pipelineOutputConfigs: {}
  };
}

export function toDaedalusGraph(plan: PipelineGraphPlan): DaedalusGraph {
  const nestingEnabled = hasEmbeddedGroups(plan);
  const nestedPayload = nestingEnabled ? JSON.parse(JSON.stringify(plan)) : null;
  if (nestedPayload?.daedalus?.metadata) {
    delete (nestedPayload.daedalus.metadata as Record<string, string>)[UI_NESTED_PLAN_KEY];
    delete (nestedPayload.daedalus.metadata as Record<string, string>)[UI_BOUNDARY_TYPES_KEY];
  }

  const flattened = flattenPipelineChildGroups(plan).plan;

  const nodeOrder = resolveNodeOrder(flattened);
  const indexById = new Map(nodeOrder.map((id, index) => [id, index]));

  const connectionInputs = new Map<string, Set<string>>();
  const connectionOutputs = new Map<string, Set<string>>();
  for (const connection of flattened.connections ?? []) {
    const fromId = connection?.from?.node;
    const toId = connection?.to?.node;
    const fromPort = connection?.from?.port;
    const toPort = connection?.to?.port;
    if (typeof fromId === 'string' && typeof fromPort === 'string') {
      (connectionOutputs.get(fromId) ?? connectionOutputs.set(fromId, new Set()).get(fromId))?.add(fromPort);
    }
    if (typeof toId === 'string' && typeof toPort === 'string') {
      (connectionInputs.get(toId) ?? connectionInputs.set(toId, new Set()).get(toId))?.add(toPort);
    }
  }

  const nodes: DaedalusNodeInstance[] = nodeOrder.map((nodeId) => {
    const node = flattened.nodes[nodeId];
    const base = cloneNodeInstance(node?.source) ?? {
      id: typeof node?.backendId === 'string' ? node.backendId : 'unknown',
      bundle: null,
      label: null,
      inputs: [],
      outputs: [],
      const_inputs: [],
      sync_groups: [],
      metadata: {}
    };
    const { compute: _ignoredCompute, ...baseWithoutCompute } = base;

    const labelCandidate = typeof node?.metadata?.name === 'string' ? node.metadata.name.trim() : '';
    const backendIdRaw = typeof node?.backendId === 'string' && node.backendId.trim() ? node.backendId.trim() : base.id;
    const backendIdLower = backendIdRaw.toLowerCase();
    const isBoundaryInput = backendIdLower === PIPELINE_INPUT_BACKEND_ID;
    const isBoundaryOutput = backendIdLower === PIPELINE_OUTPUT_BACKEND_ID;
    const isPipelineChild = backendIdLower === 'pipeline:child';
    const backendId = isBoundaryInput
      ? HOST_BRIDGE_NODE_ID
      : isBoundaryOutput
        ? HOST_OUTPUT_NODE_ID
        : backendIdRaw;

    const boundaryLabel = isBoundaryInput ? 'Input' : 'Output';

    const label =
      isBoundaryInput || isBoundaryOutput
        ? boundaryLabel
        : labelCandidate && labelCandidate !== backendIdRaw
          ? labelCandidate
          : base.label ?? null;

    const inputPorts = new Set<string>([
      ...Object.keys(node?.inputs ?? {}),
      ...Array.from(connectionInputs.get(nodeId) ?? []),
      ...Object.keys(node?.info?.values ?? {})
    ]);
    const outputPorts = new Set<string>([
      ...Object.keys(node?.outputs ?? {}),
      ...Array.from(connectionOutputs.get(nodeId) ?? [])
    ]);

    const metadata = { ...(base.metadata ?? {}) };
    const docSlug = typeof node?.metadata?.doc === 'string' && node.metadata.doc.trim() ? node.metadata.doc.trim() : null;
    appendLocationMetadata(metadata, node?.info?.location);
    appendNodeIdMetadata(metadata, nodeId);
    appendDocMetadata(metadata, docSlug);
    appendHostBridgeMetadata(metadata, backendId);

    if (isHostBridgeId(backendId)) {
      const { hostBridgeInputs, hostBridgeOutputs } = buildHostBridgeMetadata(node, plan, isBoundaryInput, isBoundaryOutput);
      const hostBridgeInputsValue = serializeHostBridgeTypes(hostBridgeInputs);
      const hostBridgeOutputsValue = serializeHostBridgeTypes(hostBridgeOutputs);
      if (hostBridgeInputsValue) {
        metadata['host_bridge_inputs'] = { type: 'String', value: hostBridgeInputsValue };
      }
      if (hostBridgeOutputsValue) {
        metadata['host_bridge_outputs'] = { type: 'String', value: hostBridgeOutputsValue };
      }
    }

    if (node?.embedded && !isPipelineChild) {
      const embeddedGraph = toDaedalusGraph(node.embedded);
      metadata[EMBEDDED_GRAPH_KEY] = { type: 'String', value: JSON.stringify(embeddedGraph) };
    }

    return {
      ...baseWithoutCompute,
      id: backendId,
      label,
      inputs: Array.from(inputPorts).sort((a, b) => a.localeCompare(b)),
      outputs: Array.from(outputPorts).sort((a, b) => a.localeCompare(b)),
      const_inputs: encodeConstInputs(node?.info?.values, node?.inputs, node?.metadata?.inputPorts),
      metadata
    };
  });

  const edgeStyleMap: Record<string, unknown> = {};
  const edges: DaedalusEdge[] = (flattened.connections ?? [])
    .map((connection): DaedalusEdge | null => {
      const fromId = connection?.from?.node;
      const toId = connection?.to?.node;
      const fromPort = connection?.from?.port;
      const toPort = connection?.to?.port;
      if (typeof fromId !== 'string' || typeof toId !== 'string') return null;
      if (typeof fromPort !== 'string' || typeof toPort !== 'string') return null;
      const fromIndex = indexById.get(fromId);
      const toIndex = indexById.get(toId);
      if (fromIndex == null || toIndex == null) return null;
      const styleRecord =
        connection?.style && typeof connection.style === 'object'
          ? (connection.style as Record<string, unknown>)
          : null;
      const styleValue = encodeEdgeStyle(styleRecord);
      if (styleRecord) {
        edgeStyleMap[`${fromIndex}:${fromPort}->${toIndex}:${toPort}`] = styleRecord;
      }
      const metadata: Record<string, DaedalusValue> | undefined = styleValue ? { [UI_EDGE_STYLE_KEY]: styleValue } : undefined;
      return {
        from: { node: fromIndex, port: fromPort },
        to: { node: toIndex, port: toPort },
        ...(metadata ? { metadata } : {})
      };
    })
    .filter((edge): edge is DaedalusEdge => Boolean(edge));

  const metadata: Record<string, string> = { ...(flattened.daedalus?.metadata ?? {}) };
  delete metadata[UI_BOUNDARY_TYPES_KEY];
  metadata[UI_ORDER_KEY] = JSON.stringify(nodeOrder);
  if (Object.keys(edgeStyleMap).length > 0) {
    metadata[UI_EDGE_STYLES_KEY] = JSON.stringify(edgeStyleMap);
  } else {
    delete metadata[UI_EDGE_STYLES_KEY];
  }
  if (nestingEnabled && nestedPayload) {
    metadata[UI_NESTED_PLAN_KEY] = JSON.stringify({ version: 1, plan: nestedPayload });
  } else {
    delete metadata[UI_NESTED_PLAN_KEY];
  }

  return {
    nodes,
    edges,
    metadata
  };
}
