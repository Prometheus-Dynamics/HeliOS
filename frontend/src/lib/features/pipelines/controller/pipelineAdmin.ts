import { browser } from '$app/environment';
import { get, type Readable, type Writable } from 'svelte/store';
import { toaster } from '$lib';
import { StreamsApi } from '$lib/api/streamsApi';
import { fetchPipelinePagePayload } from '$lib/api/pipelinesPayload';
import type {
  PipelineGraphPlan,
  PipelineOverviewPipeline,
  PipelinePagePayload,
  PipelineRegistryEntry,
  PipelineTemplateSummary,
  PipelineTypeDescriptor
} from '$lib/types/pipeline';
import type { StreamInfo } from '$lib/api/client';
import { applyPaletteToGraphPlan, serializeGraphPlan } from '../graph';
import { refreshPipelineIoCaches } from '../boundary';
import { fromApiGraphPlan } from '../graphConverters';
import { hydrateGraphWithRegistry } from '../styleHydration';
import { fetchPipelineTemplate, listCaptureBackends, uploadPipelineGraph } from './io';
import { describeError } from './utils';
import { reportError } from '$lib/ui/errorPolicy';

type CreateMode = 'blank' | 'existing' | 'import' | 'template';

type PipelineAdminDeps = {
  pipelines: Readable<PipelineOverviewPipeline[]>;
  templates: Readable<PipelineTemplateSummary[]>;
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  selectedPipelineId: Writable<string | null>;
  pipelineSearch: Writable<string>;
  loadError: Writable<string | null>;
  dirtyState: Writable<Record<string, boolean>>;
  createModalOpen: Writable<boolean>;
  createName: Writable<string>;
  createMode: Writable<CreateMode>;
  createSourcePipelineId: Writable<string | null>;
  createSourceTemplateId: Writable<string | null>;
  createBusy: Writable<boolean>;
  createError: Writable<string | null>;
  assignModalOpen: Writable<boolean>;
  captureDevices: Writable<StreamInfo[]>;
  selectedCaptureSessionId: Writable<string | null>;
  assignBusy: Writable<boolean>;
  assignError: Writable<string | null>;
  getRegistryEntries: () => PipelineRegistryEntry[];
  getDataTypes: () => Record<string, PipelineTypeDescriptor>;
  setAllPipelines: (next: PipelineOverviewPipeline[]) => void;
  updatePipeline: (pipelineId: string, mutator: (pipeline: PipelineOverviewPipeline) => void) => void;
  applyPipelinePayload: (payload: PipelinePagePayload, options: { preserveDirty: boolean }) => void;
  ensurePipelineGraph: (pipelineId: string) => void | Promise<void>;
  reloadPipelineGraph: (pipelineId: string) => void | Promise<void>;
  isStubOverviewGraph: (graph: PipelineGraphPlan) => boolean;
};

