<script lang="ts">
  import type { PipelineDetailContext } from '$lib';
  import type {
    PipelineInputQueueConfig,
    PipelineNodeValue,
    PipelineOutputSinkConfig,
    PipelineTypeDescriptor
  } from '$lib/types/pipeline';
  import type { PipelineOutputEntry, PipelinePortEntry } from '$lib/components/pipelines/types';
  import InspectorCard from '$lib/components/pipelines/inspector/InspectorCard.svelte';
  import InspectorEmptyState from '$lib/components/pipelines/inspector/InspectorEmptyState.svelte';
  import InspectorHeader from '$lib/components/pipelines/inspector/InspectorHeader.svelte';
  import BoundaryPorts from './BoundaryPorts.svelte';

  type PortConfigEvent =
    | { direction: 'input'; name: string; config: PipelineInputQueueConfig }
    | { direction: 'output'; name: string; config: PipelineOutputSinkConfig };

  type PortEditEvent = {
    direction: 'input' | 'output';
    nodeId: string;
    name: string;
    oldName?: string;
    dataTypeKey: string;
  };

  const {
    context,
    inputs = [],
    outputs = [],
    onRemovePort,
    onSetPortConfig,
    onSetPortValue,
    onEditPort,
    onAddPort,
    typePalette = {}
  }: {
    context: PipelineDetailContext;
    inputs?: PipelinePortEntry[];
    outputs?: PipelineOutputEntry[];
    onRemovePort?: (payload: { direction: 'input' | 'output'; name: string }) => void;
    onSetPortConfig?: (payload: PortConfigEvent) => void;
    onSetPortValue?: (payload: { direction: 'input' | 'output'; name: string; value: PipelineNodeValue | null }) => void;
    onEditPort?: (payload: PortEditEvent) => void;
    onAddPort?: (payload: { direction: 'input' | 'output'; name: string; dataTypeKey: string }) => void;
    typePalette?: Record<string, PipelineTypeDescriptor>;
  } = $props();

  const pipelineOutputConfigs = $derived.by(() => context.pipeline?.graph.pipelineOutputConfigs ?? {});
</script>

{#if !context.pipeline}
  <InspectorEmptyState message="Select a pipeline to configure pipeline IO ports." />
{:else}
  <InspectorCard>
    <InspectorHeader
      eyebrow="Pipeline IO"
      title="Pipeline IO"
      summary="Manage ingress/egress ports. Add, rename, and configure pipeline IO ports."
    />
    <BoundaryPorts
      inputs={inputs}
      outputs={outputs}
      pipelineOutputConfigs={pipelineOutputConfigs}
      onRemovePort={onRemovePort}
      onSetPortConfig={onSetPortConfig}
      onSetPortValue={onSetPortValue}
      onEditPort={onEditPort}
      onAddPort={onAddPort}
      typePalette={typePalette}
    />
  </InspectorCard>
{/if}
