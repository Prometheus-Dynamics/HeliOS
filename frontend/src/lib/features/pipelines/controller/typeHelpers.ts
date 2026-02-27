import { get } from 'svelte/store';
import type { Readable } from 'svelte/store';
import type { PipelineDataType, PipelineTypeDescriptor } from '$lib/types/pipeline';
import { optionalString } from './utils';

type TypeHelperDeps = {
  dataTypes: Readable<Record<string, PipelineTypeDescriptor>>;
};

export type TypeHelpers = ReturnType<typeof createTypeHelpers>;

export function createTypeHelpers(deps: TypeHelperDeps) {
  function resolveTypeDescriptor(input: string): PipelineTypeDescriptor | null {
    const trimmed = input.trim();
    if (!trimmed) return null;
    const palette = get(deps.dataTypes);
    const candidates = [trimmed];
    const suffixMatch = trimmed.match(/\(([^)]+)\)\s*$/);
    if (suffixMatch?.[1]) {
      candidates.push(suffixMatch[1].trim());
    }
    for (const candidate of candidates) {
      const descriptor = palette[candidate];
      if (descriptor) {
        return descriptor;
      }
    }
    const lowered = candidates.map((value) => value.toLowerCase());
    const matchByLabel = Object.values(palette).find((descriptor) => {
      const label = descriptor.label?.toLowerCase();
      return label ? lowered.includes(label) : false;
    });
    return matchByLabel ?? null;
  }

  function buildDataTypeFromKey(input: string): PipelineDataType {
    const descriptor = resolveTypeDescriptor(input);
    if (descriptor) {
      return {
        kind: descriptor.id,
        label: optionalString(descriptor.label),
        color: optionalString(descriptor.color),
        summary: optionalString(descriptor.summary),
        settable: descriptor.settable,
        descriptor
      };
    }
    const trimmed = input.trim();
    return trimmed ? trimmed : 'Generic';
  }

  return {
    resolveTypeDescriptor,
    buildDataTypeFromKey
  };
}
