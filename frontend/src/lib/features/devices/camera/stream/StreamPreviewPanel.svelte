<script lang="ts">
  import CameraSensorBenchmarkTab from '$lib/features/devices/camera/CameraSensorBenchmarkTab.svelte';
  import type { ProbedBackend, ProbedDevice } from '$lib/ts-bindings/http/client';

  type BenchCodecStat = { implementation: string; avg_ms: number; avg_fps: number; errors?: number };
  type BenchFormatGroup = {
    format: string;
    capture_avg_fps: number;
    host_avg_fps: number;
    decoders?: BenchCodecStat[];
    encoders?: BenchCodecStat[];
  };

  type StreamPreviewPanelProps = {
    benchTargetFps: number;
    benchSampleMs: number;
    benchRunning: boolean;
    benchError: string | null;
    benchWarnings: string[];
    benchResults: BenchFormatGroup[];
    apiPath: (path: string) => string;
    currentDevice: ProbedDevice | null;
    currentBackend: ProbedBackend | null;
    onRunBenchmark: () => void;
  };

  let {
    benchTargetFps = $bindable(120),
    benchSampleMs = $bindable(1500),
    benchRunning,
    benchError,
    benchWarnings,
    benchResults,
    apiPath,
    currentDevice,
    currentBackend,
    onRunBenchmark
  }: StreamPreviewPanelProps = $props();

  export type $$Props = StreamPreviewPanelProps;
</script>

<div class="space-y-4">
  <div class="rounded border border-surface-800 bg-surface-900/70 p-4 space-y-3">
    <div class="text-sm text-surface-300">
      Benchmarks every capture format at the selected resolution against all available decoders (by input FOURCC) and all RG24 encoders.
    </div>
    <div class="flex flex-wrap items-end gap-3">
      <label class="text-sm w-full max-w-[10rem]">
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Target FPS</span>
        <input
          class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
          type="number"
          min="1"
          step="1"
          value={benchTargetFps}
          oninput={(e) => {
            const raw = (e.currentTarget as HTMLInputElement).value;
            const parsed = raw.trim().length ? Number(raw) : 120;
            benchTargetFps = Number.isFinite(parsed) && parsed > 0 ? parsed : 120;
          }}
        />
      </label>
      <label class="text-sm w-full max-w-[10rem]">
        <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Sample ms</span>
        <input
          class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
          type="number"
          min="250"
          step="50"
          value={benchSampleMs}
          oninput={(e) => {
            const raw = (e.currentTarget as HTMLInputElement).value;
            const parsed = raw.trim().length ? Number(raw) : 1500;
            benchSampleMs = Number.isFinite(parsed) && parsed >= 250 ? parsed : 1500;
          }}
        />
      </label>
      <button
        class="btn btn-sm preset-tonal mt-6"
        type="button"
        onclick={onRunBenchmark}
        disabled={benchRunning}
        aria-busy={benchRunning}
      >
        {benchRunning ? 'Benchmarking…' : 'Run benchmark'}
      </button>
      <div class="text-2xs text-surface-500 mt-6">
        Runs can take a while; the API temporarily stops conflicting streams and restores them afterwards.
      </div>
    </div>
  </div>

  {#if benchError}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-4 py-3 text-sm text-error-100">{benchError}</div>
  {/if}

  {#if benchWarnings.length}
    <details class="rounded border border-surface-800 bg-surface-950/40 p-3 text-sm">
      <summary class="cursor-pointer text-surface-300">Warnings ({benchWarnings.length})</summary>
      <ul class="mt-2 list-disc pl-5 text-surface-400">
        {#each benchWarnings as w, idx (`warn-${idx}`)}
          <li>{w}</li>
        {/each}
      </ul>
    </details>
  {/if}

  {#if benchResults.length}
    <div class="space-y-3">
      {#each benchResults as fmt, idx (`fmt-${fmt.format}-${idx}`)}
        <details class="rounded border border-surface-800 bg-surface-950/40 p-3" open>
          <summary class="cursor-pointer select-none flex flex-wrap items-center gap-3">
            <span class="font-mono text-surface-100">{fmt.format}</span>
            <span class="text-2xs text-surface-500">capture {Number(fmt.capture_avg_fps ?? 0).toFixed(1)} fps</span>
            <span class="text-2xs text-surface-500">host {Number(fmt.host_avg_fps ?? 0).toFixed(1)} fps</span>
          </summary>

          <div class="mt-3 grid gap-4 lg:grid-cols-2">
            <div>
              <div class="text-2xs uppercase tracking-[0.3em] text-surface-500 mb-2">Decoders</div>
              {#if (fmt.decoders ?? []).length}
                <ul class="space-y-1 font-mono text-sm">
                  {#each (fmt.decoders ?? []) as c (`dec-${fmt.format}-${c.implementation}`)}
                    <li class={Number(c.avg_fps ?? 0) >= 120 ? 'text-emerald-300' : 'text-surface-200'}>
                      - {c.implementation}: {Number(c.avg_ms ?? 0).toFixed(2)} ms, {Number(c.avg_fps ?? 0).toFixed(1)} fps{c.errors ? ` (err ${c.errors})` : ''}
                    </li>
                  {/each}
                </ul>
              {:else}
                <div class="text-sm text-surface-500">No decoders registered for this FOURCC.</div>
              {/if}
            </div>

            <div>
              <div class="text-2xs uppercase tracking-[0.3em] text-surface-500 mb-2">Encoders (RG24)</div>
              {#if (fmt.encoders ?? []).length}
                <ul class="space-y-1 font-mono text-sm">
                  {#each (fmt.encoders ?? []) as c (`enc-${fmt.format}-${c.implementation}`)}
                    <li class={Number(c.avg_fps ?? 0) >= 120 ? 'text-emerald-300' : 'text-surface-200'}>
                      - {c.implementation}: {Number(c.avg_ms ?? 0).toFixed(2)} ms, {Number(c.avg_fps ?? 0).toFixed(1)} fps{c.errors ? ` (err ${c.errors})` : ''}
                    </li>
                  {/each}
                </ul>
              {:else}
                <div class="text-sm text-surface-500">No RG24 encoders enabled.</div>
              {/if}
            </div>
          </div>
        </details>
      {/each}
    </div>
  {/if}

  <details class="rounded border border-surface-800 bg-surface-950/40 p-4">
    <summary class="cursor-pointer select-none text-sm text-surface-300">Sensor benchmark (all modes)</summary>
    <div class="mt-4">
      <CameraSensorBenchmarkTab apiPath={apiPath} device={currentDevice} backend={currentBackend} />
    </div>
  </details>
</div>
