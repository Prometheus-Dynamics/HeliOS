import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';

type PipelineGraphDocument = {
  name?: string;
  graph?: unknown;
};

export const createPipelineTuneUiActions = (options: {
  browser: boolean;
  PIPELINE_UI_METADATA_KEY: string;
  DEFAULT_PIPELINE_UI: PipelineUi;
  getSelectedPipelineId: () => string | null;
  getPipelineUiDraft: () => PipelineUi;
  setPipelineUiDraft: (next: PipelineUi) => void;
  PipelinesApi: {
    fetchGraph: (params: { id: string }) => Promise<unknown>;
    updateGraph: (params: { id: string; requestBody: { name?: string; graph: unknown } }) => Promise<unknown>;
  };
  pipelineLabelById: (pipelineId: string) => string;
  toaster: { success: (payload: { title: string; description?: string }) => void; error: (payload: { title: string; description?: string }) => void };
  buildErrorMessage: (options: { error: unknown; fallback: string }) => string;
  reportError: (options: { title: string; error: unknown; fallback: string }) => void;
}) => {
  const saveTunePipelineUi = async (): Promise<void> => {
    const pipelineId = options.getSelectedPipelineId();
    if (!options.browser || !pipelineId) return;
    try {
      const doc = (await options.PipelinesApi.fetchGraph({ id: pipelineId })) as PipelineGraphDocument;
      const graph = (doc as { graph?: unknown })?.graph as Record<string, unknown> | undefined;
      const graphDoc = graph ?? {};
      if (typeof graphDoc !== 'object' || Array.isArray(graphDoc) || !graphDoc) {
        throw new Error('Pipeline graph is not an object');
      }
      const metadata = (graphDoc as { metadata?: Record<string, unknown> }).metadata ?? {};
      (graphDoc as { metadata: Record<string, unknown> }).metadata = {
        ...metadata,
        [options.PIPELINE_UI_METADATA_KEY]: options.getPipelineUiDraft()
      };
      await options.PipelinesApi.updateGraph({
        id: pipelineId,
        requestBody: { name: doc.name ?? undefined, graph: graphDoc }
      });
      options.toaster.success({ title: 'Pipeline UI saved', description: options.pipelineLabelById(pipelineId) });
    } catch (error) {
      options.reportError({
        title: 'Failed to save Pipeline UI',
        error,
        fallback: options.buildErrorMessage({ error, fallback: 'Unable to save the Pipeline UI.' })
      });
    }
  };

  const resetTunePipelineUi = async (): Promise<void> => {
    const pipelineId = options.getSelectedPipelineId();
    options.setPipelineUiDraft(options.DEFAULT_PIPELINE_UI);
    if (!options.browser || !pipelineId) return;
    try {
      const doc = (await options.PipelinesApi.fetchGraph({ id: pipelineId })) as PipelineGraphDocument;
      const graph = (doc as { graph?: unknown })?.graph as Record<string, unknown> | undefined;
      const graphDoc = graph ?? {};
      if (typeof graphDoc !== 'object' || Array.isArray(graphDoc) || !graphDoc) {
        throw new Error('Pipeline graph is not an object');
      }
      const metadata = { ...((graphDoc as { metadata?: Record<string, unknown> }).metadata ?? {}) };
      delete metadata[options.PIPELINE_UI_METADATA_KEY];
      (graphDoc as { metadata: Record<string, unknown> }).metadata = metadata;
      await options.PipelinesApi.updateGraph({
        id: pipelineId,
        requestBody: { name: doc.name ?? undefined, graph: graphDoc }
      });
      options.toaster.success({ title: 'Pipeline UI reset', description: options.pipelineLabelById(pipelineId) });
    } catch (error) {
      options.reportError({
        title: 'Failed to reset Pipeline UI',
        error,
        fallback: options.buildErrorMessage({ error, fallback: 'Unable to reset the Pipeline UI.' })
      });
    }
  };

  return { saveTunePipelineUi, resetTunePipelineUi };
};
