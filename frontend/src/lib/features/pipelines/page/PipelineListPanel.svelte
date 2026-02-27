<script lang="ts">
  import type { Writable } from 'svelte/store';
  import type { PipelineOverviewPipeline } from '$lib/types/pipeline';
  import PipelineSidebar from './PipelineSidebar.svelte';

  type PluginListEntry = { name: string; detail: string; description: string };
  type PipelineListEntry = {
    id: string;
    name: string;
    revision?: string | null;
    issueCount?: number | null;
    appearance?: unknown;
  };

  type PipelineListPanelProps = {
    activeTab: 'pipeline' | 'tune';
    customNodeSearch?: string;
    visiblePlugins?: PluginListEntry[];
    pipelinesRefreshing?: boolean;
    isInitialLoading?: boolean;
    pipelineListItems?: PipelineListEntry[];
    pipelineMap?: Record<string, PipelineOverviewPipeline>;
    selectedPipelineId?: string | null;
    pipelineSearch: Writable<string>;
    onOpenPluginProject?: () => void;
    onOpenPluginInIde?: (name: string) => void;
    onOpenCreateModal?: () => void;
    onSelectPipeline?: (id: string) => void;
    onOpenPipelineIcon?: (id: string) => void;
    onOpenDeleteModal?: (id: string) => void;
    onPipelineCardKeydown?: (event: KeyboardEvent, id: string) => void;
  };

  let {
    activeTab,
    customNodeSearch = $bindable(''),
    visiblePlugins = [],
    pipelinesRefreshing = false,
    isInitialLoading = false,
    pipelineListItems = [],
    pipelineMap = {},
    selectedPipelineId = null,
    pipelineSearch,
    onOpenPluginProject,
    onOpenPluginInIde,
    onOpenCreateModal,
    onSelectPipeline,
    onOpenPipelineIcon,
    onOpenDeleteModal,
    onPipelineCardKeydown
  }: PipelineListPanelProps = $props();

  export type $$Props = PipelineListPanelProps;
</script>

<PipelineSidebar
  {activeTab}
  bind:customNodeSearch
  {visiblePlugins}
  {pipelinesRefreshing}
  {isInitialLoading}
  {pipelineListItems}
  {pipelineMap}
  {selectedPipelineId}
  {pipelineSearch}
  {onOpenPluginProject}
  {onOpenPluginInIde}
  {onOpenCreateModal}
  {onSelectPipeline}
  {onOpenPipelineIcon}
  {onOpenDeleteModal}
  {onPipelineCardKeydown}
/>
