import { subscribeDomainInvalidations } from '$lib/api/invalidation';
import { realtimeUpdateMatchesKind, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
import {
  DEFAULT_PIPELINE_COLOR,
  DEFAULT_PIPELINE_ICON_ID,
  defaultColorForPipeline,
  defaultIconIdForPipeline,
  resolvePipelineIconOption
} from '$lib/features/pipelines/iconCatalog';
import type { PipelineGraphPlan, PipelineOverviewPipeline } from '$lib/types/pipeline';
import { scheduleAfterPaint, scheduleWhenIdle } from '$lib/utils/browserSchedule';

type PipelineDetailPanelComponent =
  (typeof import('$lib/components/pipelines/PipelineDetailPanel.svelte'))['default'];
type PipelineListPanelComponent =
  (typeof import('$lib/features/pipelines/page/PipelineListPanel.svelte'))['default'];
type PipelineInspectorPanelComponent =
  (typeof import('$lib/features/pipelines/page/PipelineInspectorPanel.svelte'))['default'];
type PipelineModalsComponent =
  (typeof import('$lib/features/pipelines/page/PipelineModals.svelte'))['default'];
type PipelineGraphWorkspaceComponent =
  (typeof import('$lib/features/pipelines/page/PipelineGraphWorkspace.svelte'))['default'];
type PipelineGraphContextMenuComponent =
  (typeof import('$lib/features/pipelines/page/PipelineGraphContextMenu.svelte'))['default'];
type PipelineTunePanelComponent =
  (typeof import('$lib/features/pipelines/page/PipelineTunePanel.svelte'))['default'];

type PipelinePageComponentKey =
  | 'detail'
  | 'list'
  | 'inspector'
  | 'modals'
  | 'graph-workspace'
  | 'graph-context'
  | 'tune';

type PipelineFocusRequest = {
  pipelineId: string;
  nodeId?: string | null;
  port?: string | null;
};

type PipelineDetailPanelHandle = {
  focusOnNode?: (nodeId: string, options?: { port?: string | null }) => void;
};

type PipelinePageComponentLoaderDeps = {
  browser: boolean;
  getPipelineDetailPanel: () => PipelineDetailPanelComponent | null;
  setPipelineDetailPanel: (component: PipelineDetailPanelComponent) => void;
  getPipelineListPanel: () => PipelineListPanelComponent | null;
  setPipelineListPanel: (component: PipelineListPanelComponent) => void;
  getPipelineInspectorPanel: () => PipelineInspectorPanelComponent | null;
  setPipelineInspectorPanel: (component: PipelineInspectorPanelComponent) => void;
  getPipelineModals: () => PipelineModalsComponent | null;
  setPipelineModals: (component: PipelineModalsComponent) => void;
  getPipelineGraphWorkspace: () => PipelineGraphWorkspaceComponent | null;
  setPipelineGraphWorkspace: (component: PipelineGraphWorkspaceComponent) => void;
  getPipelineGraphContextMenu: () => PipelineGraphContextMenuComponent | null;
  setPipelineGraphContextMenu: (component: PipelineGraphContextMenuComponent) => void;
  getPipelineTunePanel: () => PipelineTunePanelComponent | null;
  setPipelineTunePanel: (component: PipelineTunePanelComponent) => void;
};

type PipelinePageRuntimeDeps = {
  browser: boolean;
  hasInitialPipelineData: boolean;
  getInitialPipelineId: () => string | null | undefined;
  getSelectedPipelineId: () => string | null;
  getSelectedPipeline: () => PipelineOverviewPipeline | null;
  setSelectedPipeline: (pipelineId: string) => void;
  setActiveTab: (tab: 'pipeline' | 'tune') => void;
  getPendingPipelineFocus: () => PipelineFocusRequest | null;
  setPendingPipelineFocus: (request: PipelineFocusRequest | null) => void;
  getDetailPanelRef: () => PipelineDetailPanelHandle | null;
  loadPipelineOverview: (options?: { bootstrap?: boolean; preserveDirty?: boolean }) => Promise<void>;
  loadStreamCapabilities: () => Promise<void>;
  refreshCaptureDevices: () => Promise<void> | void;
  scheduleRegistryRefresh: () => void;
  loadPipelineModals: () => Promise<void>;
  loadPipelineShellComponents: () => Promise<void>;
  loadPipelineGraphComponents: () => Promise<void>;
  loadPipelineTunePanel: () => Promise<void>;
  getPipelineUiReady: () => boolean;
  setPipelineUiReady: (ready: boolean) => void;
  getActiveTab: () => 'pipeline' | 'tune';
  getRegistryDrawerOpen: () => boolean;
  getIconModalOpen: () => boolean;
  getPluginProjectModalOpen: () => boolean;
  getCreateModalOpen: () => boolean;
  getDeleteModalOpen: () => boolean;
  getAssignModalOpen: () => boolean;
  disposePageStore: () => void;
};

type PipelinePageIconModalDeps = {
  getPipelines: () => PipelineOverviewPipeline[];
  pipelineLabelById: (pipelineId: string) => string;
  setPipelineAppearance: (
    pipelineId: string,
    appearance: { icon: string; color: string }
  ) => Promise<void>;
  buildErrorMessage: (input: { error: unknown; fallback: string }) => string;
  onSaveSuccess: (pipelineLabel: string) => void;
  getIconModalOpen: () => boolean;
  setIconModalOpen: (open: boolean) => void;
  getIconModalPipelineId: () => string | null;
  setIconModalPipelineId: (pipelineId: string | null) => void;
  getIconModalIconId: () => string;
  setIconModalIconId: (iconId: string) => void;
  getIconModalColor: () => string;
  setIconModalColor: (color: string) => void;
  setIconModalSaving: (saving: boolean) => void;
  setIconModalError: (message: string | null) => void;
};

export const PIPELINE_FOCUS_REQUEST_KEY = 'helios.pipelines.focus_request';
const LIVE_UPDATES_REFRESH_DEBOUNCE_MS = 300;

export function graphPlanHasWorkspaceData(plan: PipelineGraphPlan | null | undefined): boolean {
  if (!plan || typeof plan !== 'object') return false;
  if (Object.keys(plan.nodes ?? {}).length > 0) return true;
  if ((plan.connections?.length ?? 0) > 0) return true;
  if (Object.keys(plan.pipelineInputs ?? {}).length > 0) return true;
  if (Object.keys(plan.pipelineOutputs ?? {}).length > 0) return true;
  return false;
}

function shouldApplyLiveUpdate(event: RealtimeUpdateEvent): boolean {
  if (
    event.path.startsWith('/v1/pipelines') ||
    event.path.startsWith('/v1/streams') ||
    event.path.startsWith('/v1/ws/pipelines') ||
    event.path.startsWith('/v1/ws/streams')
  ) {
    return true;
  }
  if (realtimeUpdateMatchesKind(event, 'api')) {
    return false;
  }
  return (
    realtimeUpdateMatchesKind(event, 'pipelines') ||
    realtimeUpdateMatchesKind(event, 'streams') ||
    realtimeUpdateMatchesKind(event, 'device') ||
    realtimeUpdateMatchesKind(event, 'settings')
  );
}

export function createPipelinePageComponentLoaders(deps: PipelinePageComponentLoaderDeps) {
  const componentLoads: Partial<Record<PipelinePageComponentKey, Promise<void>>> = {};

  async function loadComponentOnce(
    key: PipelinePageComponentKey,
    loader: () => Promise<void>
  ): Promise<void> {
    const inFlight = componentLoads[key];
    if (inFlight) {
      return inFlight;
    }
    const next = loader().finally(() => {
      componentLoads[key] = undefined;
    });
    componentLoads[key] = next;
    return next;
  }

  async function loadPipelineDetailPanel(): Promise<void> {
    if (!deps.browser || deps.getPipelineDetailPanel()) return;
    await loadComponentOnce('detail', async () => {
      const module = await import('$lib/components/pipelines/PipelineDetailPanel.svelte');
      deps.setPipelineDetailPanel(module.default);
    });
  }

  async function loadPipelineShellComponents(): Promise<void> {
    await Promise.all([
      deps.getPipelineListPanel()
        ? Promise.resolve()
        : loadComponentOnce('list', async () => {
            const module = await import('$lib/features/pipelines/page/PipelineListPanel.svelte');
            deps.setPipelineListPanel(module.default);
          }),
      deps.getPipelineInspectorPanel()
        ? Promise.resolve()
        : loadComponentOnce('inspector', async () => {
            const module = await import('$lib/features/pipelines/page/PipelineInspectorPanel.svelte');
            deps.setPipelineInspectorPanel(module.default);
          })
    ]);
  }

  async function loadPipelineGraphComponents(): Promise<void> {
    await Promise.all([
      deps.getPipelineGraphWorkspace()
        ? Promise.resolve()
        : loadComponentOnce('graph-workspace', async () => {
            const module = await import('$lib/features/pipelines/page/PipelineGraphWorkspace.svelte');
            deps.setPipelineGraphWorkspace(module.default);
          }),
      deps.getPipelineGraphContextMenu()
        ? Promise.resolve()
        : loadComponentOnce('graph-context', async () => {
            const module = await import(
              '$lib/features/pipelines/page/PipelineGraphContextMenu.svelte'
            );
            deps.setPipelineGraphContextMenu(module.default);
          }),
      loadPipelineDetailPanel()
    ]);
  }

  async function loadPipelineTunePanel(): Promise<void> {
    if (deps.getPipelineTunePanel()) return;
    await loadComponentOnce('tune', async () => {
      const module = await import('$lib/features/pipelines/page/PipelineTunePanel.svelte');
      deps.setPipelineTunePanel(module.default);
    });
  }

  async function loadPipelineModals(): Promise<void> {
    if (deps.getPipelineModals()) return;
    await loadComponentOnce('modals', async () => {
      const module = await import('$lib/features/pipelines/page/PipelineModals.svelte');
      deps.setPipelineModals(module.default);
    });
  }

  return {
    loadPipelineDetailPanel,
    loadPipelineShellComponents,
    loadPipelineGraphComponents,
    loadPipelineTunePanel,
    loadPipelineModals
  };
}

export function createPipelinePageRuntime(deps: PipelinePageRuntimeDeps) {
  let stopLiveUpdates: (() => void) | null = null;
  let liveUpdatesRefreshHandle: number | null = null;
  let cancelPipelineUiBoot: (() => void) | null = null;
  let cancelPipelineBootstrapRefresh: (() => void) | null = null;
  let cancelPipelineIdleWarmup: (() => void) | null = null;
  let cancelPipelineGraphWarmup: (() => void) | null = null;
  let cancelPipelineTuneWarmup: (() => void) | null = null;
  let initialSelectionApplied = false;

  function scheduleLiveUpdatesRefresh(): void {
    if (!deps.browser) return;
    if (liveUpdatesRefreshHandle != null) return;
    liveUpdatesRefreshHandle = window.setTimeout(() => {
      liveUpdatesRefreshHandle = null;
      if (document.hidden) return;
      void deps.loadPipelineOverview({ preserveDirty: true });
      void deps.refreshCaptureDevices();
    }, LIVE_UPDATES_REFRESH_DEBOUNCE_MS);
  }

  function consumePipelineFocusRequest(): void {
    if (!deps.browser) return;
    const raw = window.sessionStorage.getItem(PIPELINE_FOCUS_REQUEST_KEY);
    if (!raw) return;
    window.sessionStorage.removeItem(PIPELINE_FOCUS_REQUEST_KEY);
    try {
      const parsed = JSON.parse(raw) as PipelineFocusRequest;
      if (!parsed?.pipelineId) return;
      deps.setPendingPipelineFocus(parsed);
      deps.setActiveTab('pipeline');
      deps.setSelectedPipeline(parsed.pipelineId);
    } catch {
      // Ignore malformed session handoff data.
    }
  }

  function syncPendingPipelineFocus(): void {
    const request = deps.getPendingPipelineFocus();
    const selectedPipeline = deps.getSelectedPipeline();
    if (!request || !selectedPipeline || selectedPipeline.id !== request.pipelineId) return;
    const nodeId = request.nodeId ?? null;
    if (!nodeId) {
      deps.setPendingPipelineFocus(null);
      return;
    }
    const detailPanel = deps.getDetailPanelRef();
    if (typeof detailPanel?.focusOnNode !== 'function') return;
    detailPanel.focusOnNode(nodeId, { port: request.port ?? null });
    deps.setPendingPipelineFocus(null);
  }

  function syncInitialPipelineSelection(): void {
    if (initialSelectionApplied) return;
    const pipelineId =
      typeof deps.getInitialPipelineId() === 'string' ? deps.getInitialPipelineId()?.trim() : '';
    if (!pipelineId) {
      initialSelectionApplied = true;
      return;
    }
    if (deps.getSelectedPipelineId() === pipelineId) {
      initialSelectionApplied = true;
      return;
    }
    deps.setSelectedPipeline(pipelineId);
    initialSelectionApplied = true;
  }

  function warmShellComponents(): void {
    if (!deps.getPipelineUiReady()) return;
    void deps.loadPipelineShellComponents();
  }

  function warmActiveTabComponents(): void {
    cancelPipelineGraphWarmup?.();
    cancelPipelineGraphWarmup = null;
    cancelPipelineTuneWarmup?.();
    cancelPipelineTuneWarmup = null;
    if (!deps.getPipelineUiReady()) return;
    if (!deps.getSelectedPipeline()) return;
    if (deps.getActiveTab() === 'pipeline') {
      cancelPipelineGraphWarmup = scheduleAfterPaint(() => {
        void deps.loadPipelineGraphComponents();
      }, 1);
      return;
    }
    cancelPipelineTuneWarmup = scheduleAfterPaint(() => {
      void deps.loadPipelineTunePanel();
    }, 1);
  }

  function warmModalComponents(): void {
    if (
      deps.getRegistryDrawerOpen() ||
      deps.getIconModalOpen() ||
      deps.getPluginProjectModalOpen() ||
      deps.getCreateModalOpen() ||
      deps.getDeleteModalOpen() ||
      deps.getAssignModalOpen()
    ) {
      void deps.loadPipelineModals();
    }
  }

  function mount(): void {
    if (!deps.browser) return;
    consumePipelineFocusRequest();
    cancelPipelineUiBoot = scheduleAfterPaint(() => {
      deps.setPipelineUiReady(true);
    }, 1);
    if (deps.hasInitialPipelineData) {
      cancelPipelineBootstrapRefresh = scheduleWhenIdle(() => {
        if (document.hidden) return;
        void deps.loadStreamCapabilities();
      }, { timeoutMs: 1800, fallbackMs: 700 });
    } else {
      cancelPipelineBootstrapRefresh = scheduleAfterPaint(() => {
        void deps.loadStreamCapabilities();
        void deps.loadPipelineOverview({ bootstrap: true, preserveDirty: false });
      }, 2);
    }
    stopLiveUpdates = subscribeDomainInvalidations(
      ['pipelines', 'streams', 'device', 'settings'],
      (event) => {
        if (document.hidden) return;
        if (!shouldApplyLiveUpdate(event)) return;
        scheduleLiveUpdatesRefresh();
      },
      { debounceMs: LIVE_UPDATES_REFRESH_DEBOUNCE_MS }
    );
    cancelPipelineIdleWarmup = scheduleWhenIdle(() => {
      deps.scheduleRegistryRefresh();
      void deps.loadPipelineModals();
    }, { timeoutMs: 2000, fallbackMs: 1200 });
  }

  function destroy(): void {
    cancelPipelineUiBoot?.();
    cancelPipelineUiBoot = null;
    cancelPipelineBootstrapRefresh?.();
    cancelPipelineBootstrapRefresh = null;
    cancelPipelineIdleWarmup?.();
    cancelPipelineIdleWarmup = null;
    cancelPipelineGraphWarmup?.();
    cancelPipelineGraphWarmup = null;
    cancelPipelineTuneWarmup?.();
    cancelPipelineTuneWarmup = null;
    stopLiveUpdates?.();
    stopLiveUpdates = null;
    if (liveUpdatesRefreshHandle != null) {
      clearTimeout(liveUpdatesRefreshHandle);
      liveUpdatesRefreshHandle = null;
    }
    deps.disposePageStore();
  }

  return {
    consumePipelineFocusRequest,
    syncPendingPipelineFocus,
    syncInitialPipelineSelection,
    warmShellComponents,
    warmActiveTabComponents,
    warmModalComponents,
    mount,
    destroy
  };
}

export function handlePipelineCardKeydown(
  event: KeyboardEvent,
  pipelineId: string,
  setSelectedPipeline: (pipelineId: string) => void
): void {
  if (event.key !== 'Enter' && event.key !== ' ') {
    return;
  }
  event.preventDefault();
  setSelectedPipeline(pipelineId);
}

export function createPipelinePageIconModalSupport(deps: PipelinePageIconModalDeps) {
  function openPipelineIconModal(pipelineId: string): void {
    const pipeline = deps.getPipelines().find((entry) => entry.id === pipelineId);
    const appearance = pipeline?.appearance ?? null;
    deps.setIconModalPipelineId(pipelineId);
    const selectedIconId = appearance?.icon ?? defaultIconIdForPipeline(pipelineId);
    const resolvedIcon = resolvePipelineIconOption(selectedIconId);
    deps.setIconModalIconId(resolvedIcon.id);
    deps.setIconModalColor(appearance?.color ?? defaultColorForPipeline(pipelineId));
    deps.setIconModalError(null);
    deps.setIconModalSaving(false);
    deps.setIconModalOpen(true);
  }

  function closePipelineIconModal(): void {
    deps.setIconModalOpen(false);
    deps.setIconModalPipelineId(null);
    deps.setIconModalError(null);
    deps.setIconModalSaving(false);
  }

  async function savePipelineIconSelection(): Promise<void> {
    const pipelineId = deps.getIconModalPipelineId();
    if (!pipelineId) return;
    deps.setIconModalSaving(true);
    deps.setIconModalError(null);
    try {
      const pipelineLabel = deps.pipelineLabelById(pipelineId);
      await deps.setPipelineAppearance(pipelineId, {
        icon: deps.getIconModalIconId(),
        color: deps.getIconModalColor()
      });
      closePipelineIconModal();
      deps.onSaveSuccess(pipelineLabel);
    } catch (error) {
      deps.setIconModalError(
        deps.buildErrorMessage({ error, fallback: 'Failed to update appearance' })
      );
    } finally {
      deps.setIconModalSaving(false);
    }
  }

  return {
    openPipelineIconModal,
    closePipelineIconModal,
    savePipelineIconSelection
  };
}
