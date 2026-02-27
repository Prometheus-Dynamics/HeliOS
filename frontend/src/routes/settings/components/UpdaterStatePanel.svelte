<script lang="ts">
  type UpdaterArtifact = {
    url?: string;
    checksum?: string;
    size_bytes?: number;
  };

  type UpdaterState = {
    update_id?: string;
    stage?: string;
    progress_percent?: number;
    started_at?: string;
    finished_at?: string;
    last_error?: string;
    artifacts?: UpdaterArtifact[];
  };

  type UpdaterStatePanelProps = {
    currentState: UpdaterState | null;
    stageLabel: string;
    stateError: string | null;
    stateLoading: boolean;
    cacheUsageBytes: number | null;
  };

  const { currentState, stageLabel, stateError, stateLoading, cacheUsageBytes }: UpdaterStatePanelProps = $props();

  export type $$Props = UpdaterStatePanelProps;
</script>

<section class="space-y-3 border border-surface-700/60 bg-surface-950/35 p-4">
  <header class="flex items-center justify-between gap-3">
    <div>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Current state</p>
      <p class="text-sm text-surface-400">Updater telemetry from /ota/state.</p>
    </div>
    {#if currentState}
      <span class="text-xs uppercase tracking-[0.3em] text-surface-400">{stageLabel}</span>
    {:else}
      <span class="text-xs uppercase tracking-[0.3em] text-surface-500">{stateLoading ? 'Loading…' : 'Idle'}</span>
    {/if}
  </header>
  {#if stateError}
    <p class="mt-2 text-xs text-error-400">{stateError}</p>
  {:else if currentState}
    <div class="mt-2 space-y-2">
      <div class="h-2 w-full overflow-hidden rounded bg-surface-800">
        <div
          class="h-2 bg-primary-500 transition-[width] duration-300"
          style={`width: ${currentState.progress_percent != null ? Math.max(0, Math.min(100, currentState.progress_percent)) : 0}%`}
        ></div>
      </div>
      <div class="flex items-center justify-between text-xs text-surface-500">
        <span>{currentState.progress_percent != null ? `${currentState.progress_percent}%` : '—'}</span>
        {#if cacheUsageBytes != null}
          <span>Cache {cacheUsageBytes.toLocaleString()} bytes</span>
        {/if}
      </div>
    </div>
    <dl class="mt-1 grid gap-2 text-xs text-surface-300 sm:grid-cols-2">
      <div>
        <dt class="text-surface-500 uppercase tracking-[0.25em]">Progress</dt>
        <dd>{currentState.progress_percent != null ? `${currentState.progress_percent}%` : '—'}</dd>
      </div>
      <div>
        <dt class="text-surface-500 uppercase tracking-[0.25em]">Started</dt>
        <dd>{currentState.started_at ? new Date(currentState.started_at).toLocaleString() : '—'}</dd>
      </div>
      <div>
        <dt class="text-surface-500 uppercase tracking-[0.25em]">Finished</dt>
        <dd>{currentState.finished_at ? new Date(currentState.finished_at).toLocaleString() : '—'}</dd>
      </div>
      {#if currentState.last_error}
        <div class="sm:col-span-2">
          <dt class="text-surface-500 uppercase tracking-[0.25em]">Last error</dt>
          <dd class="text-error-300">{currentState.last_error}</dd>
        </div>
      {/if}
      {#if currentState.artifacts?.length}
        <div class="sm:col-span-2 space-y-1">
          <dt class="text-surface-500 uppercase tracking-[0.25em]">Artifact</dt>
          <dd class="break-all font-mono text-[0.75rem]">{currentState.artifacts[0].url ?? '—'}</dd>
          {#if currentState.artifacts[0].checksum}
            <dd class="text-surface-400 break-all font-mono text-[0.75rem]">{currentState.artifacts[0].checksum}</dd>
          {/if}
          {#if currentState.artifacts[0].size_bytes != null}
            <dd class="text-surface-400">{currentState.artifacts[0].size_bytes.toLocaleString()} bytes</dd>
          {/if}
        </div>
      {/if}
    </dl>
  {:else}
    <p class="mt-3 text-xs text-surface-500">No active update reported.</p>
  {/if}
</section>
