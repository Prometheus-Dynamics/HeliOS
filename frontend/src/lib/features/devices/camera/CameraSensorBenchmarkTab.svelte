<script lang="ts">
  import { onMount } from 'svelte';
  import { readStorage, removeStorage, writeStorage } from '$lib/utils/storage';
  import type { Mode, ProbedBackend, ProbedDevice } from '$lib/ts-bindings/http/client';
  import { toaster } from '$lib';
  import BenchmarkSetup from './benchmark/BenchmarkSetup.svelte';
  import BenchmarkProgress from './benchmark/BenchmarkProgress.svelte';
  import BenchmarkResults from './benchmark/BenchmarkResults.svelte';
  import {
    availableFormats,
    availableResolutions,
    cpuDeltaEngine,
    cpuDeltaSystem,
    formatCpu,
    formatLabel,
    rankRows,
    resolutionKey,
    toggleIncluded,
    msToDuration
  } from './benchmark/benchmarkUtils';
  import type {
    BenchmarkListItem,
    BenchmarkSummary,
    CodecInfo,
    ModeResult,
    SensorBenchmarkStatus
  } from './benchmark/benchmarkUtils';
  import { etaMs, maxEtaMs, updateEtaEstimator, type EtaEstimator } from './benchmark/benchmarkState';

  let { apiPath, device, backend } = $props<{
    apiPath: (path: string) => string;
    device: ProbedDevice | null;
    backend: ProbedBackend | null;
  }>();

  let benchTargetFps = $state(120);
  let benchSampleFrames = $state(100);
  let benchSampleTimeoutMs = $state(1500);

  let runningId = $state<string | null>(null);
  let runningStatus = $state<SensorBenchmarkStatus | null>(null);
  let pollTimer: number | null = null;

  let includedFormats = $state<string[]>([]);
  let includedResolutions = $state<string[]>([]);
  let lastIdentityKey = $state<string | null>(null);

  let listLoading = $state(false);
  let listError = $state<string | null>(null);
  let benchmarks = $state<BenchmarkListItem[]>([]);
  let selectedBenchmarkId = $state<string | null>(null);
  let selectedResult = $state<{ summary: BenchmarkSummary; modes: ModeResult[]; warnings: string[] } | null>(null);

  let etaEstimator = $state<EtaEstimator | null>(null);

  let codecInventory = $state<CodecInfo[]>([]);
  const selectedModesCount = $derived.by(() => selectedModes().length);
  const totalModesCount = $derived.by(() => allModes().length);

  const progressView = $derived.by(() => {
    if (!runningStatus) return null;
    if (runningStatus.status === 'running') {
      const total = Number(runningStatus.progress.total_modes ?? 0);
      const done = Number(runningStatus.progress.completed_modes ?? 0);
      const pct = total > 0 ? Math.max(0, Math.min(100, (done / total) * 100)) : 0;
      const eta = etaMsForStatus(runningStatus);
      const maxEta = maxEtaForSelection();
      return {
        state: 'running' as const,
        done,
        total,
        pct,
        format: runningStatus.progress.current_format ?? null,
        resolution: runningStatus.progress.current_resolution ?? null,
        etaLabel: eta != null ? msToDuration(eta) : null,
        maxEtaLabel: maxEta != null ? msToDuration(maxEta) : null
      };
    }
    if (runningStatus.status === 'failed') {
      return { state: 'failed' as const, error: runningStatus.error };
    }
    if (runningStatus.status === 'completed') {
      return { state: 'completed' as const, canceled: runningStatus.summary?.canceled };
    }
    return null;
  });

  const rankRowsView = $derived.by(() => (selectedResult ? rankRows(selectedResult, benchTargetFps) : []));

  function stopPolling(): void {
    if (pollTimer) {
      window.clearInterval(pollTimer);
      pollTimer = null;
    }
  }

  function allModes(): Mode[] {
    return backend?.descriptor?.modes ?? [];
  }

  function resolveFilters(): void {
    const identityKey = `${backend?.kind ?? 'none'}:${device?.identity?.keys?.[0] ?? 'none'}`;
    if (identityKey === lastIdentityKey) return;
    lastIdentityKey = identityKey;
    includedFormats = availableFormats(allModes());
    includedResolutions = availableResolutions(allModes());
  }

  function selectedModes(): Mode[] {
    const formats = new Set(includedFormats);
    const resolutions = new Set(includedResolutions);
    return allModes().filter((mode) => formats.has(formatLabel(mode.format?.code)) && resolutions.has(resolutionKey(mode) ?? ''));
  }

  function runningStorageKey(): string | null {
    if (!backend || !device) return null;
    const devKey = device.identity?.keys?.[0] ?? 'unknown';
    return `helios.sensorBench.runningId.${backend.kind}.${devKey}`;
  }

  function updateEta(status: SensorBenchmarkStatus): void {
    etaEstimator = updateEtaEstimator(etaEstimator, status);
  }

  function etaMsForStatus(status: SensorBenchmarkStatus): number | null {
    return etaMs(status, etaEstimator);
  }

  function maxEtaForSelection(): number | null {
    return maxEtaMs(selectedModes(), codecInventory, benchSampleTimeoutMs);
  }

  async function refreshList(): Promise<void> {
    listLoading = true;
    listError = null;
    try {
      const resp = await fetch(apiPath('/streams/bench/sensor'));
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Failed (${resp.status})`);
      }
      const json = (await resp.json()) as any;
      benchmarks = Array.isArray(json?.benchmarks) ? json.benchmarks : [];
      if (benchmarks.length) {
        const ids = new Set(benchmarks.map((b) => b?.summary?.benchmark_id).filter(Boolean) as string[]);
        if (!selectedBenchmarkId || !ids.has(selectedBenchmarkId)) {
          selectedBenchmarkId = benchmarks[0]?.summary?.benchmark_id ?? null;
        }
      } else {
        selectedBenchmarkId = null;
      }
    } catch (err) {
      listError = err instanceof Error ? err.message : String(err);
    } finally {
      listLoading = false;
    }
  }

  async function fetchStatus(id: string): Promise<SensorBenchmarkStatus> {
    const resp = await fetch(apiPath(`/streams/bench/sensor/${encodeURIComponent(id)}`));
    if (!resp.ok) {
      const text = await resp.text().catch(() => '');
      throw new Error(text || `Fetch failed (${resp.status})`);
    }
    return (await resp.json()) as SensorBenchmarkStatus;
  }

  async function loadSelected(): Promise<void> {
    if (!selectedBenchmarkId) {
      selectedResult = null;
      return;
    }
    try {
      const status = await fetchStatus(selectedBenchmarkId);
      if (status.status === 'completed') {
        selectedResult = status.result;
      } else {
        selectedResult = null;
      }
    } catch (err) {
      toaster.error({ title: 'Load failed', description: err instanceof Error ? err.message : String(err) });
      selectedResult = null;
    }
  }

  const rankRowsForTarget = (result: { modes: any[] } | null) =>
    rankRows(result, benchTargetFps);

  async function cancelBenchmark(): Promise<void> {
    if (!runningId) return;
    try {
      const resp = await fetch(apiPath(`/streams/bench/sensor/${encodeURIComponent(runningId)}/cancel`), { method: 'POST' });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Cancel failed (${resp.status})`);
      }
      toaster.success({ title: 'Cancellation requested', description: `id ${runningId}` });
    } catch (err) {
      toaster.error({ title: 'Cancel failed', description: err instanceof Error ? err.message : String(err) });
    }
  }

  async function startBenchmark(): Promise<void> {
    if (!backend || !device) {
      toaster.error({ title: 'Benchmark failed', description: 'Select a device + backend first.' });
      return;
    }
    const modes = selectedModes();
    if (modes.length === 0) {
      toaster.error({ title: 'Benchmark failed', description: 'No modes selected.' });
      return;
    }

    stopPolling();
    runningId = null;
    runningStatus = null;
    etaEstimator = null;

    try {
      const payload = {
        backend: backend.kind,
        handle: backend.handle,
        device_keys: device.identity?.keys ?? [],
        target_fps: Math.max(1, Math.trunc(Number(benchTargetFps) || 120)),
        sample_ms: Math.max(250, Math.trunc(Number(benchSampleTimeoutMs) || 1500)),
        mode_ids: modes.map((m) => m.id),
        sample_frames: Math.max(1, Math.trunc(Number(benchSampleFrames) || 100)),
        sample_timeout_ms: Math.max(100, Math.trunc(Number(benchSampleTimeoutMs) || 1500)),
        restore_existing: true,
        controls: []
      };

      const resp = await fetch(apiPath('/streams/bench/sensor'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Start failed (${resp.status})`);
      }

      const json = (await resp.json()) as any;
      const id = String(json?.benchmark_id ?? '');
      if (!id) throw new Error('Missing benchmark id');

      runningId = id;
      const key = runningStorageKey();
      if (key) writeStorage(key, id);

      runningStatus = await fetchStatus(id);
      updateEta(runningStatus);

      pollTimer = window.setInterval(async () => {
        try {
          if (!runningId) return;
          const status = await fetchStatus(runningId);
          runningStatus = status;
          updateEta(status);
          if (status.status !== 'running') {
            stopPolling();
            const storageKey = runningStorageKey();
            if (storageKey) removeStorage(storageKey);
            await refreshList();
          }
        } catch (err) {
          console.warn('Polling failed', err);
        }
      }, 1000);

      toaster.success({ title: 'Benchmark started', description: `id ${id}` });
    } catch (err) {
      toaster.error({ title: 'Benchmark failed', description: err instanceof Error ? err.message : String(err) });
    }
  }

  onMount(() => {
    void refreshList();
    resolveFilters();
    return () => stopPolling();
  });

  $effect(() => {
    void loadSelected();
  });

  $effect(() => {
    resolveFilters();
    const key = runningStorageKey();
    if (!key) return;
    const stored = readStorage(key);
    if (!stored || runningId || pollTimer) return;
    runningId = stored;
    void (async () => {
      try {
        runningStatus = await fetchStatus(stored);
        updateEta(runningStatus);
        if (runningStatus?.status !== 'running') {
          removeStorage(key);
          runningId = null;
          return;
        }
        pollTimer = window.setInterval(async () => {
          try {
            if (!runningId) return;
            const status = await fetchStatus(runningId);
            runningStatus = status;
            updateEta(status);
            if (status.status !== 'running') {
              stopPolling();
              removeStorage(key);
              await refreshList();
            }
          } catch (err) {
            console.warn('Polling failed', err);
          }
        }, 1000);
      } catch {
        removeStorage(key);
        runningId = null;
      }
    })();
  });

  $effect(() => {
    // Keep codec inventory fresh for "max ETA" computation.
    if (!backend || !device) return;
    void (async () => {
      try {
        const resp = await fetch(apiPath('/streams/codecs'));
        if (!resp.ok) return;
        const json = (await resp.json()) as unknown;
        codecInventory = Array.isArray(json) ? (json as CodecInfo[]) : [];
      } catch {
        // ignore
      }
    })();
  });
</script>

<div class="space-y-4">
  <BenchmarkSetup
    bind:benchTargetFps={benchTargetFps}
    bind:benchSampleFrames={benchSampleFrames}
    bind:benchSampleTimeoutMs={benchSampleTimeoutMs}
    {backend}
    {device}
    selectedModesCount={selectedModesCount}
    totalModes={totalModesCount}
    includedFormats={includedFormats}
    includedResolutions={includedResolutions}
    availableFormats={availableFormats(allModes())}
    availableResolutions={availableResolutions(allModes())}
    onToggleFormat={(fmt) => (includedFormats = toggleIncluded(includedFormats, fmt))}
    onToggleResolution={(res) => (includedResolutions = toggleIncluded(includedResolutions, res))}
    onIncludeAllFormats={() => (includedFormats = availableFormats(allModes()))}
    onExcludeAllFormats={() => (includedFormats = [])}
    onIncludeAllResolutions={() => (includedResolutions = availableResolutions(allModes()))}
    onExcludeAllResolutions={() => (includedResolutions = [])}
    onStart={startBenchmark}
    onCancel={cancelBenchmark}
    isRunning={Boolean(runningId && runningStatus?.status === 'running')}
  />

  <BenchmarkProgress progress={progressView} />

  <BenchmarkResults
    bind:selectedBenchmarkId={selectedBenchmarkId}
    listLoading={listLoading}
    listError={listError}
    benchmarks={benchmarks}
    selectedResult={selectedResult}
    rankRows={rankRowsView}
    benchTargetFps={benchTargetFps}
    formatCpu={formatCpu}
    onRefresh={refreshList}
  />
</div>
