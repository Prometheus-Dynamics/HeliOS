import type { PipelineDataType } from '$lib/types/pipeline';
import { resolveDataTypeKey } from './valueFormatting';
import { PIPELINE_OUTPUT_BACKEND_ID } from './boundaryUtils';

// NOTE: Do not infer "image-like" from port *names*; only infer from the port's declared/solved type.
// If the type is unknown, upstream call sites should either provide type info or avoid filtering.
const OPTIONAL_TYPE_RE = /^optional\s*<\s*(.+)\s*>$/iu;

const unwrapOptionalKey = (raw: string): string => {
  let key = raw.trim();
  // Unwrap nested optional<...> wrappers if present (Daedalus TypeExpr formatting).
  while (true) {
    const match = key.match(OPTIONAL_TYPE_RE);
    if (!match?.[1]) break;
    key = match[1].trim();
  }
  return key;
};

const isImageTypeKey = (raw: string): boolean => {
  const key = unwrapOptionalKey(raw).trim().toLowerCase();
  return key === 'image' || key.startsWith('image:');
};

export const isEncoderCompatibleOutput = (name: string, dataType?: PipelineDataType | null): boolean => {
  void name;
  const key = resolveDataTypeKey(dataType ?? undefined);
  if (!key) return false;
  return isImageTypeKey(key);
};

export const filterEncoderCompatibleOutputs = (
  outputs: string[],
  types: Record<string, PipelineDataType | null | undefined> = {}
): string[] =>
  outputs.filter((name) => isEncoderCompatibleOutput(name, types[name]));

export const extractGraphOutputPortTypes = (graph: any): Record<string, PipelineDataType> => {
  const types: Record<string, PipelineDataType> = {};
  if (!graph || typeof graph !== 'object') return types;
  const nested = (graph as any)?.graph ?? (graph as any)?.pipeline_graph ?? (graph as any)?.pipelineGraph;
  const target = nested && nested !== graph ? nested : graph;
  if (!target || typeof target !== 'object') return types;
  const pipelineOutputs = (target as any).pipelineOutputs ?? (target as any).pipeline_outputs;
  if (pipelineOutputs && typeof pipelineOutputs === 'object' && !Array.isArray(pipelineOutputs)) {
    Object.entries(pipelineOutputs as Record<string, PipelineDataType>).forEach(([name, dataType]) => {
      const trimmed = name.trim();
      if (trimmed) types[trimmed] = dataType ?? 'Generic';
    });
  }
  const signatureOutputs = (target as any)?.signature?.outputs;
  if (signatureOutputs && typeof signatureOutputs === 'object' && !Array.isArray(signatureOutputs)) {
    Object.entries(signatureOutputs as Record<string, PipelineDataType>).forEach(([name, dataType]) => {
      const trimmed = name.trim();
      if (trimmed && !types[trimmed]) types[trimmed] = dataType ?? 'Generic';
    });
  }
  const nodesArray = Array.isArray((target as any)?.nodes) ? (target as any).nodes : null;
  if (nodesArray) {
    nodesArray.forEach((node: any) => {
      if (!node) return;
      const backendId = String(node?.backendId ?? node?.backend_id ?? node?.backend ?? '').toLowerCase();
      const id = String(node?.id ?? node?.info?.id ?? '').toLowerCase();
      const isOutput =
        backendId === PIPELINE_OUTPUT_BACKEND_ID ||
        backendId.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`) ||
        backendId === 'io.host_output' ||
        backendId.endsWith(':io.host_output') ||
        id === PIPELINE_OUTPUT_BACKEND_ID ||
        id.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`) ||
        id === 'io.host_output' ||
        id.endsWith(':io.host_output');
      if (!isOutput) return;
      const inputs = node?.inputs;
      if (!inputs || typeof inputs !== 'object' || Array.isArray(inputs)) return;
      Object.entries(inputs as Record<string, PipelineDataType>).forEach(([name, dataType]) => {
        const trimmed = name.trim();
        if (!trimmed) return;
        if (!types[trimmed]) types[trimmed] = dataType ?? 'Generic';
      });
    });
  }
  const nodesRecord =
    (target as any)?.nodes && typeof (target as any).nodes === 'object' && !Array.isArray((target as any).nodes)
      ? ((target as any).nodes as Record<string, any>)
      : null;
  if (nodesRecord) {
    Object.values(nodesRecord).forEach((node) => {
      if (!node) return;
      const backendId = String(node?.backendId ?? '').toLowerCase();
      const id = String(node?.id ?? node?.info?.id ?? '').toLowerCase();
      const isOutput =
        backendId === PIPELINE_OUTPUT_BACKEND_ID ||
        backendId.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`) ||
        backendId === 'io.host_output' ||
        backendId.endsWith(':io.host_output') ||
        id === PIPELINE_OUTPUT_BACKEND_ID ||
        id.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`) ||
        id === 'io.host_output' ||
        id.endsWith(':io.host_output');
      if (!isOutput) return;
      const inputs = node?.inputs;
      if (!inputs || typeof inputs !== 'object' || Array.isArray(inputs)) return;
      Object.entries(inputs as Record<string, PipelineDataType>).forEach(([name, dataType]) => {
        const trimmed = name.trim();
        if (!trimmed) return;
        if (!types[trimmed]) types[trimmed] = dataType ?? 'Generic';
      });
    });
  }
  return types;
};