export function createPipelineAdminActions(deps: PipelineAdminDeps) {
  let pipelineRefreshPromise: Promise<void> | null = null;

  function pipelineForSource(): PipelineOverviewPipeline | null {
    if (get(deps.createMode) !== 'existing') return null;
    const sourceId = get(deps.createSourcePipelineId);
    if (!sourceId) return null;
    return get(deps.pipelines).find((pipeline) => pipeline.id === sourceId) ?? null;
  }

  function templateForSource(): PipelineTemplateSummary | null {
    if (get(deps.createMode) !== 'template') return null;
    const sourceId = get(deps.createSourceTemplateId);
    if (!sourceId) return null;
    return get(deps.templates).find((template) => template.templateId === sourceId) ?? null;
  }

  function openCreateModal() {
    deps.createModalOpen.set(true);
    deps.createName.set('');
    deps.createMode.set('blank');
    deps.createSourcePipelineId.set(get(deps.pipelines)[0]?.id ?? null);
    deps.createSourceTemplateId.set(get(deps.templates)[0]?.templateId ?? null);
    deps.createError.set(null);
  }

  function closeCreateModal() {
    if (get(deps.createBusy)) return;
    deps.createModalOpen.set(false);
  }

  async function createPipeline() {
    if (get(deps.createMode) === 'import') return;

    const name = get(deps.createName).trim();
    const requestedName = name.length ? name : null;
    deps.createBusy.set(true);
    deps.createError.set(null);
    try {
      const mode = get(deps.createMode);
      let baseGraph: unknown = { nodes: [], edges: [], metadata: {} };
      if (mode === 'existing') {
        const source = pipelineForSource();
        if (!source) {
          throw new Error('Source pipeline not found.');
        }
        baseGraph = serializeGraphPlan(source.graph, { minimal: true });
      }
      if (mode === 'template') {
        const source = templateForSource();
        if (!source) {
          throw new Error('Template not found.');
        }
        const templateDoc = await fetchPipelineTemplate(source.templateId);
        baseGraph = templateDoc.graph ?? { nodes: [], edges: [], metadata: {} };
      }

      const response = await uploadPipelineGraph(baseGraph, requestedName);
      const graphPlan = fromApiGraphPlan(response.graph ?? {});
      hydrateGraphWithRegistry(graphPlan, deps.getRegistryEntries());
      const decorated = applyPaletteToGraphPlan(graphPlan, deps.getDataTypes());
      refreshPipelineIoCaches(decorated);
      const createdAt = Math.floor((response.updated_at_ms ?? Date.now()) / 1000);
      const responseName =
        typeof response?.name === 'string' && response.name.trim().length ? response.name.trim() : null;
      const displayName = responseName ?? requestedName ?? 'Pipeline';
      const pipeline: PipelineOverviewPipeline = {
        id: response.id,
        name: displayName,
        alias: displayName,
        status: 'draft',
        revision: response.updated_at_ms ? String(response.updated_at_ms) : null,
        planHash: null,
        createdAt,
        updatedAt: createdAt,
        graph: decorated,
        attachments: [],
        diagnostics: null
      };
      const next = [...get(deps.pipelines), pipeline].sort((a, b) => a.name.localeCompare(b.name));
      deps.setAllPipelines(next);
      deps.selectedPipelineId.set(pipeline.id);
      deps.createModalOpen.set(false);
      toaster.success({
        title: 'Pipeline created',
        description: `${pipeline.name} is ready to edit.`
      });
    } catch (error) {
      console.error('Failed to create pipeline', error);
      deps.createError.set(describeError(error));
    } finally {
      deps.createBusy.set(false);
    }
  }

  function openAssignModal() {
    deps.assignError.set(null);
    deps.selectedCaptureSessionId.set(null);
    deps.assignModalOpen.set(true);
    void refreshCaptureDevices();
  }

  function closeAssignModal() {
    if (get(deps.assignBusy)) return;
    deps.assignModalOpen.set(false);
  }

  function runPipelineRefresh(options: { preserveDirty?: boolean } = {}) {
    if (!pipelineRefreshPromise) {
      pipelineRefreshPromise = refreshPipelineOverview(options).finally(() => {
        pipelineRefreshPromise = null;
      });
    }
    return pipelineRefreshPromise;
  }

  async function refreshPipelineOverview(options: { preserveDirty?: boolean } = {}) {
    if (!browser) return;
    try {
      const preserveDirty = options.preserveDirty ?? true;
      const previousPipelinesById = new Map(get(deps.pipelines).map((pipeline) => [pipeline.id, pipeline]));
      const previousDirtyState: Record<string, boolean> = preserveDirty ? get(deps.dirtyState) : {};
      const payload = await fetchPipelinePagePayload();
      deps.applyPipelinePayload(payload, { preserveDirty });
      const activePipelineId = get(deps.selectedPipelineId);
      if (activePipelineId) {
        const previousPipeline = previousPipelinesById.get(activePipelineId) ?? null;
        const payloadPipeline = payload.pipelines.find((pipeline) => pipeline.id === activePipelineId) ?? null;
        const revisionChanged = (previousPipeline?.revision ?? null) !== (payloadPipeline?.revision ?? null);
        const shouldReloadSelectedPipeline =
          preserveDirty &&
          !previousDirtyState[activePipelineId] &&
          Boolean(previousPipeline && !deps.isStubOverviewGraph(previousPipeline.graph)) &&
          Boolean(payloadPipeline && deps.isStubOverviewGraph(payloadPipeline.graph)) &&
          revisionChanged;
        if (shouldReloadSelectedPipeline) {
          void deps.reloadPipelineGraph(activePipelineId);
        } else {
          void deps.ensurePipelineGraph(activePipelineId);
        }
      }
    } catch (error) {
      console.error('Failed to refresh pipeline overview', error);
      deps.loadError.set(describeError(error));
    }
  }

  function refreshPipelines(options: { preserveDirty?: boolean } = {}) {
    return runPipelineRefresh(options);
  }

  async function refreshCaptureDevices() {
    if (!get(deps.selectedPipeline)) return;
    deps.assignBusy.set(true);
    try {
      const response = await listCaptureBackends();
      deps.captureDevices.set(Array.isArray(response) ? response : []);
    } catch (error) {
      console.error('Failed to fetch capture devices', error);
      const message = describeError(error);
      deps.assignError.set(message);
      toaster.error({
        title: 'Failed to load capture devices',
        description: message
      });
    } finally {
      deps.assignBusy.set(false);
    }
  }

  async function attachPipelineToDevice() {
    const pipeline = get(deps.selectedPipeline);
    const streamId = get(deps.selectedCaptureSessionId);
    if (!pipeline || !streamId) return;
    if (get(deps.assignBusy)) return;

    deps.assignBusy.set(true);
    deps.assignError.set(null);
    try {
      const graph = serializeGraphPlan(pipeline.graph);
      await StreamsApi.setPipelineGraph({
        id: streamId,
        requestBody: { graph, pipeline_id: pipeline.id, output: null }
      });
      const smoke = await StreamsApi.smokePipelineGraph({ id: streamId, timeoutMs: 1500 });
      if (!smoke.ok) {
        const message =
          Array.isArray(smoke.errors) && smoke.errors.length
            ? smoke.errors.join('\n')
            : 'Pipeline runtime error.';
        reportError({
          title: 'Pipeline runtime error',
          error: new Error(message),
          fallback: message
        });
      }
      toaster.success({
        title: 'Pipeline applied',
        description: `Assigned ${pipeline.name} to stream ${streamId.slice(0, 8)}`
      });
      try {
        const updatedStreams = await listCaptureBackends();
        deps.captureDevices.set(Array.isArray(updatedStreams) ? updatedStreams : []);
      } catch (error) {
        console.error('Failed to refresh capture devices after assignment', error);
      }
      try {
        await refreshPipelineOverview({ preserveDirty: true });
      } catch (error) {
        console.error('Failed to refresh pipelines after assignment', error);
      }
      deps.assignModalOpen.set(false);
      deps.selectedCaptureSessionId.set(null);
    } catch (error) {
      console.error('Failed to assign pipeline', error);
      const message = describeError(error);
      deps.assignError.set(message);
      reportError({
        title: 'Failed to assign pipeline',
        error,
        fallback: message
      });
    } finally {
      deps.assignBusy.set(false);
    }
  }

  async function detachAttachment() {
    toaster.error({
      title: 'Not supported',
      description: 'Detaching pipelines is disabled with the new pipeline API.'
    });
  }

  async function setPipelineAppearance(pipelineId: string, payload: { icon?: string; color?: string }) {
    if (!pipelineId) return;
    deps.updatePipeline(pipelineId, (next) => {
      const appearance = { ...(next.appearance ?? {}) };
      if (payload.icon !== undefined) {
        appearance.icon = payload.icon ?? null;
      }
      if (payload.color !== undefined) {
        appearance.color = payload.color ?? null;
      }
      next.appearance = appearance;
    });
  }

  function slugifyPipelineAlias(value: string): string {
    const lower = value.toLowerCase();
    let slug = '';
    let pendingDash = false;
    for (const char of lower) {
      if ((char >= 'a' && char <= 'z') || (char >= '0' && char <= '9')) {
        if (pendingDash && slug.length > 0) {
          slug += '-';
        }
        pendingDash = false;
        slug += char;
      } else {
        pendingDash = true;
      }
    }
    slug = slug.replace(/^-+|-+$/g, '');
    if (!slug) {
      const random =
        typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
          ? crypto.randomUUID().slice(0, 8)
          : Math.random().toString(16).slice(2, 10);
      return `pipeline-${random}`;
    }
    return slug;
  }

  async function renamePipeline(pipelineId: string, payload: { name?: string; alias?: string }) {
    if (!pipelineId) return;
    const trimmedName = payload.name?.trim();
    const trimmedAlias = payload.alias?.trim();
    if (!trimmedName && !trimmedAlias) return;

    const normalizedAlias = trimmedAlias ? slugifyPipelineAlias(trimmedAlias) : '';
    deps.updatePipeline(pipelineId, (next) => {
      if (trimmedName) {
        next.name = trimmedName;
      }
      if (payload.alias !== undefined) {
        next.alias = normalizedAlias;
      }
    });
    toaster.success({
      title: 'Pipeline updated',
      description:
        payload.alias !== undefined && trimmedName
          ? 'Name and alias saved.'
          : trimmedName
            ? 'Name saved.'
            : 'Alias saved.'
    });
  }

  function setSelectedCaptureSession(sessionId: string | null) {
    deps.selectedCaptureSessionId.set(sessionId);
  }

  function setPipelineSearch(value: string) {
    deps.pipelineSearch.set(value);
  }

  return {
    openCreateModal,
    closeCreateModal,
    pipelineForSource,
    templateForSource,
    createPipeline,
    openAssignModal,
    closeAssignModal,
    refreshPipelines,
    refreshCaptureDevices,
    attachPipelineToDevice,
    detachAttachment,
    setPipelineAppearance,
    renamePipeline,
    setSelectedCaptureSession,
    setPipelineSearch
  };
}
