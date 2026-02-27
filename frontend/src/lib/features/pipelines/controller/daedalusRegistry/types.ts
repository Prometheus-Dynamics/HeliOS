import type { PipelineNodeMetadata } from '$lib/types/pipeline';

export type DaedalusRegistryPort = {
  name?: string;
  ty?: unknown;
  source?: string | null;
  const_value?: unknown;
};

export type DaedalusFanInPort = {
  prefix?: string;
  start?: number;
  ty?: unknown;
};

export type DaedalusRegistryType = {
  rust?: string;
  ty?: unknown;
};

export type TypeDescription = {
  key: string;
  label: string;
  kind: string;
  element?: TypeDescription;
  variants?: string[];
};

export type TypeRegistryLookup = Map<string, unknown>;

export type GpuPreference = NonNullable<PipelineNodeMetadata['gpu']>['preference'];
