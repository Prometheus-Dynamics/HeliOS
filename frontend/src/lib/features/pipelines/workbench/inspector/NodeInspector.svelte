<script lang="ts">
  import { onDestroy } from 'svelte';
  import { buildGraphDiagnostics } from '$lib/components/flow/pipeline-graph/editorUtils';
  import InspectorCard from '$lib/components/pipelines/inspector/InspectorCard.svelte';
  import InspectorEmptyState from '$lib/components/pipelines/inspector/InspectorEmptyState.svelte';
  import DaedalusNodeRuntimeEditor from './DaedalusNodeRuntimeEditor.svelte';
  import NodeSummary from './NodeSummary.svelte';
  import NodeMetadata from './NodeMetadata.svelte';
  import NodeInputs from './NodeInputs.svelte';
  import NodeOutputs from './NodeOutputs.svelte';
  import NodeSyncPolicyEditor from './NodeSyncPolicyEditor.svelte';
  import type { PipelineDetailContext } from '$lib';
  import type {
    PipelineNodeSyncConfig,
    PipelineNodeValue,
    PipelineRegistryEntry,
    PipelineTypeDescriptor,
    PipelineOverviewPipeline
  } from '$lib/types/pipeline';
  import type { PipelineGraphDiagnostics } from '$lib/components/flow/pipeline-graph/types';
  import { SvelteURL } from 'svelte/reactivity';

  const {
    context,
    typePalette = {},
    registryEntries = [],
    pipelines = [],
    onSetConstantValue,
    onSetSyncConfig,
    onSetDaedalusNodeRuntime,
    onRelinkExternal,
    onOpenExternal,
    onSetMetadata,
    onAddIoPort,
    onRemoveIoPort
  }: {
    context: PipelineDetailContext;
    typePalette?: Record<string, PipelineTypeDescriptor>;
    registryEntries?: PipelineRegistryEntry[];
    pipelines?: PipelineOverviewPipeline[];
    onSetConstantValue?: (payload: { nodeId: string; port: string; value: PipelineNodeValue | null }) => void;
    onSetSyncConfig?: (payload: { nodeId: string; config: PipelineNodeSyncConfig | null }) => void;
    onSetDaedalusNodeRuntime?: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
    onRelinkExternal?: (payload: { nodeId: string; pipelineId: string }) => Promise<void> | void;
    onOpenExternal?: (payload: { pipelineId: string }) => Promise<void> | void;
    onSetMetadata?: (payload: { nodeId: string; name?: string; summary?: string }) => void;
    onAddIoPort?: (payload: { nodeId: string; name: string; dataTypeKey: string }) => void;
    onRemoveIoPort?: (payload: { nodeId: string; name: string }) => void;
  } = $props();

  const isHostBridgeBackendId = (backendId: string): boolean =>
    backendId === 'io.host_bridge' || backendId.endsWith(':io.host_bridge') || backendId === 'pipeline:input';

  const isHostOutputBackendId = (backendId: string): boolean =>
    backendId === 'io.host_output' || backendId.endsWith(':io.host_output') || backendId === 'pipeline:output';

  const selectedNode = $derived.by(() => {
    if (!context.pipeline || !context.graphSelectionNodeId) {
      return null;
    }
    return context.pipeline.graph.nodes?.[context.graphSelectionNodeId] ?? null;
  });

  const ioNodeContext = $derived.by(() => {
    if (!selectedNode) return null;
    const backendId = (selectedNode.backendId ?? '').trim().toLowerCase();
    const isBridge = isHostBridgeBackendId(backendId);
    const isOutput = isHostOutputBackendId(backendId);
    if (!isBridge && !isOutput) return null;
    const ports = Object.entries(isBridge ? selectedNode.outputs ?? {} : selectedNode.inputs ?? {})
      .map(([name, dataType]) => ({ name, dataType }))
      .sort((a, b) => a.name.localeCompare(b.name));
    const kind: 'input' | 'output' = isBridge ? 'input' : 'output';
    return {
      nodeId: selectedNode.id,
      kind,
      ports
    };
  });

  const isDaedalusGraph = $derived.by(() => {
    const graph = context.pipeline?.graph;
    return Boolean(graph?.format === 'daedalus' || graph?.daedalus);
  });

  let diagnosticsWorker: Worker | null = null;
  let diagnosticsRequestId = 0;
  let diagnosticsLookup = $state<PipelineGraphDiagnostics>({});

  if (typeof Worker !== 'undefined') {
    diagnosticsWorker = new Worker(new SvelteURL('$lib/workers/pipelineDiagnosticsWorker.ts', import.meta.url), { type: 'module' });
    diagnosticsWorker.onmessage = (event) => {
      const payload = event.data as { requestId?: number; map?: PipelineGraphDiagnostics };
      if (!payload || payload.requestId !== diagnosticsRequestId) return;
      diagnosticsLookup = payload.map ?? {};
    };
  }

  onDestroy(() => {
    if (diagnosticsWorker) {
      diagnosticsWorker.terminate();
      diagnosticsWorker = null;
    }
  });

  $effect(() => {
    const diagnostics = context.pipeline?.diagnostics ?? null;
    const graph = context.pipeline?.graph ?? null;
    if (!diagnostics || !graph) {
      diagnosticsLookup = {};
      return;
    }
    if (diagnosticsWorker) {
      diagnosticsRequestId += 1;
      const requestId = diagnosticsRequestId;
      diagnosticsWorker.postMessage({ requestId, graph, diagnostics });
    } else {
      diagnosticsLookup = buildGraphDiagnostics(graph, diagnostics);
    }
  });

  const selectedNodeDiagnostics = $derived.by<PipelineGraphDiagnostics[keyof PipelineGraphDiagnostics] | null>(() => {
    if (!context.graphSelectionNodeId) return null;
    return diagnosticsLookup[context.graphSelectionNodeId] ?? null;
  });
  const nodeWarnings = $derived.by(() => selectedNodeDiagnostics?.nodeMessages ?? []);
  const canEditNodeMetadata = $derived.by(() => Boolean(selectedNode));

  let nodeNameDraft = $state('');
  let nodeSummaryDraft = $state('');

  $effect(() => {
    if (!selectedNode) {
      nodeNameDraft = '';
      nodeSummaryDraft = '';
      return;
    }
    nodeNameDraft = selectedNode.metadata?.name ?? '';
    nodeSummaryDraft = selectedNode.metadata?.summary ?? '';
  });

  const metadataDirty = $derived.by(
    () =>
      (selectedNode?.metadata?.name ?? '') !== (nodeNameDraft ?? '') ||
      (selectedNode?.metadata?.summary ?? '') !== (nodeSummaryDraft ?? '')
  );

  const registryDocSlug = $derived.by(() => {
    if (!selectedNode) return null;
    const entry = registryEntries.find((candidate) => candidate.id === selectedNode.backendId);
    const raw = entry?.metadata.doc;
    if (typeof raw !== 'string') return null;
    const trimmed = raw.trim();
    return trimmed || null;
  });
  const docSlug = $derived.by(() => {
    const raw = selectedNode?.metadata?.doc ?? registryDocSlug;
    if (typeof raw !== 'string') return null;
    const trimmed = raw.trim();
    return trimmed || null;
  });
  const docHref = $derived.by(() => {
    if (!docSlug) return null;
    const normalized = docSlug.replace(/^\/?docs\//i, '').replace(/^\//, '');
    return `/docs/${normalized}`;
  });
  const docBadge = $derived.by(() => (docSlug ? docSlug.replace(/^\/?docs\//i, '').replace(/^\//, '') : null));
  const registrySummary = $derived.by(() =>
    registryEntries.find((candidate) => candidate.id === selectedNode?.backendId)?.metadata.summary ?? null
  );

  const childLinkState = $derived.by(() => {
    if (!selectedNode || selectedNode.backendId?.toLowerCase() !== 'pipeline:child') return null;
    const external = selectedNode.external;
    const alias = external?.alias?.trim() || null;
    const targetId = external?.pipelineId?.trim() || null;
    const warnings = nodeWarnings;
    const hasEmbedded = Boolean(selectedNode.embedded);
    const mismatch = warnings.some((msg) => /mismatch/i.test(msg));
    const unresolved = warnings.some((msg) => /resolve|missing|cycle/i.test(msg));
    const status: 'embedded' | 'linked' | 'mismatch' | 'unresolved' =
      hasEmbedded && !external
        ? 'embedded'
        : mismatch
          ? 'mismatch'
          : unresolved
            ? 'unresolved'
            : 'linked';
    return {
      status,
      alias: alias || targetId,
      targetId,
      hasExternal: Boolean(external),
      hasEmbedded
    };
  });

  const constantsReadOnly = $derived.by(() => Boolean(childLinkState && childLinkState.hasExternal));

  function applyMetadata() {
    if (!context.graphSelectionNodeId || !canEditNodeMetadata || !metadataDirty) return;
    const name = nodeNameDraft?.trim();
    const summary = nodeSummaryDraft?.trim();
    onSetMetadata?.({
      nodeId: context.graphSelectionNodeId,
      name: name || undefined,
      summary: summary || undefined
    });
  }
</script>

{#if selectedNode && isDaedalusGraph && context.graphSelectionNodeId}
  <DaedalusNodeRuntimeEditor
    node={selectedNode}
    nodeId={context.graphSelectionNodeId}
    onChange={(payload) =>
      onSetDaedalusNodeRuntime?.({
        nodeId: payload.nodeId,
        syncGroups: payload.syncGroups
      })}
  />
{/if}

{#if !context.pipeline || !selectedNode}
  <InspectorEmptyState message="Select a node in the graph to view configuration." />
{:else}
  <InspectorCard className="space-y-5">
    <NodeSummary
      {selectedNode}
      registrySummary={registrySummary}
      docHref={docHref}
      docBadge={docBadge}
      nodeWarnings={nodeWarnings}
    />

    <NodeMetadata
      canEditNodeMetadata={canEditNodeMetadata}
      bind:nodeNameDraft={nodeNameDraft}
      bind:nodeSummaryDraft={nodeSummaryDraft}
      metadataDirty={metadataDirty}
      onApply={applyMetadata}
    />

    <NodeOutputs
      {ioNodeContext}
      childLinkState={childLinkState}
      pipelines={pipelines}
      pipelineId={context.pipeline?.id ?? null}
      graphSelectionNodeId={context.graphSelectionNodeId}
      typePalette={typePalette}
      onAddIoPort={onAddIoPort}
      onRemoveIoPort={onRemoveIoPort}
      onRelinkExternal={onRelinkExternal}
      onOpenExternal={onOpenExternal}
    />

    <NodeInputs
      {context}
      {selectedNode}
      registryEntries={registryEntries}
      typePalette={typePalette}
      constantsReadOnly={constantsReadOnly}
      onSetConstantValue={onSetConstantValue}
    />

    {#if !isDaedalusGraph}
      <NodeSyncPolicyEditor context={context} onSetSyncConfig={onSetSyncConfig} />
    {/if}
  </InspectorCard>
{/if}
