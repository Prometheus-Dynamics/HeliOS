import type { ChannelPolicy, PipelineConnection, PipelineDataType, PipelineEndpoint, PipelineGraphNode, PipelineGraphPlan } from '$lib/types/pipeline';
import { createPipelineInputNode, createPipelineOutputNode, normalizePipelinePortName } from './boundary';
import { emptyPipelineGraphPlan } from './graph';

function dataTypeForEndpoint(plan: PipelineGraphPlan, endpoint: PipelineEndpoint, direction: 'in' | 'out'): PipelineDataType {
  const node = plan.nodes?.[endpoint.node];
  if (!node) return 'generic';
  const ports = direction === 'in' ? node.inputs ?? {} : node.outputs ?? {};
  return ports[endpoint.port] ?? 'generic';
}

function allocateBoundaryPort(basePort: string, used: Set<string>): string {
  const normalized = normalizePipelinePortName(basePort);
  if (!used.has(normalized)) {
    used.add(normalized);
    return normalized;
  }
  let suffix = 2;
  while (used.has(`${normalized}__${suffix}`)) {
    suffix += 1;
  }
  const next = `${normalized}__${suffix}`;
  used.add(next);
  return next;
}

function cloneNode(node: PipelineGraphNode): PipelineGraphNode {
  if (typeof structuredClone === 'function') {
    return structuredClone(node) as PipelineGraphNode;
  }
  return JSON.parse(JSON.stringify(node)) as PipelineGraphNode;
}

