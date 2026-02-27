import type { StreamInfo } from '$lib/ts-bindings/http/client';
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
    const direct =
      String(
        (manifest as any)?.active_pipeline_id ??
          (manifest as any)?.pipeline_id ??
          (manifest as any)?.activePipelineId ??
          (manifest as any)?.pipelineId ??
          (manifest as any)?.pipeline?.id ??
          ''
      ).trim();
    if (direct && direct === pipelineId) return true;

    const assignments: any = (manifest as any)?.pipelines ?? null;
    if (Array.isArray(assignments)) {
      return assignments.some((binding: any) => {
        const id = String(binding?.pipeline_id ?? binding?.pipelineId ?? binding?.pipeline?.id ?? '').trim();
        return id === pipelineId;
      });
    }
    if (assignments && typeof assignments === 'object') {
      return Object.values(assignments).some((binding: any) => {
        const id = String(binding?.pipeline_id ?? binding?.pipelineId ?? binding?.pipeline?.id ?? '').trim();
        return id === pipelineId;
      });
    }

    return false;
  };

  const streamLabel = (stream: StreamInfo): string => resolveStreamLabel(stream, 'Stream');

  const streamGraphForPipeline = (stream: StreamInfo, pipelineId: string): unknown | null => {
    const manifest = stream.manifest ?? null;
    if (!manifest) return null;
    const directId =
      String(
        (manifest as any)?.pipeline_id ??
          (manifest as any)?.pipelineId ??
          ''
      ).trim();
    if (directId && directId === pipelineId) return (manifest as any).pipeline_graph ?? null;

    const activeId =
      String(
        (manifest as any)?.active_pipeline_id ??
          (manifest as any)?.activePipelineId ??
          ''
      ).trim();
    if (activeId && activeId === pipelineId) {
      const direct = (manifest as any).pipeline_graph ?? null;
      if (direct) return direct;
    }

    const assignments: any = (manifest as any)?.pipelines ?? null;
    const findInBinding = (binding: any): unknown | null => {
      const id = String(binding?.pipeline_id ?? binding?.pipelineId ?? binding?.pipeline?.id ?? '').trim();
      if (!id || id !== pipelineId) return null;
      return binding?.pipeline_graph ?? binding?.pipelineGraph ?? binding?.pipeline?.graph ?? null;
    };
    if (Array.isArray(assignments)) {
      for (const binding of assignments) {
        const graph = findInBinding(binding);
        if (graph) return graph;
      }
      return null;
    }
    if (assignments && typeof assignments === 'object') {
      for (const binding of Object.values(assignments)) {
        const graph = findInBinding(binding);
        if (graph) return graph;
      }
      return null;
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
