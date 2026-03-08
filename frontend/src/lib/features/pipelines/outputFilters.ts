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

type UnknownRecord = Record<string, unknown>;

const asRecord = (value: unknown): UnknownRecord | null =>
  value && typeof value === 'object' ? (value as UnknownRecord) : null;

const coerceDataType = (value: unknown): PipelineDataType =>
  value == null ? 'Generic' : (value as PipelineDataType);

const isOutputNode = (node: UnknownRecord | null): boolean => {
  const backendId = String(node?.backendId ?? node?.backend_id ?? node?.backend ?? '').toLowerCase();
  const id = String(node?.id ?? asRecord(node?.info)?.id ?? '').toLowerCase();
  return (
    backendId === PIPELINE_OUTPUT_BACKEND_ID ||
    backendId.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`) ||
    backendId === 'io.host_output' ||
    backendId.endsWith(':io.host_output') ||
    id === PIPELINE_OUTPUT_BACKEND_ID ||
    id.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`) ||
    id === 'io.host_output' ||
    id.endsWith(':io.host_output')
  );
};

const applyPortTypes = (
  target: Record<string, PipelineDataType>,
  source: UnknownRecord | null,
  overwrite = false
): void => {
  if (!source) return;
  Object.entries(source).forEach(([name, dataType]) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    if (!overwrite && target[trimmed]) return;
    target[trimmed] = coerceDataType(dataType);
  });
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

export const extractGraphOutputPortTypes = (graph: unknown): Record<string, PipelineDataType> => {
  const types: Record<string, PipelineDataType> = {};
  const graphRecord = asRecord(graph);
  if (!graphRecord) return types;
  const nested = graphRecord.graph ?? graphRecord.pipeline_graph ?? graphRecord.pipelineGraph;
  const nestedRecord = asRecord(nested);
  const target = nestedRecord && nested !== graph ? nestedRecord : graphRecord;
  applyPortTypes(types, asRecord(target.pipelineOutputs ?? target.pipeline_outputs), true);
  applyPortTypes(types, asRecord(asRecord(target.signature)?.outputs));

  const nodesArray = Array.isArray(target.nodes) ? target.nodes : null;
  if (nodesArray) {
    nodesArray.forEach((nodeValue) => {
      const node = asRecord(nodeValue);
      if (!isOutputNode(node)) return;
      applyPortTypes(types, asRecord(node?.inputs));
    });
  }
  const nodesRecord = asRecord(target.nodes);
  if (nodesRecord && !Array.isArray(target.nodes)) {
    Object.values(nodesRecord).forEach((nodeValue) => {
      const node = asRecord(nodeValue);
      if (!isOutputNode(node)) return;
      applyPortTypes(types, asRecord(node?.inputs));
    });
  }
  return types;
};
