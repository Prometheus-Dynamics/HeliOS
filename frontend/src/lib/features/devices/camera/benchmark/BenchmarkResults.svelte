<script lang="ts">
  type BenchRankRow = {
    format: string;
    resolution: string;
    captureFps: number;
    hostFps: number;
    bestDecoderImpl?: string | null;
    bestDecoderFps: number;
    bestDecoderCpu: string;
    bestEncoderImpl?: string | null;
    bestEncoderFps: number;
    bestEncoderCpu: string;
  };

  type CpuSample = { engine_cpu_avg?: number | null; system_cpu_avg?: number | null };

  type BenchCodecStat = { implementation: string; avg_ms: number; avg_fps: number; errors?: number };
  type BenchCodecStatCpu = { stat: BenchCodecStat; cpu?: CpuSample; cpu_delta?: CpuSample };

  type ModeResult = {
    format: string;
    resolution: string;
    capture_avg_fps: number;
    host_avg_fps: number;
    baseline_cpu?: CpuSample;
    encoder_input_decoder?: string | null;
    decoders: BenchCodecStatCpu[];
    encoders: BenchCodecStatCpu[];
  };

  type BenchmarkSummary = {
    benchmark_id: string;
    backend: string;
    completed_at: string;
    canceled?: boolean;
  };

  type BenchmarkListItem = { summary: BenchmarkSummary };

  type BenchmarkResult = { summary: BenchmarkSummary; modes: ModeResult[]; warnings: string[] };

  type Props = {
    listLoading: boolean;
    listError: string | null;
    benchmarks: BenchmarkListItem[];
    selectedBenchmarkId: string | null;
    selectedResult: BenchmarkResult | null;
    rankRows: BenchRankRow[];
    benchTargetFps: number;
    formatCpu: (sample?: CpuSample | null) => string;
    onRefresh: () => void;
  };

  let {
    listLoading,
    listError,
    benchmarks,
    selectedBenchmarkId = $bindable<string | null>(null),
    selectedResult,
    rankRows,
    benchTargetFps,
    formatCpu,
    onRefresh
  }: Props = $props();
</script>

