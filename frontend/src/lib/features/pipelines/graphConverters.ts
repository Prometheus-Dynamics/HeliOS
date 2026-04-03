import { emptyPipelineGraphPlan } from '$lib/features/pipelines/graph';
import { fromDaedalusGraph, isDaedalusGraph } from '$lib/features/pipelines/daedalusGraph';
import { trimmedString } from './normalizeHelpers';
import type {
  PipelineDataType,
  PipelineGraphPlan
} from '$lib/types/pipeline';
import type {
  ApiPortDescriptor
} from '$lib/types/pipeline-api';

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' ? (value as Record<string, unknown>) : null;
}

function unwrapGraphPayload(value: unknown): unknown {
  let current = value;
  for (let depth = 0; depth < 3; depth += 1) {
    const record = asRecord(current);
    if (!record) return current;
    if (record.__helios_resource_cache__ === true && 'data' in record) {
      current = record.data;
      continue;
    }
    const nested = asRecord(record.graph);
    if (!nested) return current;
    current = nested;
  }
  return current;
}

export function fromApiPortDescriptor(
  descriptor: ApiPortDescriptor | null | undefined
): PipelineDataType | null {
  if (!descriptor) return null;
  const kind = trimmedString(descriptor.kind);
  if (!kind) return null;

  const overrides = descriptor.overrides ?? undefined;
  const extras: Partial<Exclude<PipelineDataType, string>> = {};

  if (overrides) {
    const format = trimmedString(overrides.format);
    if (format) extras.format = format;
    const label = trimmedString(overrides.label);
    if (label) extras.label = label;
    const color = trimmedString(overrides.color);
    if (color) extras.color = color;
    const summary = trimmedString(overrides.summary);
    if (summary) extras.summary = summary;
    if (typeof overrides.settable === 'boolean') extras.settable = overrides.settable;
  }

  const element = descriptor.element ? fromApiPortDescriptor(descriptor.element) : null;
  if (element) {
    (extras as { element?: PipelineDataType }).element = element;
  }

  const variants = Array.isArray(descriptor.variants)
    ? descriptor.variants
        .map((value: unknown) => (typeof value === 'string' ? value.trim() : ''))
        .filter((value): value is string => value.length > 0)
    : [];
  if (variants.length > 0) {
    (extras as { variants?: string[] }).variants = variants;
  }

  const hasExtras = Object.keys(extras).length > 0;
  if (!hasExtras) {
    return kind;
  }

  return {
    kind,
    ...extras
  };
}

export function fromApiGraphPlan(plan: unknown): PipelineGraphPlan {
  const unwrapped = unwrapGraphPayload(plan);
  if (!unwrapped) return emptyPipelineGraphPlan();
  if (isDaedalusGraph(unwrapped)) return fromDaedalusGraph(unwrapped);
  if (typeof unwrapped === 'object' && unwrapped !== null && Object.keys(unwrapped as Record<string, unknown>).length > 0) {
    console.warn('Ignoring non-Daedalus graph payload; expected a Daedalus graph.');
  }
  return emptyPipelineGraphPlan();
}
