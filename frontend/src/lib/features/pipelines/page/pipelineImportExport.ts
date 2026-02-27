import type { PipelineGraphPlan, PipelineOverviewPipeline, PipelineTypeDescriptor } from '$lib/types/pipeline';

export const PIPELINE_EXPORT_VERSION = 1;
export const SUPPORTED_PIPELINE_EXPORT_VERSIONS = [PIPELINE_EXPORT_VERSION];

export type PipelineImportExportDeps = {
  PipelinesApi: {
    uploadGraph: (params: { requestBody: { graph: unknown; name: string } }) => Promise<{ id?: string; graph?: unknown }>;
  };
  applyPaletteToGraphPlan: (plan: PipelineGraphPlan, palette: Record<string, PipelineTypeDescriptor>) => PipelineGraphPlan;
  fromApiGraphPlan: (plan: unknown) => PipelineGraphPlan;
  serializeGraphPlan: (plan: PipelineGraphPlan, options?: { minimal?: boolean }) => unknown;
  refreshPipelineIoCaches: (plan: PipelineGraphPlan) => void;
  loadPipelineOverview: (options: { preserveDirty: boolean; bootstrap?: boolean }) => Promise<void>;
  setSelectedPipeline: (pipelineId: string) => void;
  closeCreateModal: () => void;
  reportError: (params: { title: string; error: unknown; fallback?: string }) => void;
  toaster: { success: (payload: { title: string; description?: string }) => void };
  getDataTypes: () => Record<string, PipelineTypeDescriptor>;
  getSelectedPipeline: () => PipelineOverviewPipeline | null;
  setLoadError: (value: string | null) => void;
  updateDirtyState: (updater: (map: Record<string, boolean>) => Record<string, boolean>) => void;
  updateSaveState: (updater: (map: Record<string, 'idle' | 'saving' | 'error'>) => Record<string, 'idle' | 'saving' | 'error'>) => void;
  updateValidationMessages: (updater: (map: Record<string, { ok: boolean; warnings: string[] }>) => Record<string, { ok: boolean; warnings: string[] }>) => void;
};

export const createPipelineImportExport = (deps: PipelineImportExportDeps) => {
  const handlePipelineImport = async (event: Event): Promise<void> => {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;

    try {
      const text = await file.text();
      const payload = JSON.parse(text) as unknown;
      const payloadObject = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : null;
      const version = typeof payloadObject?.version === 'number' ? payloadObject.version : null;
      const pipelineObject =
        payloadObject && typeof payloadObject.pipeline === 'object' ? (payloadObject.pipeline as Record<string, unknown>) : null;
      const pipelineGraph = pipelineObject?.graph ?? null;
      const templateGraph = payloadObject?.graph ?? null;
      const pipelineName = typeof pipelineObject?.name === 'string' ? pipelineObject.name : null;
      const templateName = typeof payloadObject?.name === 'string' ? payloadObject.name : null;
      let importGraph: unknown | null = null;
      let nameCandidate: string | null = null;

      if (pipelineGraph) {
        if (version != null && !SUPPORTED_PIPELINE_EXPORT_VERSIONS.includes(version)) {
          const versionLabel = String(version);
          throw new Error(
            `Unsupported pipeline export format: expected version ${PIPELINE_EXPORT_VERSION}, got ${versionLabel}. ` +
              `Supported versions: ${SUPPORTED_PIPELINE_EXPORT_VERSIONS.join(', ')}.`
          );
        }
        importGraph = pipelineGraph;
        nameCandidate = pipelineName;
      } else if (templateGraph && typeof templateGraph === 'object' && ('nodes' in templateGraph || 'edges' in templateGraph)) {
        importGraph = templateGraph;
        nameCandidate = templateName;
      } else if (payloadObject && ('nodes' in payloadObject || 'edges' in payloadObject)) {
        importGraph = payloadObject;
      } else {
        const versionLabel = version == null ? 'missing' : String(version);
        throw new Error(
          `Unsupported pipeline export format: expected version ${PIPELINE_EXPORT_VERSION}, got ${versionLabel}. ` +
            `Supported versions: ${SUPPORTED_PIPELINE_EXPORT_VERSIONS.join(', ')}.`
        );
      }
      if (!importGraph) {
        throw new Error('Pipeline export missing graph data');
      }

      const importName = nameCandidate ?? file.name.replace(/\.json$/i, '');
      const finalImportName =
        importName && importName.trim().length ? importName.trim() : `Imported Pipeline ${new Date().toLocaleString()}`;

      const snapshot = await deps.PipelinesApi.uploadGraph({
        requestBody: { graph: importGraph, name: finalImportName }
      });
      const pipelineId = String(snapshot?.id ?? crypto.randomUUID?.() ?? '');
      const palette = deps.getDataTypes();
      const decoratedPlan = deps.applyPaletteToGraphPlan(deps.fromApiGraphPlan(snapshot.graph ?? {}), palette);
      deps.refreshPipelineIoCaches(decoratedPlan);

      deps.updateDirtyState((map) => ({ ...map, [pipelineId]: false }));
      deps.updateSaveState((map) => ({ ...map, [pipelineId]: 'idle' }));
      deps.updateValidationMessages((map) => {
        if (!map[pipelineId]) return map;
        const next = { ...map };
        delete next[pipelineId];
        return next;
      });
      await deps.loadPipelineOverview({ preserveDirty: true });
      deps.setSelectedPipeline(pipelineId);
      deps.setLoadError(null);
      deps.toaster.success({ title: 'Pipeline imported', description: `${finalImportName} created` });
      deps.closeCreateModal();
    } catch (error) {
      console.error('Failed to import pipeline', error);
      deps.reportError({ title: 'Import failed', error });
    } finally {
      if (input) {
        input.value = '';
      }
    }
  };

  const exportCurrentPipeline = async (inlineExternals = false): Promise<void> => {
    const pipeline = deps.getSelectedPipeline();
    if (!pipeline) return;
    try {
      void inlineExternals;
      const payload = {
        version: PIPELINE_EXPORT_VERSION,
        pipeline: {
          id: pipeline.id,
          name: pipeline.name,
          alias: pipeline.alias ?? null,
          createdAt: pipeline.createdAt ?? null,
          updatedAt: pipeline.updatedAt ?? null,
          revision: pipeline.revision ?? null,
          graph: deps.serializeGraphPlan(pipeline.graph, { minimal: true })
        }
      };
      const contents = JSON.stringify(payload, null, 2);
      const blob = new Blob([contents], { type: 'application/json' });
      const anchor = document.createElement('a');
      const safeName = (pipeline.alias ?? pipeline.name ?? 'pipeline')
        .replace(/[^a-z0-9]+/gi, '-')
        .replace(/^-+|-+$/g, '')
        .toLowerCase();
      anchor.href = URL.createObjectURL(blob);
      anchor.download = `${safeName || 'pipeline'}.json`;
      document.body.appendChild(anchor);
      anchor.click();
      document.body.removeChild(anchor);
      URL.revokeObjectURL(anchor.href);
      deps.toaster.success({
        title: 'Pipeline exported',
        description: `${pipeline.name} saved as JSON`
      });
    } catch (error) {
      console.error('Failed to export pipeline', error);
      deps.reportError({ title: 'Export failed', error });
    }
  };

  return {
    handlePipelineImport,
    exportCurrentPipeline
  };
};
