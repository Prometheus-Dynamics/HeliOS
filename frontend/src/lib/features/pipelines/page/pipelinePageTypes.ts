import type { ComponentProps, Snippet } from 'svelte';
import type { Readable, Writable } from 'svelte/store';
import type { PipelineOverviewPipeline } from '$lib/types/pipeline';
import type PipelineGraphContextMenu from './PipelineGraphContextMenu.svelte';
import type PipelineGraphWorkspace from './PipelineGraphWorkspace.svelte';
import type PipelineInspectorPanel from './PipelineInspectorPanel.svelte';
import type PipelineModals from './PipelineModals.svelte';
import type PipelineTunePanel from './PipelineTunePanel.svelte';
import type { PipelineTuneWorkspaceContext } from './pipelineTuneWorkspaceContext.svelte';

type PipelineGraphWorkspaceProps = ComponentProps<typeof PipelineGraphWorkspace>;
type PipelineInspectorPanelProps = ComponentProps<typeof PipelineInspectorPanel>;
type PipelineModalsProps = ComponentProps<typeof PipelineModals>;

type PipelineListEntry = {
  id: string;
  name: string;
  revision?: string | null;
  issueCount?: number | null;
  appearance?: unknown;
};

export type PipelinePageSidebarContext = {
  PipelineListPanel: (typeof import('./PipelineListPanel.svelte'))['default'] | null;
  ideBindings: { customNodeSearch: string };
  pipelinesRefreshing: boolean;
  isInitialLoading: boolean;
  pipelineListItems: Readable<PipelineListEntry[]>;
  pipelineMap: Readable<Record<string, PipelineOverviewPipeline>>;
  selectedPipelineId: Readable<string | null>;
  pipelineSearch: Writable<string>;
  openCreateModal: () => void;
  setSelectedPipeline: (pipelineId: string) => void;
  openPipelineIconModal: (pipelineId: string) => void;
  handlePipelineCardKeydown: (event: KeyboardEvent, pipelineId: string) => void;
  openDeleteModal?: (id: string) => void;
};

export type PipelinePageContentBaseContext = {
  PipelineDetailPanelComponent: PipelineGraphWorkspaceProps['detailPanelComponent'];
  PipelineGraphContextMenu: typeof PipelineGraphContextMenu | null;
  PipelineGraphWorkspace: typeof PipelineGraphWorkspace | null;
  PipelineInspectorPanel: typeof PipelineInspectorPanel | null;
  PipelineTunePanel: typeof PipelineTunePanel | null;
  activeTab: Writable<PipelineInspectorPanelProps['activeTab']>;
  loadError: Readable<PipelineGraphWorkspaceProps['loadError']>;
  registryLoading: Readable<PipelineGraphWorkspaceProps['registryLoading']>;
  registryError: Readable<PipelineGraphWorkspaceProps['registryError']>;
  registry: Readable<PipelineGraphWorkspaceProps['registryEntries']>;
  detailContext: Readable<PipelineGraphWorkspaceProps['detailContext']>;
  editingPlan: Readable<PipelineGraphWorkspaceProps['editingPlan']>;
  editingBreadcrumbs: Readable<PipelineGraphWorkspaceProps['editingBreadcrumbs']>;
  dataTypes: Readable<PipelineGraphWorkspaceProps['dataTypes']>;
  pipelines: Readable<PipelineGraphWorkspaceProps['pipelines']>;
  pipelineInputEntries: Readable<PipelineGraphWorkspaceProps['pipelineInputEntries']>;
  pipelineOutputEntries: Readable<PipelineGraphWorkspaceProps['pipelineOutputEntries']>;
  inspectorTab: Readable<PipelineGraphWorkspaceProps['inspectorTab']>;
  captureDevices: Readable<PipelineGraphWorkspaceProps['captureDevices']>;
  selectedPipeline: Readable<PipelineGraphWorkspaceProps['selectedPipeline']>;
};

type RegistryFilterUpdate = {
  search?: string;
  tag?: string | null;
  category?: string | null;
  provider?: string | null;
};

