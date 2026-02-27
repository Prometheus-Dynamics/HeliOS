<script lang="ts">
  import type { PipelineDetailContext } from '$lib';
  import type { ChannelPolicy } from '$lib/types/pipeline';
  import type { GraphEdgeSelection } from '$lib/components/pipelines/types';
  import {
    connectionSignature
  } from '$lib/components/flow/pipeline-graph/utils';
  import { buildEdgeId } from '$lib/components/flow/pipeline-graph/edges';
  import {
    DEFAULT_CONNECTION_STYLE,
    normalizeConnectionStyle
  } from '$lib/components/flow/pipeline-graph/edgeStyle';
  import type {
    PipelineConnectionRoute,
    PipelineConnectionStyle
  } from '$lib/types/pipeline';
  import { channelPolicyOptions } from './channelPolicies';

  const {
    context,
    onSetPolicy,
    onSetStyle
  }: {
    context: PipelineDetailContext;
    onSetPolicy?: (payload: { connection: GraphEdgeSelection; policy: ChannelPolicy }) => void;
    onSetStyle?: (payload: { connection: GraphEdgeSelection; style: PipelineConnectionStyle }) => void;
  } = $props();

  const pipeline = context.pipeline;
  const selectedEdgeId = context.graphSelectionEdgeId;
  const selectedEdge =
    pipeline && selectedEdgeId
      ? pipeline.graph.connections?.find(
          (conn, index) =>
            connectionSignature(conn) === selectedEdgeId || buildEdgeId(conn, index) === selectedEdgeId
        ) ?? null
      : null;

  function handlePolicyChange(event: Event) {
    if (!selectedEdge) return;
    const target = event.currentTarget as HTMLSelectElement;
    const value = target.value as ChannelPolicy;
    onSetPolicy?.({
      connection: {
        id: connectionSignature(selectedEdge),
        from: { node: selectedEdge.from.node, port: selectedEdge.from.port, dataType: undefined },
        to: { node: selectedEdge.to.node, port: selectedEdge.to.port, dataType: undefined }
      },
      policy: value
    });
  }

  const edgeDescription = $derived.by(() => {
    if (!selectedEdge) return '';
    return `${selectedEdge.from.node}.${selectedEdge.from.port} → ${selectedEdge.to.node}.${selectedEdge.to.port}`;
  });

  const edgePolicy = $derived.by(() => selectedEdge?.policy ?? 'NewestWins');

  const resolvedStyle = $derived.by(() =>
    normalizeConnectionStyle(selectedEdge?.style ?? DEFAULT_CONNECTION_STYLE)
  );

  const updateStyle = (patch: Partial<PipelineConnectionStyle>) => {
    if (!selectedEdge || !onSetStyle) return;
    const nextStyle: PipelineConnectionStyle = {
      ...(selectedEdge.style ?? {}),
      ...patch
    };
    onSetStyle({
      connection: {
        id: connectionSignature(selectedEdge),
        from: { node: selectedEdge.from.node, port: selectedEdge.from.port, dataType: undefined },
        to: { node: selectedEdge.to.node, port: selectedEdge.to.port, dataType: undefined }
      },
      style: nextStyle
    });
  };

  const curveLabel = $derived.by(() => {
    if (resolvedStyle.curvature <= 0.1) return 'Minimal';
    if (resolvedStyle.curvature <= 0.25) return 'Gentle';
    if (resolvedStyle.curvature <= 0.45) return 'Balanced';
    return 'Expressive';
  });

  const routeOptions: Array<{ value: PipelineConnectionRoute; label: string; helper: string }> = [
    { value: 'bezier', label: 'Curved', helper: 'Smooth bezier curve' },
    { value: 'straight', label: 'Straight', helper: 'Direct line' },
    { value: 'step', label: 'Right angle', helper: 'Elbowed route' },
    { value: 'teleport', label: 'Teleporter', helper: 'Hide mid-path, show portals' }
  ];
</script>

