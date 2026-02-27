import type { Writable } from 'svelte/store';
import type { PipelineOverviewPipeline } from '$lib/types/pipeline';

export type PipelineCrudDeps = {
  getPipelines: () => PipelineOverviewPipeline[];
  getSelectedPipeline: () => PipelineOverviewPipeline | null;
  setSelectedPipeline: (id: string | null) => void;
  loadPipelineOverview: (options?: { preserveDirty?: boolean }) => Promise<void> | void;
  renamePipeline: (pipelineId: string, payload: { name?: string; alias?: string }) => Promise<void> | void;
  PipelinesApi: { deleteGraph: (params: { id: string }) => Promise<void> };
  toaster: { error: (payload: { title: string; description?: string }) => void; success: (payload: { title: string; description?: string }) => void; info?: (payload: { title: string; description?: string }) => void };
  reportError: (params: { title: string; error: unknown; fallback: string }) => void;
  dirtyState: Writable<Record<string, boolean>>;
  saveState: Writable<Record<string, 'idle' | 'saving' | 'error'>>;
  validationMessages: Writable<Record<string, { ok: boolean; warnings: string[] }>>;
  getDeleteModalPipeline: () => PipelineOverviewPipeline | null;
  setDeleteModalPipeline: (pipeline: PipelineOverviewPipeline | null) => void;
  getDeleteModalOpen: () => boolean;
  setDeleteModalOpen: (open: boolean) => void;
  getDeleteModalBusy: () => boolean;
  setDeleteModalBusy: (busy: boolean) => void;
  getDeleteModalError: () => string | null;
  setDeleteModalError: (message: string | null) => void;
};

export const createPipelineCrudActions = (deps: PipelineCrudDeps) => {
  const deletePipelineById = async (pipelineId: string) => {
    const pipeline = deps.getPipelines().find((candidate) => candidate.id === pipelineId) ?? null;
    if (!pipeline) {
      deps.toaster.error({ title: 'Delete failed', description: 'Pipeline not found.' });
      return false;
    }
    try {
      await deps.PipelinesApi.deleteGraph({ id: pipelineId });
      deps.dirtyState.update((map) => {
        if (!map[pipelineId]) return map;
        const next = { ...map };
        delete next[pipelineId];
        return next;
      });
      deps.saveState.update((map) => {
        if (!map[pipelineId]) return map;
        const next = { ...map };
        delete next[pipelineId];
        return next;
      });
      deps.validationMessages.update((map) => {
        if (!map[pipelineId]) return map;
        const next = { ...map };
        delete next[pipelineId];
        return next;
      });
      if (deps.getSelectedPipeline()?.id === pipelineId) {
        deps.setSelectedPipeline(null);
      }
      await deps.loadPipelineOverview({ preserveDirty: true });
      deps.toaster.success({ title: 'Pipeline deleted', description: `${pipeline.name} removed` });
      return true;
    } catch (error) {
      console.error('Failed to delete pipeline', error);
      deps.reportError({ title: 'Delete failed', error, fallback: 'Unable to delete pipeline. Please retry.' });
      return false;
    }
  };

  const openDeleteModal = (pipelineId: string) => {
    const pipeline = deps.getPipelines().find((candidate) => candidate.id === pipelineId) ?? null;
    if (!pipeline) {
      deps.toaster.error({ title: 'Delete failed', description: 'Pipeline not found.' });
      return;
    }
    deps.setDeleteModalPipeline(pipeline);
    deps.setDeleteModalError(null);
    deps.setDeleteModalOpen(true);
  };

  const closeDeleteModal = () => {
    if (deps.getDeleteModalBusy()) return;
    deps.setDeleteModalOpen(false);
    deps.setDeleteModalPipeline(null);
    deps.setDeleteModalError(null);
  };

  const confirmDeletePipeline = async () => {
    const pipeline = deps.getDeleteModalPipeline();
    if (!pipeline) return;
    deps.setDeleteModalBusy(true);
    deps.setDeleteModalError(null);
    const success = await deletePipelineById(pipeline.id);
    deps.setDeleteModalBusy(false);
    if (success) {
      deps.setDeleteModalOpen(false);
      deps.setDeleteModalPipeline(null);
    } else {
      deps.setDeleteModalError('Unable to delete pipeline. Please retry.');
    }
  };

  const handlePipelineRename = (event: CustomEvent<{ pipelineId: string; name: string; alias: string }>) => {
    const pipeline = deps.getSelectedPipeline();
    if (!pipeline || event.detail.pipelineId !== pipeline.id) {
      return;
    }
    const nextName = event.detail.name?.trim() ?? '';
    const nextAlias = event.detail.alias?.trim() ?? '';
    const payload: { name?: string; alias?: string } = {};
    if (nextName && nextName !== pipeline.name) {
      payload.name = nextName;
    }
    if (nextAlias !== pipeline.alias) {
      payload.alias = nextAlias;
    }
    if (!payload.name && !payload.alias) {
      return;
    }
    void deps.renamePipeline(pipeline.id, payload);
  };

  return {
    deletePipelineById,
    openDeleteModal,
    closeDeleteModal,
    confirmDeletePipeline,
    handlePipelineRename
  };
};
