import type { XYPosition } from '@xyflow/system';
import type { PipelineDataType, PipelineGraphPlan } from '$lib/types/pipeline';
import { PIPELINE_INPUT_BACKEND_ID, PIPELINE_OUTPUT_BACKEND_ID, normalizePipelinePortName } from '$lib/features/pipelines/boundary';

export const clampValue = (value: number, min: number, max: number): number =>
  Math.min(Math.max(value, min), max);

export const resolveBoundaryDirection = (
  node: PipelineGraphPlan['nodes'][string] | null | undefined
): 'input' | 'output' | null => {
  const backendId = (node?.backendId ?? '').toLowerCase();
  if (!backendId) return null;
  if (
    backendId === PIPELINE_INPUT_BACKEND_ID ||
    backendId === 'io.host_bridge' ||
    backendId.endsWith(':io.host_bridge')
  ) {
    return 'input';
  }
  if (
    backendId === PIPELINE_OUTPUT_BACKEND_ID ||
    backendId === 'io.host_output' ||
    backendId.endsWith(':io.host_output')
  ) {
    return 'output';
  }
  return null;
};

export const deriveAutoPortName = (
  base: string | null | undefined,
  direction: 'input' | 'output',
  existing: Record<string, PipelineDataType> | undefined
): string => {
  const fallback = direction === 'input' ? 'input' : 'output';
  const normalizedBase = normalizePipelinePortName(base ?? fallback);
  const candidate = normalizedBase || fallback;
  const record = existing ?? {};
  if (!record[candidate]) {
    return candidate;
  }
  let index = 2;
  while (record[`${candidate}_${index}`]) {
    index += 1;
  }
  return `${candidate}_${index}`;
};

export const resolveClientPosition = (event: MouseEvent | TouchEvent): XYPosition | null => {
  if ('touches' in event && event.touches.length > 0) {
    return { x: event.touches[0]!.clientX, y: event.touches[0]!.clientY };
  }
  if ('changedTouches' in event && event.changedTouches.length > 0) {
    return { x: event.changedTouches[0]!.clientX, y: event.changedTouches[0]!.clientY };
  }
  if ('clientX' in event && 'clientY' in event) {
    return { x: event.clientX, y: event.clientY };
  }
  return null;
};

export const normalizeSearchTokens = (value: string | null | undefined): string[] =>
  (value ?? '')
    .toLowerCase()
    .split(/\s+/)
    .map((token) => token.trim())
    .filter(Boolean);

export const isEditableTarget = (target: EventTarget | null): boolean => {
  if (!target || !(target as HTMLElement).closest) return false;
  const element = target as HTMLElement;
  return Boolean(element.closest('input, textarea, select, [contenteditable="true"]'));
};
