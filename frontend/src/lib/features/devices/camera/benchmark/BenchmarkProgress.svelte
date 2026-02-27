<script lang="ts">
  type RunningProgress = {
    state: 'running';
    done: number;
    total: number;
    pct: number;
    format?: string | null;
    resolution?: string | null;
    etaLabel?: string | null;
    maxEtaLabel?: string | null;
  };

  type CompletedProgress = { state: 'completed'; canceled?: boolean };
  type FailedProgress = { state: 'failed'; error: string };

  type ProgressState = RunningProgress | CompletedProgress | FailedProgress | null;

  type Props = {
    progress: ProgressState;
  };

  const { progress }: Props = $props();
</script>

{#if progress}
  {#if progress.state === 'running'}
    <div class="space-y-2">
      <div class="flex flex-wrap items-center justify-between gap-2 text-sm text-surface-300">
        <div>
          Running: {progress.done}/{progress.total}
          {#if progress.format} — {progress.format}{/if}
          {#if progress.resolution} {progress.resolution}{/if}
        </div>
        {#if progress.etaLabel}
          <div class="text-xs text-surface-500">
            ETA {progress.etaLabel}
            {#if progress.maxEtaLabel}
              <span class="ml-2 text-surface-600">(max {progress.maxEtaLabel}; baseline + decoders + RG24 encoders per mode)</span>
            {/if}
          </div>
        {/if}
      </div>
      <div class="h-2 w-full overflow-hidden rounded bg-surface-800">
        <div class="h-full bg-primary-500" style={`width: ${progress.pct}%`}></div>
      </div>
    </div>
  {:else if progress.state === 'failed'}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-4 py-3 text-sm text-error-100">
      Benchmark failed: {progress.error}
    </div>
  {:else if progress.state === 'completed'}
    <div class="text-sm text-surface-300">
      {#if progress.canceled}
        Benchmark canceled (partial results saved).
      {:else}
        Benchmark completed.
      {/if}
    </div>
  {/if}
{/if}
