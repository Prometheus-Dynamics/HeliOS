<script lang="ts">
  import PipelineGraphEditor from '$lib/components/flow/PipelineGraphEditor.svelte';
  import type {
    PipelineRegistryEntry,
    PipelineGraphNode,
    PipelineGraphPlan
  } from '$lib/types/pipeline';
  import type { ApiGraphNode } from '$lib/types/pipeline-api';
  import { hydrateGraphWithRegistry } from '$lib/features/pipelines/styleHydration';

  let { entry, variant = 'full' }: { entry: PipelineRegistryEntry; variant?: 'full' | 'compact' | 'tile' } = $props();

  const buildPreviewNode = (source: PipelineRegistryEntry): PipelineGraphNode => ({
    id: source.id,
    backendId: source.id,
    metadata: {
      ...source.metadata,
      tags: source.metadata.tags ?? [],
      categories: source.metadata.categories ?? []
    },
    inputs: source.inputs ?? {},
    outputs: source.outputs ?? {},
    info: {
      id: source.id,
      location: { x: 0, y: 0 },
      values: {}
    },
    sync: null,
    embedded: null,
    source: buildApiGraphNode(source)
  });

  function buildApiGraphNode(source: PipelineRegistryEntry): ApiGraphNode {
    return {
      id: source.id,
      backendId: source.id,
      info: {
        id: source.id,
        location: { x: 0, y: 0 },
        values: {}
      },
      metadata: {
        name: source.metadata.name,
        summary: source.metadata.summary ?? null,
        description: source.metadata.summary ?? null,
        documentation: null,
        provider: source.metadata.provider ?? null,
        tags: source.metadata.tags ?? [],
        categories: source.metadata.categories ?? [],
        state: [],
        input_ports: undefined,
        output_ports: undefined,
        aligned_inputs: undefined,
        schedule: undefined,
        latency_hint: undefined,
        style: source.metadata.style ?? null
      },
      inputs: {},
      outputs: {},
      embedded: null,
      sync: null
    };
  }

  const buildPreviewPlan = (source: PipelineRegistryEntry): PipelineGraphPlan => {
    const node = buildPreviewNode(source);
    return {
      format: 'daedalus',
      nodes: {
        [node.id]: node
      },
      connections: []
    };
  };

  let previewPlan = $state<PipelineGraphPlan>({ format: 'daedalus', nodes: {}, connections: [] });

  $effect(() => {
    const plan = buildPreviewPlan(entry);
    hydrateGraphWithRegistry(plan, [entry]);
    previewPlan = plan;
  });

  const previewTheme = {
    showControls: false
  };

  const COMPACT_SIZE = 96;
  const TILE_SIZE = 240;
  const PREVIEW_CANVAS = 320;
  const COMPACT_SCALE = COMPACT_SIZE / PREVIEW_CANVAS;
  const TILE_SCALE = TILE_SIZE / PREVIEW_CANVAS;

  const containerClass = $derived.by(() => {
    if (variant === 'compact') {
      return 'relative flex h-24 w-24 overflow-hidden rounded-xl border border-surface-900/90 bg-surface-950/80 shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]';
    }
    if (variant === 'tile') {
      return 'relative flex h-[240px] w-[240px] overflow-hidden rounded-2xl border border-surface-900/90 bg-surface-950/80 shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]';
    }
    return 'relative flex aspect-[4/3] min-h-80 w-full overflow-hidden rounded-2xl border border-surface-900/90 bg-surface-950/80 shadow-[inset_0_1px_0_rgba(255,255,255,0.02),0_25px_60px_rgba(2,6,23,0.65)]';
  });
  const editorClass = $derived.by(() =>
    variant === 'compact' || variant === 'tile'
      ? 'pipeline-flow--preview pipeline-flow--preview-compact'
      : 'pipeline-flow--preview'
  );
</script>

<div class={containerClass}>
  {#if variant === 'compact' || variant === 'tile'}
    {@const scaleValue = variant === 'compact' ? COMPACT_SCALE : TILE_SCALE}
    <div
      class="absolute left-0 top-0"
      style={`width:${PREVIEW_CANVAS}px; height:${PREVIEW_CANVAS}px; transform: scale(${scaleValue}); transform-origin: top left;`}
    >
      <PipelineGraphEditor
        plan={previewPlan}
        interactive={false}
        portEditorsMode="never"
        fluid={false}
        height={PREVIEW_CANVAS}
        className={editorClass}
        registryEntries={[entry]}
        metricsHeatmap={null}
        heatmapMode={false}
        diagnostics={null}
        syncInspector={{ enabled: false, focusNodeId: null }}
        theme={previewTheme}
      />
    </div>
  {:else}
    <PipelineGraphEditor
      plan={previewPlan}
      interactive={false}
      portEditorsMode="never"
      fluid={true}
      className={`${editorClass} h-full w-full`}
      registryEntries={[entry]}
      metricsHeatmap={null}
      heatmapMode={false}
      diagnostics={null}
      syncInspector={{ enabled: false, focusNodeId: null }}
      theme={previewTheme}
    />
  {/if}
</div>

<style>
  :global(.pipeline-flow--preview) {
    pointer-events: none !important;
    height: 100%;
  }

  :global(.pipeline-flow--preview .svelteflow__pane),
  :global(.pipeline-flow--preview .svelteflow__edges),
  :global(.pipeline-flow--preview .svelte-flow__node),
  :global(.pipeline-flow--preview .svelte-flow__edge),
  :global(.pipeline-flow--preview .svelte-flow__controls) {
    pointer-events: none !important;
  }

</style>
