import type { PipelineDataType, PipelineGraphNode, PipelineGraphPlan, PipelineRegistryEntry } from '$lib/types/pipeline';

export type GpuSegmentPlan = {
  id: number;
  nodes: string[];
};

export const GPU_SEGMENT_COLORS = ['#22d3ee', '#a78bfa', '#fb923c', '#34d399', '#f472b6', '#fcd34d'];

export const gpuSegmentColor = (segmentId: number | null | undefined): string => {
  if (!segmentId || segmentId < 1) {
    return 'rgba(45, 212, 191, 0.9)';
  }
  return GPU_SEGMENT_COLORS[(segmentId - 1) % GPU_SEGMENT_COLORS.length];
};

type GpuCapability = {
  preference?: string | null;
  inputs?: { port?: string | null; kind?: string | null; format?: string | null }[];
  outputs?: { port?: string | null; kind?: string | null; format?: string | null }[];
};

const normalizePort = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed.toLowerCase() : null;
};

const resolveGpuKindFromDataType = (dataType?: PipelineDataType): string | null => {
  if (!dataType) return null;
  if (typeof dataType === 'string') {
    const normalized = dataType.toLowerCase();
    if (normalized.includes('gpuframe') || normalized === 'image') return 'frame';
    if (normalized.includes('gpumask') || normalized === 'gray' || normalized === 'binary') return 'mask';
    return null;
  }
  const normalized = (dataType.kind ?? '').toLowerCase();
  if (normalized.includes('gpuframe') || normalized === 'image') return 'frame';
  if (normalized.includes('gpumask') || normalized === 'gray' || normalized === 'binary') return 'mask';
  return null;
};

const resolveGpuPortKind = (
  node: PipelineGraphNode,
  port: string,
  isOutput: boolean,
  registryEntry: PipelineRegistryEntry | null
): string | null => {
  const meta = (node.metadata?.gpu ?? null) as GpuCapability | null;
  const registryMeta = (registryEntry?.metadata?.gpu ?? null) as GpuCapability | null;
  const formats = (isOutput ? meta?.outputs : meta?.inputs) ?? [];
  const registryFormats = (isOutput ? registryMeta?.outputs : registryMeta?.inputs) ?? [];
  const normalized = normalizePort(port);
  const explicit =
    formats.find((fmt) => normalizePort(fmt.port) === normalized) ??
    registryFormats.find((fmt) => normalizePort(fmt.port) === normalized);
  if (explicit?.kind) {
    return explicit.kind.toLowerCase();
  }
  const dataMap = isOutput
    ? (node.outputs ?? registryEntry?.outputs)
    : (node.inputs ?? registryEntry?.inputs);
  const kind = resolveGpuKindFromDataType(dataMap?.[port]);
  if (kind) return kind;
  const fallbackFormats = formats.length > 0 ? formats : registryFormats;
  if (fallbackFormats.length > 0 && fallbackFormats[0]?.kind) {
    return fallbackFormats[0].kind?.toLowerCase() ?? null;
  }
  return null;
};

const isGpuCapable = (node: PipelineGraphNode, registryEntry: PipelineRegistryEntry | null): boolean => {
  const compute = (node.source as { compute?: unknown } | null | undefined)?.compute;
  if (compute === 'GpuPreferred' || compute === 'GpuRequired') {
    return true;
  }
  const pref = (node.metadata?.gpu as GpuCapability | null)?.preference;
  const normalized = typeof pref === 'string' ? pref.toLowerCase() : '';
  if (normalized === 'supported' || normalized === 'preferred') {
    return true;
  }
  const registryPref = registryEntry?.metadata?.gpu?.preference;
  return registryPref === 'supported' || registryPref === 'preferred';
};

const edgeGpuCompatible = (
  fromNode: PipelineGraphNode,
  toNode: PipelineGraphNode,
  fromPort: string,
  toPort: string,
  fromRegistryEntry: PipelineRegistryEntry | null,
  toRegistryEntry: PipelineRegistryEntry | null
): boolean => {
  const fromKind = resolveGpuPortKind(fromNode, fromPort, true, fromRegistryEntry);
  const toKind = resolveGpuPortKind(toNode, toPort, false, toRegistryEntry);
  return Boolean(fromKind && fromKind === toKind);
};

type DetectGpuSegmentsOptions = {
  resolveRegistryEntry?: (node: PipelineGraphNode) => PipelineRegistryEntry | null;
};

export const detectGpuSegments = (
  graph: PipelineGraphPlan,
  options: DetectGpuSegmentsOptions = {}
): { segments: GpuSegmentPlan[]; nodeToSegment: Map<string, number>; gpuNodeIds: Set<string> } => {
  const resolveRegistryEntry = options.resolveRegistryEntry ?? (() => null);
  const incoming = new Map<string, Set<string>>();
  const outgoing = new Map<string, string[]>();
  const gpuNodeIds = new Set<string>();

  Object.entries(graph.nodes ?? {}).forEach(([nodeId, node]) => {
    if (isGpuCapable(node, resolveRegistryEntry(node))) {
      gpuNodeIds.add(nodeId);
    }
  });

  for (const connection of graph.connections ?? []) {
    const from = graph.nodes?.[connection.from.node];
    const to = graph.nodes?.[connection.to.node];
    if (!from || !to) continue;
    const fromRegistry = resolveRegistryEntry(from);
    const toRegistry = resolveRegistryEntry(to);
    if (!isGpuCapable(from, fromRegistry) || !isGpuCapable(to, toRegistry)) continue;
    if (!edgeGpuCompatible(from, to, connection.from.port, connection.to.port, fromRegistry, toRegistry)) continue;
    outgoing.set(connection.from.node, [...(outgoing.get(connection.from.node) ?? []), connection.to.node]);
    const destSet = incoming.get(connection.to.node) ?? new Set<string>();
    destSet.add(connection.from.node);
    incoming.set(connection.to.node, destSet);
  }

  const nodeToSegment = new Map<string, number>();
  const segments: GpuSegmentPlan[] = [];
  let nextId = 1;

  const nodes = Object.keys(graph.nodes ?? {});
  for (const nodeId of nodes) {
    if (nodeToSegment.has(nodeId)) continue;
    const node = graph.nodes?.[nodeId];
    if (!node || !isGpuCapable(node, resolveRegistryEntry(node))) continue;
    const inbound = incoming.get(nodeId);
    if (inbound && inbound.size > 0) continue;

    const stack = [nodeId];
    const segmentNodes: string[] = [];
    while (stack.length > 0) {
      const current = stack.pop()!;
      if (nodeToSegment.has(current)) continue;
      const currentNode = graph.nodes?.[current];
      if (!currentNode || !isGpuCapable(currentNode, resolveRegistryEntry(currentNode))) continue;
      nodeToSegment.set(current, nextId);
      segmentNodes.push(current);
      const children = outgoing.get(current) ?? [];
      children.forEach((child) => stack.push(child));
    }
    if (segmentNodes.length > 0) {
      segments.push({ id: nextId, nodes: segmentNodes });
      nextId += 1;
    }
  }

  return { segments, nodeToSegment, gpuNodeIds };
};
