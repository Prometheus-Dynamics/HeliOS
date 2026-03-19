<script lang="ts">
  import { resolve } from '$app/paths';
  import { goto } from '$app/navigation';
  import type { StreamMetricsError } from '$lib/api/streamMetrics';

  let {
    error = $bindable<StreamMetricsError | null>(null),
    metricsStaleMessage = $bindable<string | null>(null),
    className = $bindable('')
  } = $props();

  function formatTimestamp(timestamp: number | null): string {
    if (!timestamp) return '—';
    const delta = Date.now() - timestamp;
    if (delta < 1_000) return 'just now';
    if (delta < 60_000) return `${Math.round(delta / 1_000)}s ago`;
    const date = new Date(timestamp);
    return date.toLocaleTimeString();
  }

  function formatErrorMessage(message: string): string {
    if (message.length <= 160) return message;
    return `${message.slice(0, 157)}...`;
  }

  function describeMetricsError(source: StreamMetricsError | null): {
    summary: string;
    detail: string | null;
    code: string | null;
    source: string | null;
    operation: string | null;
    timestamp: number | null;
  } {
    if (!source) {
      return { summary: 'Metrics stream error', detail: null, code: null, source: null, operation: null, timestamp: null };
    }
    const summary = source.error || 'Metrics stream error';
    const detail = source.detail && source.detail !== summary ? source.detail : null;
    return {
      summary,
      detail,
      code: source.code ?? null,
      source: source.source ?? null,
      operation: source.operation ?? null,
      timestamp: typeof source.timestamp_ms === 'number' ? source.timestamp_ms : null
    };
  }

  const errorDisplay = $derived.by(() => (error ? describeMetricsError(error) : null));
</script>

{#if errorDisplay || metricsStaleMessage}
  <div class={`space-y-2 ${className}`}>
    {#if errorDisplay}
      <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
        <p class="font-semibold text-error-100">Metrics unavailable</p>
        <p class="mt-1">{errorDisplay.summary}</p>
        {#if errorDisplay.detail}
          <p class="mt-1 text-error-300">Backend: {formatErrorMessage(errorDisplay.detail)}</p>
        {/if}
        {#if errorDisplay.timestamp}
          <p class="mt-1 text-error-300">When: {formatTimestamp(errorDisplay.timestamp)}</p>
        {/if}
        {#if errorDisplay.source}
          <p class="mt-1 text-error-300">Source: {errorDisplay.source}</p>
        {/if}
        {#if errorDisplay.operation}
          <p class="mt-1 text-error-300">Operation: {errorDisplay.operation}</p>
        {/if}
        {#if errorDisplay.code}
          <p class="mt-1 text-error-300">Code: {errorDisplay.code}</p>
        {/if}
        <button
          class="mt-2 underline decoration-error-300/60 underline-offset-2 hover:text-error-50"
          type="button"
          onclick={() => void goto(resolve('/pipelines'))}
        >
          Open pipelines
        </button>
      </div>
    {/if}
    {#if metricsStaleMessage}
      <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-200">
        <p class="font-semibold text-warning-100">Metrics stalled</p>
        <p class="mt-1">{metricsStaleMessage}</p>
      </div>
    {/if}
  </div>
{/if}
