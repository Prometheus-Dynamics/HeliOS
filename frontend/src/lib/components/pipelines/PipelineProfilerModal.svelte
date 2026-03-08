<script lang="ts">
  import ModalShell from '$lib/components/ui/ModalShell.svelte';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { apiUrl } from '$lib/api/httpClient';

  type StreamOption = { id: string; label: string };

  type PipelineProfileResponse = {
    stream_id: string;
    pipeline_id?: string | null;
    warmup_ms: number;
    duration_ms: number;
    flamegraph?: { path: string; size_bytes: number; captured_at_ms: number } | null;
    summary?: { graph_average_time_ms: number; top_nodes: { node: string; average_time_ms: number }[] } | null;
  };

  type Props = {
    open: boolean;
    title?: string;
    pipelineId?: string | null;
    pipelineLabel?: string | null;
    streamId: string | null;
    streamOptions: StreamOption[];
    onClose: () => void;
  };

  let {
    open,
    title = 'Profiler',
    pipelineId = null,
    pipelineLabel = null,
    streamId = $bindable<string | null>(null),
    streamOptions = [],
    onClose
  }: Props = $props();

  let warmupMs = $state(500);
  let durationMs = $state(5_000);
  let enablePerf = $state(true);
  let captureFlamegraph = $state(true);
  let resetMetrics = $state(true);

  let running = $state(false);
  let error = $state<string | null>(null);
  let result = $state<PipelineProfileResponse | null>(null);

  $effect(() => {
    if (!open) return;
    if (streamId) return;
    streamId = streamOptions[0]?.id ?? null;
  });

  function flamegraphHref(): string | null {
    if (!streamId) return null;
    const pid = result?.pipeline_id ?? pipelineId;
    const suffix = pid ? `?pipeline_id=${encodeURIComponent(pid)}` : '';
    return apiUrl(`/streams/${encodeURIComponent(streamId)}/pipeline/flamegraph.svg${suffix}`);
  }

  function openFlamegraph(): void {
    const href = flamegraphHref();
    if (!href) return;
    window.open(href, '_blank', 'noopener,noreferrer');
  }

  async function runProfile() {
    if (!streamId) {
      error = 'Select a stream to profile.';
      return;
    }
    running = true;
    error = null;
    result = null;
    try {
      const warmup_ms = Math.max(0, Math.min(30_000, Math.trunc(warmupMs)));
      const duration_ms = Math.max(250, Math.min(120_000, Math.trunc(durationMs)));
      const timeoutMs = Math.min(180_000, warmup_ms + duration_ms + 15_000);
      const resp = (await StreamsApi.profilePipeline(
        {
          id: streamId,
          requestBody: {
            warmup_ms,
            duration_ms,
            pipeline_id: pipelineId,
            enable_perf_counters: enablePerf,
            capture_flamegraph: captureFlamegraph,
            reset_metrics: resetMetrics
          }
        },
        { timeoutMs }
      )) as PipelineProfileResponse;
      result = resp;
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err ?? 'Profile failed');
      error = message || 'Profile failed';
    } finally {
      running = false;
    }
  }
</script>

<ModalShell
  open={open}
  title={title}
  subtitle={pipelineLabel ? `Pipeline: ${pipelineLabel}` : pipelineId ? `Pipeline: ${pipelineId}` : ''}
  size="lg"
  onClose={onClose}
  panelClassName="border-surface-800 bg-surface-900/95 text-surface-50 shadow-2xl backdrop-blur max-h-[min(94dvh,58rem)]"
  closeLabel="Close profiler"
>
  {#snippet actions()}
    <button
      class="rounded bg-surface-800 px-3 py-2 text-xs font-semibold text-surface-50 hover:bg-surface-700 disabled:opacity-50"
      type="button"
      onclick={() => void runProfile()}
      disabled={running || !streamId}
    >
      {running ? 'Profiling…' : 'Run'}
    </button>
  {/snippet}

  <div class="space-y-4">
    <div class="grid gap-3 md:grid-cols-3">
      <label class="flex flex-col gap-1 text-xs text-surface-300">
        Stream
        <select
          class="rounded border border-surface-800 bg-surface-950 px-2 py-2 text-surface-100 disabled:opacity-50"
          bind:value={streamId}
          disabled={streamOptions.length <= 1}
        >
          {#if streamOptions.length === 0}
            <option value="" selected>No active streams</option>
          {:else}
            {#each streamOptions as option (option.id)}
              <option value={option.id}>{option.label}</option>
            {/each}
          {/if}
        </select>
      </label>

      <label class="flex flex-col gap-1 text-xs text-surface-300">
        Warmup (ms)
        <input
          class="rounded border border-surface-800 bg-surface-950 px-2 py-2 text-surface-100"
          type="number"
          bind:value={warmupMs}
          min="0"
          max="30000"
          step="50"
        />
      </label>

      <label class="flex flex-col gap-1 text-xs text-surface-300">
        Duration (ms)
        <input
          class="rounded border border-surface-800 bg-surface-950 px-2 py-2 text-surface-100"
          type="number"
          bind:value={durationMs}
          min="250"
          max="120000"
          step="250"
        />
      </label>
    </div>

    <div class="grid gap-2 md:grid-cols-3">
      <label class="flex items-center gap-2 text-xs text-surface-300">
        <input type="checkbox" bind:checked={enablePerf} />
        Perf counters
      </label>
      <label class="flex items-center gap-2 text-xs text-surface-300">
        <input type="checkbox" bind:checked={captureFlamegraph} />
        Flamegraph
      </label>
      <label class="flex items-center gap-2 text-xs text-surface-300">
        <input type="checkbox" bind:checked={resetMetrics} />
        Reset metrics
      </label>
    </div>

    {#if error}
      <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">{error}</div>
    {/if}

    {#if result?.summary}
      <div class="rounded border border-surface-800/70 bg-surface-950/40 px-4 py-3 text-xs text-surface-200">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <p class="font-semibold text-surface-50">
            Graph avg: {result.summary.graph_average_time_ms.toFixed(2)} ms
          </p>
          {#if (result.flamegraph || captureFlamegraph) && flamegraphHref()}
            <button
              type="button"
              class="underline decoration-surface-500/60 underline-offset-2 hover:text-surface-50"
              onclick={openFlamegraph}
            >
              Download flamegraph SVG
            </button>
          {/if}
        </div>

        {#if result.summary.top_nodes?.length}
          <div class="mt-3">
            <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Top nodes</p>
            <ul class="mt-2 space-y-1">
              {#each result.summary.top_nodes as node (node.node)}
                <li class="flex justify-between gap-3 text-surface-300">
                  <span class="truncate">{node.node}</span>
                  <span class="font-semibold text-surface-50">{node.average_time_ms.toFixed(2)} ms</span>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</ModalShell>
