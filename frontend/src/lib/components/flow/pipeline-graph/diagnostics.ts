import type { PipelineDiagnostics, PipelineGraphPlan } from '$lib/types/pipeline';
import type { PipelineGraphDiagnostics } from './types';

export type GraphNodeEntry = { key: string; node: PipelineGraphPlan['nodes'][string] };

export const normalizeIdentifier = (value: string | null | undefined): string | null => {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed.toLowerCase() : null;
};

export const findNodeEntryForId = (
  graph: PipelineGraphPlan,
  nodeId: string | null | undefined
): GraphNodeEntry | null => {
  if (!nodeId) return null;
  const nodes = graph.nodes ?? {};
  if (nodes[nodeId]) {
    return { key: nodeId, node: nodes[nodeId] };
  }
  const normalized = normalizeIdentifier(nodeId);
  if (!normalized) return null;
  for (const [key, candidate] of Object.entries(nodes)) {
    if (normalizeIdentifier(key) === normalized) {
      return { key, node: candidate };
    }
    const candidateId = normalizeIdentifier((candidate as { id?: string }).id as string | undefined);
    if (candidateId === normalized) {
      return { key, node: candidate };
    }
    const infoId = normalizeIdentifier(candidate.info?.id);
    if (infoId === normalized) {
      return { key, node: candidate };
    }
  }
  return null;
};

export const matchPortInNode = (
  planNode: PipelineGraphPlan['nodes'][string],
  portName: string | null | undefined
): { name: string; direction: 'input' | 'output' } | null => {
  const normalized = normalizeIdentifier(portName);
  if (!normalized) return null;
  const matchInRecord = (record: Record<string, unknown> | undefined) => {
    if (!record) return null;
    for (const key of Object.keys(record)) {
      if (normalizeIdentifier(key) === normalized) {
        return key;
      }
    }
    return null;
  };
  const inputMatch = matchInRecord(planNode.inputs);
  if (inputMatch) {
    return { name: inputMatch, direction: 'input' };
  }
  const outputMatch = matchInRecord(planNode.outputs);
  if (outputMatch) {
    return { name: outputMatch, direction: 'output' };
  }
  return null;
};

export const buildGraphDiagnostics = (
  graph: PipelineGraphPlan,
  diagnostics: PipelineDiagnostics | null | undefined
): PipelineGraphDiagnostics => {
  if (!diagnostics) {
    return {};
  }
  const warnings = diagnostics.warnings ?? [];
  if (warnings.length === 0) {
    return {};
  }
  const lookup: PipelineGraphDiagnostics = {};
  for (const warning of warnings) {
    const entry = findNodeEntryForId(graph, warning.nodeId ?? null);
    if (!entry) {
      continue;
    }
    const key = entry.key;
    const nodeState =
      lookup[key] ??
      {
        nodeMessages: [],
        hasNodeIssue: false,
        inputPorts: [],
        outputPorts: [],
        portMessages: {}
      };
    nodeState.nodeMessages.push(warning.message);
    nodeState.hasNodeIssue = true;
    if (warning.port) {
      const matched = matchPortInNode(entry.node, warning.port);
      if (matched) {
        const target = matched.direction === 'input' ? nodeState.inputPorts : nodeState.outputPorts;
        if (!target.includes(matched.name)) {
          target.push(matched.name);
        }
        (nodeState.portMessages ??= {});
        const messages = (nodeState.portMessages[matched.name] ??= []);
        if (!messages.includes(warning.message)) {
          messages.push(warning.message);
        }
      }
    }
    lookup[key] = nodeState;
  }
  return lookup;
};