export function groupSelectionInPlace(options: {
  plan: PipelineGraphPlan;
  selectedNodeIds: string[];
  primaryNodeId: string;
  generateNodeId: () => string;
}): string | null {
  const { plan, selectedNodeIds, primaryNodeId, generateNodeId } = options;
  const nodesMap = plan.nodes ?? {};
  const present = selectedNodeIds.filter((id) => nodesMap[id]);
  if (present.length === 0) return null;
  const representative = nodesMap[primaryNodeId] ?? nodesMap[present[0] ?? ''];
  if (!representative) return null;

  const selectedIdsSet = new Set(present);
  const incoming = (plan.connections ?? []).filter((conn) => selectedIdsSet.has(conn.to.node) && !selectedIdsSet.has(conn.from.node));
  const outgoing = (plan.connections ?? []).filter((conn) => selectedIdsSet.has(conn.from.node) && !selectedIdsSet.has(conn.to.node));
  const internalConnections = (plan.connections ?? []).filter((conn) => selectedIdsSet.has(conn.from.node) && selectedIdsSet.has(conn.to.node));

  const childPlan = emptyPipelineGraphPlan();
  childPlan.nodes = {};
  childPlan.connections = [];
  childPlan.pipelineInputs = {};
  childPlan.pipelineOutputs = {};
  childPlan.pipelineInputValues = {};
  childPlan.pipelineOutputConfigs = {};
  childPlan.pipelineInputConfigs = {};

  for (const id of selectedIdsSet) {
    const node = nodesMap[id];
    if (!node) continue;
    childPlan.nodes[id] = cloneNode(node);
  }
  childPlan.connections.push(...internalConnections.map((conn) => ({ ...conn })));

  const makeConnectionKey = (connection: PipelineConnection) =>
    `${connection.from.node}:${connection.from.port}->${connection.to.node}:${connection.to.port}`;

  const childSeen = new Set<string>(childPlan.connections.map(makeConnectionKey));
  const pushUniqueChild = (connection: PipelineConnection) => {
    const key = makeConnectionKey(connection);
    if (childSeen.has(key)) return;
    childSeen.add(key);
    childPlan.connections.push(connection);
  };

  const parentInputs: Record<string, PipelineDataType> = {};
  const inputBoundaryNodeIds = new Map<string, string>();
  const inputBoundaryPorts = new Map<string, string>();
  const usedInputPorts = new Set<string>();

  for (const incomingConn of incoming) {
    // Key by upstream endpoint so fan-out into the selected nodes shares a single group input.
    const portKey = `${incomingConn.from.node}:${incomingConn.from.port}`;
    const portName = inputBoundaryPorts.get(portKey) ?? allocateBoundaryPort(incomingConn.to.port, usedInputPorts);
    const dataType = dataTypeForEndpoint(plan, incomingConn.to, 'in') ?? dataTypeForEndpoint(plan, incomingConn.from, 'out');
    const existingBoundaryId = inputBoundaryNodeIds.get(portKey);
    const inputNode =
      existingBoundaryId && childPlan.nodes?.[existingBoundaryId]
        ? childPlan.nodes[existingBoundaryId]
        : createPipelineInputNode(portName, dataType);
    if (!existingBoundaryId) {
      inputBoundaryNodeIds.set(portKey, inputNode.id);
      inputBoundaryPorts.set(portKey, portName);
      childPlan.nodes[inputNode.id] = inputNode;
    }
    pushUniqueChild({
      from: { node: inputNode.id, port: portName },
      to: { node: incomingConn.to.node, port: incomingConn.to.port },
      policy: incomingConn.policy
    });
    childPlan.pipelineInputs![portName] = dataType;
    parentInputs[portName] = dataType;
  }

  const parentOutputs: Record<string, PipelineDataType> = {};
  const outputBoundaryNodeIds = new Map<string, string>();
  const outputBoundaryPorts = new Map<string, string>();
  const usedOutputPorts = new Set<string>();

  for (const outgoingConn of outgoing) {
    const portKey = `${outgoingConn.from.node}:${outgoingConn.from.port}`;
    const portName = outputBoundaryPorts.get(portKey) ?? allocateBoundaryPort(outgoingConn.from.port, usedOutputPorts);
    const dataType = dataTypeForEndpoint(plan, outgoingConn.from, 'out') ?? dataTypeForEndpoint(plan, outgoingConn.to, 'in');
    const existingBoundaryId = outputBoundaryNodeIds.get(portKey);
    const outputNode =
      existingBoundaryId && childPlan.nodes?.[existingBoundaryId]
        ? childPlan.nodes[existingBoundaryId]
        : createPipelineOutputNode(portName, dataType);
    if (!existingBoundaryId) {
      outputBoundaryNodeIds.set(portKey, outputNode.id);
      outputBoundaryPorts.set(portKey, portName);
      childPlan.nodes[outputNode.id] = outputNode;
    }
    childPlan.connections.push({
      from: { node: outgoingConn.from.node, port: outgoingConn.from.port },
      to: { node: outputNode.id, port: portName },
      policy: outgoingConn.policy
    });
    childPlan.pipelineOutputs![portName] = dataType;
    parentOutputs[portName] = dataType;
  }

  const remainingConnections = (plan.connections ?? []).filter((conn) => !selectedIdsSet.has(conn.from.node) && !selectedIdsSet.has(conn.to.node));
  const newNodeId = generateNodeId();

  const selectedLocations = present
    .map((id) => nodesMap[id]?.info?.location)
    .filter((loc): loc is { x: number; y: number } => !!loc && typeof loc.x === 'number' && typeof loc.y === 'number');
  const center =
    selectedLocations.length > 0
      ? {
          x: selectedLocations.reduce((sum, loc) => sum + loc.x, 0) / selectedLocations.length,
          y: selectedLocations.reduce((sum, loc) => sum + loc.y, 0) / selectedLocations.length
        }
      : representative.info?.location ?? { x: 0, y: 0 };

  const newNode: PipelineGraphNode = {
    id: newNodeId,
    backendId: 'pipeline:child',
    metadata: {
      name: representative.metadata?.name || 'Group',
      summary: representative.metadata?.summary
    },
    inputs: parentInputs,
    outputs: parentOutputs,
    info: {
      id: `pipeline:child-${newNodeId.slice(0, 8)}`,
      location: center
    },
    sync: representative.sync,
    embedded: childPlan,
    external: null,
    source: null
  };

  const rewired: PipelineConnection[] = [...remainingConnections];
  const rewiredSeen = new Set<string>(rewired.map(makeConnectionKey));
  const pushUniqueRewired = (connection: PipelineConnection) => {
    const key = makeConnectionKey(connection);
    if (rewiredSeen.has(key)) return;
    rewiredSeen.add(key);
    rewired.push(connection);
  };
  for (const incomingConn of incoming) {
    const boundaryPort = inputBoundaryPorts.get(`${incomingConn.from.node}:${incomingConn.from.port}`) ?? incomingConn.to.port;
    pushUniqueRewired({
      from: incomingConn.from,
      to: { node: newNodeId, port: boundaryPort },
      policy: incomingConn.policy
    });
  }
  for (const outgoingConn of outgoing) {
    const boundaryPort = outputBoundaryPorts.get(`${outgoingConn.from.node}:${outgoingConn.from.port}`) ?? outgoingConn.from.port;
    pushUniqueRewired({
      from: { node: newNodeId, port: boundaryPort },
      to: outgoingConn.to,
      policy: outgoingConn.policy
    });
  }

  const nextNodes = { ...(plan.nodes ?? {}) };
  for (const id of selectedIdsSet) {
    delete nextNodes[id];
  }
  nextNodes[newNodeId] = newNode;

  plan.nodes = nextNodes;
  plan.connections = rewired;
  return newNodeId;
}

