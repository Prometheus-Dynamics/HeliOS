import type { StreamInfo, StreamManifest, StreamPipelineBinding } from '$lib/api/client';
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

const pipelineAssignments = (manifest: StreamManifest): StreamPipelineBinding[] =>
  Array.isArray(manifest.pipelines) ? manifest.pipelines : [];

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
    if (manifest.active_pipeline_id?.trim() === pipelineId) return true;
    return pipelineAssignments(manifest).some((binding) => binding.pipeline_id.trim() === pipelineId);
  };

  const streamLabel = (stream: StreamInfo): string => resolveStreamLabel(stream, 'Stream');

  const streamGraphForPipeline = (stream: StreamInfo, pipelineId: string): unknown | null => {
    const manifest = stream.manifest ?? null;
    if (!manifest) return null;
    for (const binding of pipelineAssignments(manifest)) {
      if (binding.pipeline_id.trim() !== pipelineId) {
        continue;
      }
      const graph = binding.pipeline_graph ?? null;
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