<div class="rounded border border-surface-800 bg-surface-950/40 p-4 space-y-3">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <div class="text-2xs uppercase tracking-[0.3em] text-surface-500">Saved Runs</div>
    <button class="btn btn-sm preset-tonal" type="button" onclick={onRefresh} disabled={listLoading}>Refresh</button>
  </div>

  {#if listError}
    <div class="text-sm text-error-100">{listError}</div>
  {/if}

  <select class="w-full border border-surface-700 bg-surface-900/70 px-3 py-2" bind:value={selectedBenchmarkId} disabled={listLoading || benchmarks.length === 0}>
    {#each benchmarks as item (item.summary.benchmark_id)}
      <option value={item.summary.benchmark_id}>
        {item.summary.backend} — {item.summary.completed_at}{item.summary.canceled ? ' (canceled)' : ''}
      </option>
    {/each}
  </select>
</div>

{#if selectedResult}
  <div class="rounded border border-surface-800 bg-surface-950/40 p-4 space-y-3">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="text-2xs uppercase tracking-[0.3em] text-surface-500">Mode Rankings (best per mode)</div>
      <div class="text-2xs text-surface-500">Target {Math.max(1, Math.trunc(Number(benchTargetFps) || 120))} fps</div>
    </div>
    <div class="text-xs text-surface-500">
      Sorted by closeness to target FPS (primary) + engine CPU Δ (secondary). Click a row to jump to that mode below.
    </div>
    <div class="max-h-[50vh] max-h-[50svh] max-h-[50dvh] overflow-y-auto rounded border border-surface-800/70 bg-surface-900/40">
      <table class="w-full text-xs">
        <thead class="sticky top-0 bg-surface-900/90 text-surface-400">
          <tr>
            <th class="px-3 py-2 text-left font-semibold">Mode</th>
            <th class="px-3 py-2 text-left font-semibold">Capture</th>
            <th class="px-3 py-2 text-left font-semibold">Host</th>
            <th class="px-3 py-2 text-left font-semibold">Best decoder</th>
            <th class="px-3 py-2 text-left font-semibold">Best encoder</th>
          </tr>
        </thead>
        <tbody>
          {#each rankRows as row (row.format + row.resolution)}
            <tr class="border-t border-surface-800/70 hover:bg-surface-900/60">
              <td class="px-3 py-2">
                <span class="font-mono text-surface-100">{row.format}</span>
                <span class="ml-2 text-surface-500">{row.resolution}</span>
              </td>
              <td class="px-3 py-2 text-surface-300">{row.captureFps.toFixed(1)} fps</td>
              <td class="px-3 py-2 text-surface-300">{row.hostFps.toFixed(1)} fps</td>
              <td class="px-3 py-2">
                <div class="font-mono text-surface-200">{row.bestDecoderImpl ?? '—'}</div>
                <div class="text-surface-500">{row.bestDecoderFps.toFixed(1)} fps · CPU Δ {row.bestDecoderCpu}</div>
              </td>
              <td class="px-3 py-2">
                <div class="font-mono text-surface-200">{row.bestEncoderImpl ?? '—'}</div>
                <div class="text-surface-500">{row.bestEncoderFps.toFixed(1)} fps · CPU Δ {row.bestEncoderCpu}</div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>

  {#if selectedResult.warnings?.length}
    <details class="rounded border border-surface-800 bg-surface-950/40 p-3 text-sm">
      <summary class="cursor-pointer text-surface-300">Warnings ({selectedResult.warnings.length})</summary>
      <ul class="mt-2 list-disc pl-5 text-surface-400">
        {#each selectedResult.warnings as w, idx (`warn-${idx}`)}
          <li>{w}</li>
        {/each}
      </ul>
    </details>
  {/if}

  <div class="space-y-3">
    {#each selectedResult.modes as mode (mode.format + mode.resolution)}
      <details class="rounded border border-surface-800 bg-surface-950/40 p-3" open>
        <summary class="cursor-pointer select-none flex flex-wrap items-center gap-3">
          <span class="font-mono text-surface-100">{mode.format}</span>
          <span class="text-2xs text-surface-500">{mode.resolution}</span>
          <span class="text-2xs text-surface-500">capture {Number(mode.capture_avg_fps ?? 0).toFixed(1)} fps</span>
          <span class="text-2xs text-surface-500">host {Number(mode.host_avg_fps ?? 0).toFixed(1)} fps</span>
          <span class="text-2xs text-surface-500">baseline CPU {formatCpu(mode.baseline_cpu)}</span>
        </summary>

        <div class="mt-3 grid gap-4 lg:grid-cols-2">
          <div>
            <div class="text-2xs uppercase tracking-[0.3em] text-surface-500 mb-2">Decoders</div>
            {#if (mode.decoders ?? []).length}
              <ul class="space-y-1 font-mono text-sm">
                {#each (mode.decoders ?? []) as c (`dec-${mode.format}-${mode.resolution}-${c.stat.implementation}`)}
                  <li class={Number(c.stat.avg_fps ?? 0) >= 120 ? 'text-emerald-300' : 'text-surface-200'}>
                    - {c.stat.implementation}: {Number(c.stat.avg_ms ?? 0).toFixed(2)} ms, {Number(c.stat.avg_fps ?? 0).toFixed(1)} fps
                    {c.stat.errors ? ` (err ${c.stat.errors})` : ''} — CPU Δ {formatCpu(c.cpu_delta)}
                  </li>
                {/each}
              </ul>
            {:else}
              <div class="text-sm text-surface-500">No decoders registered for this FOURCC.</div>
            {/if}
          </div>

          <div>
            <div class="text-2xs uppercase tracking-[0.3em] text-surface-500 mb-2">
              Encoders (RG24){#if mode.encoder_input_decoder} via {mode.encoder_input_decoder}{/if}
            </div>
            {#if (mode.encoders ?? []).length}
              <ul class="space-y-1 font-mono text-sm">
                {#each (mode.encoders ?? []) as c (`enc-${mode.format}-${mode.resolution}-${c.stat.implementation}`)}
                  <li class={Number(c.stat.avg_fps ?? 0) >= 120 ? 'text-emerald-300' : 'text-surface-200'}>
                    - {c.stat.implementation}: {Number(c.stat.avg_ms ?? 0).toFixed(2)} ms, {Number(c.stat.avg_fps ?? 0).toFixed(1)} fps
                    {c.stat.errors ? ` (err ${c.stat.errors})` : ''} — CPU Δ {formatCpu(c.cpu_delta)}
                  </li>
                {/each}
              </ul>
            {:else}
              <div class="text-sm text-surface-500">No RG24 encoders enabled (or no decoder available for this format).</div>
            {/if}
          </div>
        </div>
      </details>
    {/each}
  </div>
{/if}
