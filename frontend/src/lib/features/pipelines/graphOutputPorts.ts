import { PIPELINE_OUTPUT_BACKEND_ID } from './boundaryUtils';

const normalizeId = (value: unknown): string => String(value ?? '').trim().toLowerCase();
const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

const isHostOutputId = (value: unknown): boolean => {
  const id = normalizeId(value);
  if (!id) return false;
  if (id === 'io.host_output' || id === PIPELINE_OUTPUT_BACKEND_ID) return true;
  if (id.startsWith('io.host_output.') || id.startsWith('io.host_output:')) return true;
  if (id.endsWith(':io.host_output')) return true;
  if (id.startsWith(`${PIPELINE_OUTPUT_BACKEND_ID}:`) || id.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`)) return true;
  return false;
};

const isHostOutputBackendId = (value: unknown): boolean => {
  const backendId = normalizeId(value);
  if (!backendId) return false;
  if (backendId === 'io.host_output' || backendId === PIPELINE_OUTPUT_BACKEND_ID) return true;
  if (backendId.endsWith(':io.host_output') || backendId.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`)) return true;
  if (backendId.startsWith('io.host_output.') || backendId.startsWith('io.host_output:')) return true;
  return false;
};

const collectPorts = (value: unknown, ports: Set<string>) => {
  if (!value) return;
  if (Array.isArray(value)) {
    value.forEach((entry) => {
      if (typeof entry !== 'string') return;
      const trimmed = entry.trim();
      if (trimmed) ports.add(trimmed);
    });
    return;
  }
  Object.keys(asRecord(value) ?? {}).forEach((key) => {
    const trimmed = key.trim();
    if (trimmed) ports.add(trimmed);
  });
};

export const extractGraphOutputPorts = (graph: unknown): string[] => {
  const graphRecord = asRecord(graph);
  if (!graphRecord) return [];
  const nested = graphRecord.graph ?? graphRecord.pipeline_graph ?? graphRecord.pipelineGraph;
  const nestedRecord = asRecord(nested);
  if (nestedRecord && nested !== graph) {
    return extractGraphOutputPorts(nestedRecord);
  }
  const ports = new Set<string>();

  const nodesArray = Array.isArray(graphRecord.nodes) ? graphRecord.nodes : null;
  const edgesArray = Array.isArray(graphRecord.edges) ? graphRecord.edges : null;
  if (nodesArray) {
    const hostIndices: number[] = [];
    const hostIds = new Set<string>();
    nodesArray.forEach((nodeValue, idx) => {
      const node = asRecord(nodeValue);
      if (isHostOutputId(node?.id) || isHostOutputBackendId(node?.backendId ?? node?.backend_id ?? node?.backend)) {
        hostIndices.push(idx);
        const id = normalizeId(node?.id ?? asRecord(node?.info)?.id);
        if (id) hostIds.add(id);
      }
    });

    if (hostIndices.length && edgesArray && edgesArray.length) {
      edgesArray.forEach((edgeValue) => {
        const edge = asRecord(edgeValue);
        const edgeTarget = asRecord(edge?.to);
        const toRaw = edgeTarget?.node ?? edgeTarget?.nodeId ?? edgeTarget?.id ?? edge?.to;
        const toId =
          typeof toRaw === 'string'
            ? normalizeId(toRaw)
            : asRecord(toRaw)
              ? normalizeId(asRecord(toRaw)?.id ?? asRecord(toRaw)?.nodeId)
              : '';
        const toIndex =
          typeof toRaw === 'number'
            ? Math.trunc(toRaw)
            : typeof toRaw === 'string' && /^[0-9]+$/.test(toRaw.trim())
              ? Math.trunc(Number(toRaw))
              : NaN;
        if (Number.isFinite(toIndex)) {
          if (!hostIndices.includes(toIndex)) return;
        } else if (toId) {
          if (!hostIds.has(toId)) return;
        } else {
          return;
        }
        const port = typeof edgeTarget?.port === 'string' ? edgeTarget.port.trim() : '';
        if (port) ports.add(port);
      });
      if (ports.size) {
        return Array.from(ports).sort((a, b) => a.localeCompare(b));
      }
    }

    if (hostIndices.length) {
      hostIndices.forEach((idx) => {
        const node = asRecord(nodesArray[idx] ?? null);
        collectPorts(node?.inputs, ports);
        collectPorts(asRecord(node?.metadata)?.inputPorts, ports);
      });
      if (ports.size) {
        return Array.from(ports).sort((a, b) => a.localeCompare(b));
      }
    }
  }

  const nodesRecord = asRecord(graphRecord.nodes);
  if (nodesRecord && !Array.isArray(graphRecord.nodes)) {
    Object.entries(nodesRecord).forEach(([key, nodeValue]) => {
      const node = asRecord(nodeValue);
      if (!node) return;
      const matches =
        isHostOutputBackendId(node?.backendId ?? node?.backend_id) ||
        isHostOutputId(node?.id ?? asRecord(node?.info)?.id) ||
        isHostOutputId(key);
      if (!matches) return;
      collectPorts(node?.inputs, ports);
      collectPorts(asRecord(node?.metadata)?.inputPorts, ports);
    });
  }

  const signatureOutputs = asRecord(graphRecord.signature)?.outputs;
  collectPorts(signatureOutputs, ports);

  if (!ports.size) {
    const pipelineOutputs = graphRecord.pipelineOutputs ?? graphRecord.pipeline_outputs;
    collectPorts(pipelineOutputs, ports);
  }

  return Array.from(ports).sort((a, b) => a.localeCompare(b));
};
