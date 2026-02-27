import { PIPELINE_OUTPUT_BACKEND_ID } from './boundaryUtils';

const normalizeId = (value: unknown): string => String(value ?? '').trim().toLowerCase();

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
  if (typeof value === 'object') {
    Object.keys(value as Record<string, unknown>).forEach((key) => {
      const trimmed = key.trim();
      if (trimmed) ports.add(trimmed);
    });
  }
};

export const extractGraphOutputPorts = (graph: any): string[] => {
  if (!graph || typeof graph !== 'object') return [];
  const nested = (graph as any)?.graph ?? (graph as any)?.pipeline_graph ?? (graph as any)?.pipelineGraph;
  if (nested && nested !== graph && typeof nested === 'object') {
    return extractGraphOutputPorts(nested);
  }
  const ports = new Set<string>();

  const nodesArray = Array.isArray(graph?.nodes) ? graph.nodes : null;
  const edgesArray = Array.isArray(graph?.edges) ? graph.edges : null;
  if (nodesArray) {
    const hostIndices: number[] = [];
    const hostIds = new Set<string>();
    nodesArray.forEach((node: any, idx: number) => {
      if (isHostOutputId(node?.id) || isHostOutputBackendId(node?.backendId ?? node?.backend_id ?? node?.backend)) {
        hostIndices.push(idx);
        const id = normalizeId(node?.id ?? node?.info?.id);
        if (id) hostIds.add(id);
      }
    });

    if (hostIndices.length && edgesArray && edgesArray.length) {
      edgesArray.forEach((edge: any) => {
        const toRaw = edge?.to?.node ?? edge?.to?.nodeId ?? edge?.to?.id ?? edge?.to;
        const toId =
          typeof toRaw === 'string'
            ? normalizeId(toRaw)
            : typeof toRaw === 'object'
              ? normalizeId((toRaw as any)?.id ?? (toRaw as any)?.nodeId)
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
        const port = typeof edge?.to?.port === 'string' ? edge.to.port.trim() : '';
        if (port) ports.add(port);
      });
      if (ports.size) {
        return Array.from(ports).sort((a, b) => a.localeCompare(b));
      }
    }

    if (hostIndices.length) {
      hostIndices.forEach((idx) => {
        const node = nodesArray[idx] ?? null;
        collectPorts(node?.inputs, ports);
        collectPorts(node?.metadata?.inputPorts, ports);
      });
      if (ports.size) {
        return Array.from(ports).sort((a, b) => a.localeCompare(b));
      }
    }
  }

  const nodesRecord =
    graph?.nodes && typeof graph.nodes === 'object' && !Array.isArray(graph.nodes) ? (graph.nodes as Record<string, any>) : null;
  if (nodesRecord) {
    Object.entries(nodesRecord).forEach(([key, node]) => {
      if (!node) return;
      const matches =
        isHostOutputBackendId(node?.backendId ?? node?.backend_id) ||
        isHostOutputId(node?.id ?? node?.info?.id) ||
        isHostOutputId(key);
      if (!matches) return;
      collectPorts(node?.inputs, ports);
      collectPorts(node?.metadata?.inputPorts, ports);
    });
  }

  const signatureOutputs = (graph as any)?.signature?.outputs;
  collectPorts(signatureOutputs, ports);

  if (!ports.size) {
    const pipelineOutputs = (graph as any)?.pipelineOutputs ?? (graph as any)?.pipeline_outputs;
    collectPorts(pipelineOutputs, ports);
  }

  return Array.from(ports).sort((a, b) => a.localeCompare(b));
};
