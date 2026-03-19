<script lang="ts">
  import InspectorHeader from '$lib/components/pipelines/inspector/InspectorHeader.svelte';
  import InspectorSection from '$lib/components/pipelines/inspector/InspectorSection.svelte';
  import type { PipelineGraphNode } from '$lib/types/pipeline';

  const {
    selectedNode = null,
    registrySummary = null,
    docHref = null,
    docBadge = null,
    nodeWarnings = []
  }: {
    selectedNode: PipelineGraphNode | null;
    registrySummary?: string | null;
    docHref?: string | null;
    docBadge?: string | null;
    nodeWarnings?: string[];
  } = $props();
</script>

{#if selectedNode}
  <InspectorHeader
    eyebrow="Node"
    title={selectedNode.metadata?.name || selectedNode.backendId || 'Node'}
    summary={selectedNode.metadata?.summary ?? selectedNode.backendId}
    details={[registrySummary]}
    {docHref}
    docBadge={docBadge}
    size="xl"
  />
{/if}

{#if nodeWarnings.length > 0}
  <InspectorSection title="Warnings" tone="warning">
    <ul class="space-y-1 text-[0.88rem]">
      {#each nodeWarnings as warning, index (`${index}:${warning}`)}
        <li class="leading-snug">{index + 1}. {warning}</li>
      {/each}
    </ul>
  </InspectorSection>
{/if}
