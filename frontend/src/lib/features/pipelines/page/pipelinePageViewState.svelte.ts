import {
  DEFAULT_PIPELINE_COLOR,
  DEFAULT_PIPELINE_ICON_ID
} from '$lib/features/pipelines/iconCatalog';
import { createPipelinePageComponentLoaders } from './pipelinePageSupport';

export type DetailPanelHandle = {
  focusOnNode?: (nodeId: string, options?: { port?: string | null }) => void;
};

export function createPipelinePageViewState(browser: boolean) {
  let detailPanelRef = $state<DetailPanelHandle | null>(null);

  let PipelineDetailPanelComponent = $state<
    (typeof import('$lib/components/pipelines/PipelineDetailPanel.svelte'))['default'] | null
  >(null);
  let PipelineListPanelComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineListPanel.svelte'))['default'] | null
  >(null);
  let PipelineInspectorPanelComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineInspectorPanel.svelte'))['default'] | null
  >(null);
  let PipelineModalsComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineModals.svelte'))['default'] | null
  >(null);
  let PipelineGraphWorkspaceComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineGraphWorkspace.svelte'))['default'] | null
  >(null);
  let PipelineGraphContextMenuComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineGraphContextMenu.svelte'))['default'] | null
  >(null);
  let PipelineTunePanelComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineTunePanel.svelte'))['default'] | null
  >(null);

  let importInput = $state<HTMLInputElement | null>(null);
  let iconModalOpen = $state(false);
  let iconModalPipelineId = $state<string | null>(null);
  let iconModalIconId = $state<string>(DEFAULT_PIPELINE_ICON_ID);
  let iconModalColor = $state<string>(DEFAULT_PIPELINE_COLOR);
  let iconModalSaving = $state(false);
  let iconModalError = $state<string | null>(null);

  const graphBindings = $state({
    get detailPanelRef() {
      return detailPanelRef;
    },
    set detailPanelRef(value: DetailPanelHandle | null) {
      detailPanelRef = value;
    }
  });

  const modalBindings = $state({
    get iconModalIconId() {
      return iconModalIconId;
    },
    set iconModalIconId(value: string) {
      iconModalIconId = value;
    },
    get iconModalColor() {
      return iconModalColor;
    },
    set iconModalColor(value: string) {
      iconModalColor = value;
    },
    get importInput() {
      return importInput;
    },
    set importInput(value: HTMLInputElement | null) {
      importInput = value;
    }
  });

  const loaders = createPipelinePageComponentLoaders({
    browser,
    getPipelineDetailPanel: () => PipelineDetailPanelComponent,
    setPipelineDetailPanel: (component) => {
      PipelineDetailPanelComponent = component;
    },
    getPipelineListPanel: () => PipelineListPanelComponent,
    setPipelineListPanel: (component) => {
      PipelineListPanelComponent = component;
    },
    getPipelineInspectorPanel: () => PipelineInspectorPanelComponent,
    setPipelineInspectorPanel: (component) => {
      PipelineInspectorPanelComponent = component;
    },
    getPipelineModals: () => PipelineModalsComponent,
    setPipelineModals: (component) => {
      PipelineModalsComponent = component;
    },
    getPipelineGraphWorkspace: () => PipelineGraphWorkspaceComponent,
    setPipelineGraphWorkspace: (component) => {
      PipelineGraphWorkspaceComponent = component;
    },
    getPipelineGraphContextMenu: () => PipelineGraphContextMenuComponent,
    setPipelineGraphContextMenu: (component) => {
      PipelineGraphContextMenuComponent = component;
    },
    getPipelineTunePanel: () => PipelineTunePanelComponent,
    setPipelineTunePanel: (component) => {
      PipelineTunePanelComponent = component;
    }
  });

  return {
    graphBindings,
    modalBindings,
    ...loaders,
    get detailPanelRef() {
      return detailPanelRef;
    },
    set detailPanelRef(value: DetailPanelHandle | null) {
      detailPanelRef = value;
    },
    get PipelineDetailPanelComponent() {
      return PipelineDetailPanelComponent;
    },
    get PipelineListPanelComponent() {
      return PipelineListPanelComponent;
    },
    get PipelineInspectorPanelComponent() {
      return PipelineInspectorPanelComponent;
    },
    get PipelineModalsComponent() {
      return PipelineModalsComponent;
    },
    get PipelineGraphWorkspaceComponent() {
      return PipelineGraphWorkspaceComponent;
    },
    get PipelineGraphContextMenuComponent() {
      return PipelineGraphContextMenuComponent;
    },
    get PipelineTunePanelComponent() {
      return PipelineTunePanelComponent;
    },
    get importInput() {
      return importInput;
    },
    set importInput(value: HTMLInputElement | null) {
      importInput = value;
    },
    get iconModalOpen() {
      return iconModalOpen;
    },
    set iconModalOpen(value: boolean) {
      iconModalOpen = value;
    },
    get iconModalPipelineId() {
      return iconModalPipelineId;
    },
    set iconModalPipelineId(value: string | null) {
      iconModalPipelineId = value;
    },
    get iconModalIconId() {
      return iconModalIconId;
    },
    set iconModalIconId(value: string) {
      iconModalIconId = value;
    },
    get iconModalColor() {
      return iconModalColor;
    },
    set iconModalColor(value: string) {
      iconModalColor = value;
    },
    get iconModalSaving() {
      return iconModalSaving;
    },
    set iconModalSaving(value: boolean) {
      iconModalSaving = value;
    },
    get iconModalError() {
      return iconModalError;
    },
    set iconModalError(value: string | null) {
      iconModalError = value;
    }
  };
}
