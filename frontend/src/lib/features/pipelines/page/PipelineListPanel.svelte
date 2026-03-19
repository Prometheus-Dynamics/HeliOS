<script lang="ts">
  import type { Writable } from 'svelte/store';
  import type { PipelineOverviewPipeline } from '$lib/types/pipeline';
  import PipelineSidebar from './PipelineSidebar.svelte';

  type PipelineListEntry = {
    id: string;
    name: string;
    revision?: string | null;
    issueCount?: number | null;
    appearance?: unknown;
  };

  type PipelineListPanelProps = {
    customNodeSearch?: string;
    pipelinesRefreshing?: boolean;
    isInitialLoading?: boolean;
    pipelineListItems?: PipelineListEntry[];
    pipelineMap?: Record<string, PipelineOverviewPipeline>;
    selectedPipelineId?: string | null;
    pipelineSearch: Writable<string>;
    onOpenCreateModal?: () => void;
    onSelectPipeline?: (id: string) => void;
    onOpenPipelineIcon?: (id: string) => void;
    onOpenDeleteModal?: (id: string) => void;
    onPipelineCardKeydown?: (event: KeyboardEvent, id: string) => void;
  };

  let {
    customNodeSearch = $bindable(''),
    pipelinesRefreshing = false,
    isInitialLoading = false,
    pipelineListItems = [],
    pipelineMap = {},
    selectedPipelineId = null,
    pipelineSearch,
    onOpenCreateModal,
    onSelectPipeline,
    onOpenPipelineIcon,
    onOpenDeleteModal,
    onPipelineCardKeydown
  }: PipelineListPanelProps = $props();

  export type $$Props = PipelineListPanelProps;
</script>

<PipelineSidebar
  bind:customNodeSearch
  {pipelinesRefreshing}
  {isInitialLoading}
  {pipelineListItems}
  {pipelineMap}
  {selectedPipelineId}
  {pipelineSearch}
  {onOpenCreateModal}
  {onSelectPipeline}
  {onOpenPipelineIcon}
  {onOpenDeleteModal}
  {onPipelineCardKeydown}
/>