export function ungroupSelectionInPlace(options: { plan: PipelineGraphPlan; groupNodeId: string; generateNodeId: () => string }): boolean {
  const { plan, groupNodeId, generateNodeId } = options;
  const groupNode = plan.nodes?.[groupNodeId];
  if (!groupNode || groupNode.backendId.toLowerCase() !== 'pipeline:child' || !groupNode.embedded) return false;

  const embedded = (typeof structuredClone === 'function' ? structuredClone(groupNode.embedded) : JSON.parse(JSON.stringify(groupNode.embedded))) as PipelineGraphPlan;
  const embeddedNodes = embedded.nodes ?? {};

  const boundaryInputs = Object.entries(embeddedNodes).filter(([, n]) => n.backendId === 'pipeline:input').map(([id]) => id);
  const boundaryOutputs = Object.entries(embeddedNodes).filter(([, n]) => n.backendId === 'pipeline:output').map(([id]) => id);

  const incomingParent = (plan.connections ?? []).filter((conn) => conn.to.node === groupNodeId);
  const outgoingParent = (plan.connections ?? []).filter((conn) => conn.from.node === groupNodeId);

  const idMap = new Map<string, string>();
  for (const childId of Object.keys(embeddedNodes)) {
    idMap.set(childId, generateNodeId());
  }

  const translatedNodes: Record<string, PipelineGraphNode> = {};
  for (const [childId, childNode] of Object.entries(embeddedNodes)) {
    if (boundaryInputs.includes(childId) || boundaryOutputs.includes(childId)) {
      continue;
    }
    const mappedId = idMap.get(childId)!;
    const cloned = cloneNode(childNode);
    cloned.id = mappedId;
    translatedNodes[mappedId] = cloned;
  }

  const translatedConnections: PipelineConnection[] = [];
  const boundaryInputTargets: Record<string, Array<{ node: string; port: string; policy?: ChannelPolicy }>> = {};
  const boundaryOutputSources: Record<string, Array<{ node: string; port: string; policy?: ChannelPolicy }>> = {};

  for (const conn of embedded.connections ?? []) {
    const fromId = conn.from.node;
    const toId = conn.to.node;
    const fromBoundaryInput = boundaryInputs.includes(fromId);
    const toBoundaryOutput = boundaryOutputs.includes(toId);

    if (fromBoundaryInput) {
      const boundaryPort = conn.from.port;
      boundaryInputTargets[boundaryPort] = boundaryInputTargets[boundaryPort] ?? [];
      boundaryInputTargets[boundaryPort].push({ node: idMap.get(toId) ?? toId, port: conn.to.port, policy: conn.policy });
      continue;
    }
    if (toBoundaryOutput) {
      const boundaryPort = conn.to.port;
      boundaryOutputSources[boundaryPort] = boundaryOutputSources[boundaryPort] ?? [];
      boundaryOutputSources[boundaryPort].push({ node: idMap.get(fromId) ?? fromId, port: conn.from.port, policy: conn.policy });
      continue;
    }

    const mappedFrom = idMap.get(fromId);
    const mappedTo = idMap.get(toId);
    if (mappedFrom && mappedTo) {
      translatedConnections.push({
        from: { node: mappedFrom, port: conn.from.port },
        to: { node: mappedTo, port: conn.to.port },
        policy: conn.policy
      });
    }
  }

  const remainingConnections = (plan.connections ?? []).filter((conn) => conn.from.node !== groupNodeId && conn.to.node !== groupNodeId);
  const rewired: PipelineConnection[] = [...remainingConnections, ...translatedConnections];

  for (const incoming of incomingParent) {
    const targets = boundaryInputTargets[incoming.to.port] ?? [];
    for (const target of targets) {
      rewired.push({
        from: incoming.from,
        to: { node: target.node, port: target.port },
        policy: incoming.policy ?? target.policy ?? 'NewestWins'
      });
    }
  }

  for (const outgoing of outgoingParent) {
    const sources = boundaryOutputSources[outgoing.from.port] ?? [];
    for (const source of sources) {
      rewired.push({
        from: { node: source.node, port: source.port },
        to: outgoing.to,
        policy: outgoing.policy ?? source.policy ?? 'NewestWins'
      });
    }
  }

  const nextNodes = { ...(plan.nodes ?? {}) };
  delete nextNodes[groupNodeId];
  for (const [id, node] of Object.entries(translatedNodes)) {
    nextNodes[id] = node;
  }
  plan.nodes = nextNodes;
  plan.connections = rewired;
  return true;
}
