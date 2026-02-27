<script lang="ts">
  import type { PageData } from './$types';
  import PipelinePageState from '$lib/features/pipelines/page/PipelinePageState.svelte';
  import PipelinePageSidebar from '$lib/features/pipelines/page/PipelinePageSidebar.svelte';
  import PipelinePageContent from '$lib/features/pipelines/page/PipelinePageContent.svelte';
  import PipelinePageModalsView from '$lib/features/pipelines/page/PipelinePageModalsView.svelte';

  const { data } = $props<{ data: PageData }>();
</script>

<PipelinePageState {data}>
  {#snippet children({ ctx })}
    <div class="flex h-full min-h-0 flex-1 flex-col gap-4 overflow-hidden">
      <div class="flex min-h-0 flex-1 gap-4 overflow-hidden lg:gap-6">
        <PipelinePageSidebar {ctx} />
        <PipelinePageContent {ctx} />
      </div>
    </div>
    <PipelinePageModalsView {ctx} />
  {/snippet}
</PipelinePageState>
