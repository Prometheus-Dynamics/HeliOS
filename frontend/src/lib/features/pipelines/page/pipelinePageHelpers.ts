import type { StreamInfo, StreamManifest, StreamPipelineBinding } from '$lib/ts-bindings/http/client';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineOverviewPipeline,
  PipelineTemplateSummary
} from '$lib/types/pipeline';
import { filterEncoderCompatibleOutputs } from '$lib/features/pipelines/outputFilters';
import { resolveStreamLabel } from '$lib/utils/streamLabels';

export type PipelinePageHelpersDeps = {
  getPipelines: () => PipelineOverviewPipeline[];
  getTemplates: () => PipelineTemplateSummary[];
  getCreateMode: () => 'blank' | 'existing' | 'template' | 'import';
  getCreateSourcePipelineId: () => string | null;
  getCreateSourceTemplateId: () => string | null;
  collectPipelineOutputs: (plan: PipelineGraphPlan) => Array<{ name: string; dataType?: PipelineDataType | null }>;
  isDaedalusPlan: (plan: PipelineGraphPlan) => boolean;
  RAW_STREAM_PIPELINE_ID: string;
};

type UnknownRecord = Record<string, unknown>;
type LegacyPipelineBinding = {
  pipelineId?: string | null;
  id?: string | null;
  pipeline?: { id?: string | null; graph?: unknown } | null;
  pipelineGraph?: unknown;
  pipeline_graph?: unknown;
};

const asRecord = (value: unknown): UnknownRecord | null =>
  value && typeof value === 'object' ? (value as UnknownRecord) : null;

const readString = (record: UnknownRecord | null, ...keys: string[]): string | null => {
  for (const key of keys) {
    const value = record?.[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return null;
};

const pipelineAssignments = (manifest: StreamManifest): Array<StreamPipelineBinding | LegacyPipelineBinding> => {
  if (Array.isArray(manifest.pipelines)) {
    return manifest.pipelines;
  }
  const legacyAssignments = asRecord(manifest)?.pipelines;
  if (Array.isArray(legacyAssignments)) {
    return legacyAssignments.filter((entry): entry is LegacyPipelineBinding => asRecord(entry) !== null);
  }
  const assignmentsRecord = asRecord(legacyAssignments);
  if (!assignmentsRecord) {
    return [];
  }
  return Object.values(assignmentsRecord).filter((entry): entry is LegacyPipelineBinding => asRecord(entry) !== null);
};

const bindingPipelineId = (binding: StreamPipelineBinding | LegacyPipelineBinding): string | null => {
  const bindingRecord = asRecord(binding);
  return (
    readString(bindingRecord, 'pipeline_id', 'pipelineId', 'id') ??
    readString(asRecord(bindingRecord?.pipeline), 'id')
  );
};

const bindingPipelineGraph = (binding: StreamPipelineBinding | LegacyPipelineBinding): unknown | null => {
  const bindingRecord = asRecord(binding);
  return bindingRecord?.pipeline_graph ?? bindingRecord?.pipelineGraph ?? asRecord(bindingRecord?.pipeline)?.graph ?? null;
};

export const createPipelinePageHelpers = (deps: PipelinePageHelpersDeps) => {
  const pipelineLabelById = (pipelineId: string | null): string => {
    if (!pipelineId) return 'Pipeline';
    const pipeline = deps.getPipelines().find((entry) => entry.id === pipelineId);
    return pipeline?.name ?? 'Pipeline';
  };

  const pipelineForSource = (): PipelineOverviewPipeline | null => {
    if (deps.getCreateMode() !== 'existing') {
      return null;
    }
    const sourceId = deps.getCreateSourcePipelineId();
    if (!sourceId) {
      return null;
    }
    return deps.getPipelines().find((pipeline) => pipeline.id === sourceId) ?? null;
  };

  const templateForSource = (): PipelineTemplateSummary | null => {
    if (deps.getCreateMode() !== 'template') {
      return null;
    }
    const sourceId = deps.getCreateSourceTemplateId();
    if (!sourceId) {
      return null;
    }
    return deps.getTemplates().find((template) => template.templateId === sourceId) ?? null;
  };

  const streamUsesPipeline = (stream: StreamInfo, pipelineId: string): boolean => {
    if (!pipelineId) return false;
    const manifest = stream.manifest ?? null;
    if (!manifest) return false;
    const manifestRecord = asRecord(manifest);
    const direct =
      String(
        manifest.active_pipeline_id ??
          readString(manifestRecord, 'pipeline_id', 'activePipelineId', 'pipelineId') ??
          readString(asRecord(manifestRecord?.pipeline), 'id') ??
          ''
      ).trim();
    if (direct && direct === pipelineId) return true;

    return pipelineAssignments(manifest).some((binding) => bindingPipelineId(binding) === pipelineId);
  };

  const streamLabel = (stream: StreamInfo): string => resolveStreamLabel(stream, 'Stream');

  const streamGraphForPipeline = (stream: StreamInfo, pipelineId: string): unknown | null => {
    const manifest = stream.manifest ?? null;
    if (!manifest) return null;
    const manifestRecord = asRecord(manifest);
    const directId =
      String(
        readString(manifestRecord, 'pipeline_id', 'pipelineId') ??
          ''
      ).trim();
    if (directId && directId === pipelineId) return manifestRecord?.pipeline_graph ?? null;

    const activeId =
      String(
        manifest.active_pipeline_id ??
          readString(manifestRecord, 'activePipelineId') ??
          ''
      ).trim();
    if (activeId && activeId === pipelineId) {
      const direct = manifestRecord?.pipeline_graph ?? null;
      if (direct) return direct;
    }

    for (const binding of pipelineAssignments(manifest)) {
      if (bindingPipelineId(binding) !== pipelineId) {
        continue;
      }
      const graph = bindingPipelineGraph(binding);
      if (graph) {
        return graph;
      }
    }

    return null;
  };

  const outputOptionsForPipeline = (pipelineId: string | null): string[] => {
    if (!pipelineId) return [];
    if (pipelineId === deps.RAW_STREAM_PIPELINE_ID) return ['frame'];
    const pipeline = deps.getPipelines().find((entry) => entry.id === pipelineId) ?? null;
    const plan = pipeline?.graph ?? null;
    if (!plan) return [];
    const boundaryOutputs = deps.collectPipelineOutputs(plan).filter((entry) => entry.name);
    const outputs = boundaryOutputs.map((entry) => entry.name);
    if (outputs.length) {
      const typeMap = Object.fromEntries(boundaryOutputs.map((entry) => [entry.name, entry.dataType]));
      return filterEncoderCompatibleOutputs(outputs, typeMap);
    }

    if (deps.isDaedalusPlan(plan)) {
      const hostOutputs = new Set<string>();
      for (const node of Object.values(plan.nodes ?? {})) {
        const backendId = (node?.backendId ?? '').toLowerCase();
        if (!backendId.endsWith('io.host_output')) continue;
        for (const port of Object.keys(node?.inputs ?? {})) {
          const trimmed = port.trim();
          if (trimmed) hostOutputs.add(trimmed);
        }
      }
      const list = Array.from(hostOutputs).sort((a, b) => a.localeCompare(b));
      // No reliable type information available here; do not guess from port names.
      if (list.length) return list;
    }

    return [];
  };

  return {
    pipelineLabelById,
    pipelineForSource,
    templateForSource,
    streamUsesPipeline,
    streamLabel,
    streamGraphForPipeline,
    outputOptionsForPipeline
  };
};