{#if !pipeline}
  <section class="rounded border border-surface-800/80 bg-surface-950/80 p-6 text-center text-surface-400">
    Select a pipeline to inspect connection policies.
  </section>
{:else if !selectedEdge}
  <section class="rounded border border-surface-800/80 bg-surface-950/80 p-6 text-center text-surface-400">
    Select a connection in the graph to edit its policy.
  </section>
{:else}
  <div class="space-y-4 rounded border border-surface-800/80 bg-surface-950/80 p-5 text-sm text-surface-200">
    <header>
      <p class="text-xs uppercase tracking-[0.35em] text-surface-500">Connection</p>
      <p class="text-lg font-semibold text-white">{edgeDescription}</p>
    </header>

    <div>
      <label class="text-micro uppercase tracking-[0.3em] text-surface-500" for="inspector-connection-policy">
        Overflow policy
      </label>
      <select id="inspector-connection-policy" class="input mt-1 text-sm" value={edgePolicy} onchange={handlePolicyChange}>
        {#each channelPolicyOptions as option}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
      {#if channelPolicyOptions.find((entry) => entry.value === edgePolicy)?.description}
        <p class="mt-2 text-xs text-surface-400">
          {channelPolicyOptions.find((entry) => entry.value === edgePolicy)?.description}
        </p>
      {/if}
    </div>

    <div class="grid gap-4 sm:grid-cols-2">
      <div class="space-y-2">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Path style</p>
        <div class="grid grid-cols-2 gap-2">
          {#each routeOptions as option (option.value)}
            <button
              type="button"
              class={`rounded border px-3 py-2 text-left text-sm transition ${
                resolvedStyle.route === option.value
                  ? 'border-primary-400/80 bg-primary-500/10 text-white shadow-[0_6px_18px_-8px_rgba(56,189,248,0.65)]'
                  : 'border-surface-700/80 bg-surface-800/50 text-surface-200 hover:border-primary-400/60 hover:text-white'
              }`}
              title={option.helper}
              onclick={() => updateStyle({ route: option.value })}
            >
              <p class="font-semibold">{option.label}</p>
              <p class="text-[0.75rem] text-surface-400">{option.helper}</p>
            </button>
          {/each}
        </div>
      </div>

      <div class="space-y-2">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Emphasis</p>
        <div class="grid grid-cols-3 gap-2">
          <button
            type="button"
            class={`rounded border px-3 py-2 text-sm font-semibold transition ${
              resolvedStyle.emphasis === 'soft'
                ? 'border-emerald-400/70 bg-emerald-500/10 text-emerald-50'
                : 'border-surface-700/80 bg-surface-800/50 text-surface-200 hover:border-emerald-400/60 hover:text-white'
            }`}
            onclick={() => updateStyle({ emphasis: 'soft' })}
          >
            Subtle
          </button>
          <button
            type="button"
            class={`rounded border px-3 py-2 text-sm font-semibold transition ${
              resolvedStyle.emphasis === 'normal'
                ? 'border-primary-400/70 bg-primary-500/10 text-white'
                : 'border-surface-700/80 bg-surface-800/50 text-surface-200 hover:border-primary-400/60 hover:text-white'
            }`}
            onclick={() => updateStyle({ emphasis: 'normal' })}
          >
            Default
          </button>
          <button
            type="button"
            class={`rounded border px-3 py-2 text-sm font-semibold transition ${
              resolvedStyle.emphasis === 'bold'
                ? 'border-amber-400/80 bg-amber-500/10 text-amber-50'
                : 'border-surface-700/80 bg-surface-800/50 text-surface-200 hover:border-amber-400/60 hover:text-white'
            }`}
            onclick={() => updateStyle({ emphasis: 'bold' })}
          >
            Bold
          </button>
        </div>
        <label class="flex items-center gap-2 text-[0.85rem] text-surface-200">
          <input
            type="checkbox"
            class="h-4 w-4 rounded border-surface-600/70 bg-surface-800 text-primary-300 focus:ring-primary-300"
            checked={resolvedStyle.dashed}
            onchange={(event) => updateStyle({ dashed: (event.currentTarget as HTMLInputElement)?.checked })}
          />
          <span>Dashed line</span>
        </label>
      </div>
    </div>

    {#if resolvedStyle.route === 'bezier'}
      <div>
        <label class="text-micro uppercase tracking-[0.3em] text-surface-500" for="inspector-connection-curve">
          Curve bend
        </label>
        <input
          id="inspector-connection-curve"
          type="range"
          class="range-input mt-2 w-full"
          min="0"
          max="0.8"
          step="0.02"
          value={resolvedStyle.curvature}
          oninput={(event) => updateStyle({ curvature: Number((event.currentTarget as HTMLInputElement).value) })}
        />
        <div class="mt-1 flex justify-between text-xs text-surface-400">
          <span>{curveLabel}</span>
          <span>{resolvedStyle.curvature.toFixed(2)}</span>
        </div>
      </div>
    {/if}

    {#if resolvedStyle.route === 'teleport'}
      <div class="rounded border border-primary-500/40 bg-primary-500/10 px-3 py-2 text-[0.86rem] text-primary-50">
        <p class="font-semibold uppercase tracking-[0.18em]">Teleporter</p>
        <p class="text-primary-100/80">
          Only short stubs and portal markers stay visible so long connections do not clutter the canvas.
        </p>
      </div>
    {/if}
  </div>
{/if}

<style>
  :global(.range-input) {
    accent-color: var(--color-primary-300, #38bdf8);
  }
</style>
