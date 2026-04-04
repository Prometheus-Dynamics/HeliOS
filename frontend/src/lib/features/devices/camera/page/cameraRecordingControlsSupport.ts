import type { StreamInfo, StreamManifest, StreamPipelineBinding } from '$lib/api/client';

export type PipelineBindingLike = StreamPipelineBinding & {
  pipelineId?: string | null;
  id?: string | null;
  pipelineGraph?: unknown;
  graph?: unknown;
};

export type RecordingManifest = StreamManifest & {
  pipeline_id?: string | null;
  pipeline_output?: string | null;
  pipelines?: PipelineBindingLike[] | null;
};

export type PipelineState = {
  selectedPipelineId?: string | null;
  assignedPipelineIds?: string[] | null;
  pipelineGridSlots?: Record<string, string | null> | null;
};

export type PipelineOutputOptionsCache = Map<string, string[]> | Record<string, string[]>;

export type RecordingControlsContext = {
  stream: StreamInfo | null;
  streamId: string | null;
  activePipelineIds?: string[] | null;
  pipelineState?: PipelineState | null;
  RAW_PIPELINE_ID: string;
  RAW_PIPELINE_UUID: string;
  extractGraphOutputPorts?: ((graph: unknown) => string[]) | null;
  pipelineOutputOptionsCache?: PipelineOutputOptionsCache | (() => PipelineOutputOptionsCache | null | undefined) | null;
  pipelineLabel?: ((pipelineId: string) => string) | null;
  apiPath: (path: string) => string;
  refresh?: (() => Promise<void> | void) | null;
  ensurePipelineOutputsLoaded?: ((pipelineId: string) => void) | null;
};

export const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

export const dedupePipelineIds = (values: Iterable<string>): string[] => {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const value of values) {
    const normalized = typeof value === 'string' ? value.trim() : '';
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(normalized);
  }
  return out;
};

export const manifestFor = (stream: StreamInfo | null): RecordingManifest | null => {
  const manifest = stream?.manifest ?? null;
  return manifest ? (manifest as RecordingManifest) : null;
};

export const pipelineBindingsFor = (manifest: RecordingManifest | null): PipelineBindingLike[] =>
  Array.isArray(manifest?.pipelines) ? manifest.pipelines.filter((entry): entry is PipelineBindingLike => Boolean(entry)) : [];
