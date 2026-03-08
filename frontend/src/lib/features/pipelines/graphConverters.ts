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
  if (!plan) return emptyPipelineGraphPlan();
  if (isDaedalusGraph(plan)) return fromDaedalusGraph(plan);
  console.warn('Legacy pipeline graph format is no longer supported; expected a Daedalus graph.');
  return emptyPipelineGraphPlan();
}