export type PipelinePageModalsViewContext = {
  PipelineModals: typeof PipelineModals | null;
  assignBusy: Readable<boolean>;
  assignError: Readable<PipelineModalsProps['assignError']>;
  assignModalOpen: Readable<boolean>;
  addNodeFromRegistry: NonNullable<PipelineModalsProps['onAddRegistryEntry']>;
  attachPipelineToDevice: NonNullable<PipelineModalsProps['onAttachPipeline']>;
  captureDevices: Readable<PipelineModalsProps['captureDevices']>;
  closeAssignModal: NonNullable<PipelineModalsProps['onCloseAssign']>;
  closeCreateModal: NonNullable<PipelineModalsProps['onCloseCreate']>;
  closeDeleteModal: NonNullable<PipelineModalsProps['onCloseDelete']>;
  closePipelineIconModal: NonNullable<PipelineModalsProps['onCloseIconModal']>;
  closePluginProjectModal: NonNullable<PipelineModalsProps['onClosePluginProject']>;
  confirmDeletePipeline: NonNullable<PipelineModalsProps['onConfirmDelete']>;
  createPluginProject: NonNullable<PipelineModalsProps['onCreatePluginProject']>;
  createBusy: Readable<boolean>;
  createError: Readable<PipelineModalsProps['createError']>;
  createModalOpen: Readable<boolean>;
  createMode: PipelineModalsProps['createMode'];
  createName: PipelineModalsProps['createName'];
  createPipeline: NonNullable<PipelineModalsProps['onCreate']>;
  createSourcePipelineId: PipelineModalsProps['createSourcePipelineId'];
  createSourceTemplateId: PipelineModalsProps['createSourceTemplateId'];
  deleteModalBusy: Readable<boolean>;
  deleteModalError: Readable<PipelineModalsProps['deleteModalError']>;
  deleteModalOpen: Readable<boolean>;
  deleteModalPipeline: Readable<PipelineModalsProps['deleteModalPipeline']>;
  handlePipelineImport: NonNullable<PipelineModalsProps['onFileChange']>;
  iconModalError: PipelineModalsProps['iconModalError'];
  iconModalOpen: boolean;
  iconModalPipelineId: string | null;
  iconModalSaving: boolean;
  ideBindings: {
    pluginProjectLanguage: string;
    pluginProjectName: string;
  };
  modalBindings: {
    iconModalColor: string;
    iconModalIconId: string;
    importInput: HTMLInputElement | null;
  };
  pipelineForSource: PipelineModalsProps['pipelineForSource'];
  pipelineLabelById: (pipelineId: string) => string;
  pipelines: Readable<PipelineModalsProps['pipelines']>;
  pluginProjectBusy: boolean;
  pluginProjectError: PipelineModalsProps['pluginProjectError'];
  pluginProjectModalOpen: boolean;
  refreshRegistry: NonNullable<PipelineModalsProps['onRefreshRegistry']>;
  registryDrawerOpen: Writable<boolean>;
  registrySort: Writable<NonNullable<Parameters<NonNullable<PipelineModalsProps['onChangeRegistrySort']>>[0]>>;
  registryStores: PipelineModalsProps['registryStores'];
  registryView: Writable<NonNullable<Parameters<NonNullable<PipelineModalsProps['onChangeRegistryView']>>[0]>>;
  resetRegistryFilters: NonNullable<PipelineModalsProps['onResetRegistry']>;
  savePipelineIconSelection: NonNullable<PipelineModalsProps['onSaveIcon']>;
  selectRegistryGroup: (group: string) => void;
  selectedCaptureSessionId: Readable<PipelineModalsProps['selectedCaptureSessionId']>;
  selectedPipeline: Readable<PipelineModalsProps['selectedPipeline']>;
  setSelectedCaptureSession: NonNullable<PipelineModalsProps['onSelectCaptureSession']>;
  templateForSource: PipelineModalsProps['templateForSource'];
  templates: Readable<PipelineModalsProps['templates']>;
  triggerPipelineImport: NonNullable<PipelineModalsProps['onTriggerImport']>;
  updateRegistryFilters: (update: RegistryFilterUpdate) => void;
};

export type PipelinePageBaseContext = PipelinePageSidebarContext &
  PipelinePageContentBaseContext &
  PipelinePageModalsViewContext;

export type PipelinePageRouteChildrenArgs = {
  base: PipelinePageBaseContext;
  tune: PipelineTuneWorkspaceContext;
};

export type PipelinePageRouteChildrenSnippet = Snippet<[PipelinePageRouteChildrenArgs]>;
