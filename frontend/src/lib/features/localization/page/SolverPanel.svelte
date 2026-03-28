<script lang="ts">
  import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';

  type SourceStatusRow = {
    source: LocalizationPipelineSource;
    pollMs: number;
    detections: number;
    tagSize: number | null;
    graphMs: number | null;
    metricsUpdatedAt: number | null;
    metricsError: string | null;
    error: string | null;
  };

  type ProfileTimingRow = {
    profileId: string;
    label: string;
    active: boolean;
    visible: boolean;
    solverMs: number | null;
    engineMs: number | null;
    totalMs: number | null;
    sourceFetchMs: number | null;
    sourceParseMs: number | null;
    cacheHit: boolean;
  };

  type SolverPanelProps = {
    open?: boolean;
    feedStatus?: string;
    pollHz?: number;
    lastPollMs?: number | null;
    activeSolveMs?: number | null;
    sourceStatusRows?: SourceStatusRow[];
    profileTimingRows?: ProfileTimingRow[];
    onClose?: () => void;
  };

  const {
    open = false,
    feedStatus = 'idle',
    pollHz = 0,
    lastPollMs = null,
    activeSolveMs = null,
    sourceStatusRows = [],
    profileTimingRows = [],
    onClose
  }: SolverPanelProps = $props();

  export type $$Props = SolverPanelProps;
</script>

{#if open}
  <div class="pointer-events-none absolute bottom-4 right-4 z-60 flex max-w-full flex-col items-end">
    <div
      class="pointer-events-auto w-full max-w-[28rem] rounded-md border border-surface-800/70 bg-surface-950/90 p-4 text-xs text-surface-300 shadow-2xl shadow-black/40 backdrop-blur"
      style="width: min(28rem, calc(100vw - 2rem));"
    >
      <div class="flex items-start justify-between gap-3">
        <div>
          <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Metrics / latency</p>
          <p class="mt-1 text-xs text-surface-500">Live localization timings by source.</p>
        </div>
        <button
          type="button"
          class="rounded-md border border-surface-700/70 bg-surface-900/70 p-1.5 text-surface-200 transition hover:border-surface-500 hover:text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary-300"
          onclick={onClose}
          aria-label="Close metrics"
          title="Close metrics"
        >
          X
        </button>
      </div>

      <div class="mt-3 rounded border border-surface-800/70 bg-surface-950/40 p-3">
        <div class="flex flex-wrap items-center gap-2 text-micro-tight uppercase tracking-[0.35em] text-surface-500">
          <span class="rounded border border-surface-700/60 bg-surface-900/60 px-2 py-1 text-surface-200">
            {feedStatus}
          </span>
          <span class="rounded border border-surface-700/60 bg-surface-900/60 px-2 py-1">
            {sourceStatusRows.length} sources
          </span>
          <span class="rounded border border-surface-700/60 bg-surface-900/60 px-2 py-1">
            {pollHz} Hz
          </span>
          <span class="rounded border border-surface-700/60 bg-surface-900/60 px-2 py-1">
            {lastPollMs != null ? `${lastPollMs.toFixed(1)} ms poll` : 'poll —'}
          </span>
          <span class="rounded border border-surface-700/60 bg-surface-900/60 px-2 py-1">
            {activeSolveMs != null ? `${activeSolveMs.toFixed(1)} ms solve` : 'solve —'}
          </span>
        </div>
        {#if profileTimingRows.length > 0}
          <div class="mt-3 space-y-2">
            <div class="grid grid-cols-[minmax(0,1.4fr)_minmax(0,0.65fr)_minmax(0,0.65fr)_minmax(0,0.65fr)] gap-2 text-micro-tight uppercase tracking-[0.35em] text-surface-500">
              <span>Profile</span>
              <span class="text-right">Solve</span>
              <span class="text-right">Total</span>
              <span class="text-right">Fetch</span>
            </div>
            {#each profileTimingRows as row (row.profileId)}
              <div class="rounded border border-surface-800/70 bg-surface-950/40 p-2">
                <div class="grid grid-cols-[minmax(0,1.4fr)_minmax(0,0.65fr)_minmax(0,0.65fr)_minmax(0,0.65fr)] items-center gap-2">
                  <div>
                    <p class="truncate text-micro font-semibold text-surface-50">
                      {row.label}
                      {#if row.active}
                        <span class="ml-2 text-primary-300">active</span>
                      {:else if row.visible}
                        <span class="ml-2 text-emerald-300">view</span>
                      {/if}
                    </p>
                    <p class="truncate text-micro-tight text-surface-500">
                      {row.engineMs != null ? `${row.engineMs.toFixed(1)} ms engine` : 'engine —'}
                      {#if row.sourceParseMs != null}
                        <span class="ml-2">{row.sourceParseMs.toFixed(1)} ms parse</span>
                      {/if}
                      {#if row.cacheHit}
                        <span class="ml-2 text-sky-300">cached</span>
                      {/if}
                    </p>
                  </div>
                  <div class="text-right text-micro text-surface-100">{row.solverMs != null ? `${row.solverMs.toFixed(1)} ms` : '—'}</div>
                  <div class="text-right text-micro text-surface-100">{row.totalMs != null ? `${row.totalMs.toFixed(1)} ms` : '—'}</div>
                  <div class="text-right text-micro text-surface-100">{row.sourceFetchMs != null ? `${row.sourceFetchMs.toFixed(1)} ms` : '—'}</div>
                </div>
              </div>
            {/each}
          </div>
        {/if}
        {#if sourceStatusRows.length === 0}
          <p class="mt-2 text-xs text-surface-500">No active outputs.</p>
        {:else}
          <div class="mt-3 space-y-2">
	          <div class="grid grid-cols-[minmax(0,1.6fr)_minmax(0,0.6fr)_minmax(0,0.7fr)_minmax(0,0.7fr)] gap-2 text-micro-tight uppercase tracking-[0.35em] text-surface-500">
	              <span>Source</span>
	              <span class="text-right">Det</span>
	              <span class="text-right">Poll</span>
	              <span class="text-right">Graph</span>
	            </div>
            {#each sourceStatusRows as row (row.source.id)}
              <div class="rounded border border-surface-800/70 bg-surface-950/40 p-2">
                <div class="grid grid-cols-[minmax(0,1.6fr)_minmax(0,0.6fr)_minmax(0,0.7fr)_minmax(0,0.7fr)] items-center gap-2">
	                  <div>
	                    <p class="truncate text-micro font-semibold text-surface-50">{row.source.streamLabel}</p>
	                    <p class="truncate text-micro-tight text-surface-500">
	                      {row.source.outputKey} · {row.source.pipelineLabel}
	                      {#if row.tagSize != null}
	                        <span class="ml-2 text-surface-600">tag {row.tagSize.toFixed(4)} m</span>
	                      {/if}
	                    </p>
	                  </div>
	                  <div class="text-right text-micro text-surface-100">{row.detections}</div>
	                  <div class="text-right text-micro text-surface-100">{row.pollMs.toFixed(1)} ms</div>
	                  <div class="text-right text-micro text-surface-100">
	                    {row.graphMs != null ? `${row.graphMs.toFixed(2)} ms` : '—'}
	                  </div>
	                </div>
                {#if row.error || row.metricsError}
                  <p class="mt-2 text-micro text-error-300">{row.error ?? row.metricsError}</p>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
