<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount } from 'svelte';
  import type { DashboardPayload } from '$lib/types/dashboard';
  import type { UsageTileConfig } from '$lib/components/types';
  import type { StreamPreviewItem } from '$lib/components/dashboard/DashboardStreamsPanel.svelte';
  import DashboardAlerts from '$lib/features/dashboard/page/DashboardAlerts.svelte';
  import DashboardSummarySection from '$lib/features/dashboard/page/DashboardSummarySection.svelte';
  import DashboardTelemetryPanel from '$lib/features/dashboard/page/DashboardTelemetryPanel.svelte';
  import DashboardStreamsSection from '$lib/features/dashboard/page/DashboardStreamsSection.svelte';
  import { createDomainResource } from '$lib/api/domainResources';
  import { scheduleWhenIdle } from '$lib/utils/browserSchedule';
  import { resourceTelemetryStore, EMPTY_RESOURCE_SAMPLE, type ResourceSample } from '$lib/api/telemetry';
  import { fetchDashboardPageData } from '$lib/api/dashboardPage';
  import { connectionState } from '$lib/api/connection';
  import { createBackoffTimer } from '$lib/utils/backoff';
  import {
    buildCoreSeries,
    buildCpuThrottleBadges,
    clampPercent,
    clonePayload,
    cloneSample,
    cpuTemp,
    diskPercent,
    diskPartitionPercent,
    formatBytes,
    formatCpuTooltip,
    formatDelta,
    formatDiskTooltip,
    formatFrequency,
    formatGpuMemoryTrend,
    formatGpuTooltip,
    formatMemoryTooltip,
    formatMemoryTrend,
    formatNetworkTrend,
    formatPowerTooltip,
    formatPowerTrend,
    formatRequestError,
    formatTempTrend,
    formatThroughput,
    gpuMemoryPercent,
    isCpuThermallyThrottled,
    latestGpuFrequencyMhz,
    memoryPercent,
    networkThroughput,
    powerWatts,
    throughputCombinedMbps,
    throughputMbps,
  } from '$lib/features/dashboard/page/dashboardTelemetryUtils';

  const { data } = $props<{ data: { payload: DashboardPayload } }>();
  const readPayload = () => data.payload;
  const emptyDashboard = clonePayload(readPayload());
  let dashboard = $state<DashboardPayload>(clonePayload(readPayload()));
  let dataLoadError = $state<string | null>(null);
  let isRefreshing = $state(false);
  let hasLoadedOnce = $state(
    Boolean(
      readPayload().summaryStats?.length ||
        readPayload().timelineItems?.length ||
        readPayload().pipelineWatch?.length ||
        readPayload().streamGallery?.length ||
        readPayload().meta?.fetchedAt
    )
  );
  const AUTO_REFRESH_MS = 15_000;
  const AUTO_REFRESH_MAX_MS = 60_000;
  const DASHBOARD_CACHE_KEY = 'dashboard:payload:v1';
  const DASHBOARD_CACHE_STALE_MS = 12_000;
  const DASHBOARD_CACHE_MAX_MS = 120_000;
  const dashboardResource = createDomainResource({
    key: DASHBOARD_CACHE_KEY,
    loader: fetchDashboardPageData,
    staleMs: DASHBOARD_CACHE_STALE_MS,
    maxAgeMs: DASHBOARD_CACHE_MAX_MS,
    kinds: ['device', 'localization', 'media', 'pipelines', 'settings', 'streams']
  });
  const refreshTimer = createBackoffTimer({ baseMs: AUTO_REFRESH_MS, maxMs: AUTO_REFRESH_MAX_MS });
  let selectedMetric = $state<string | null>(null);

  const summaryStats = $derived(dashboard.summaryStats ?? []);
  const streamGallery = $derived(dashboard.streamGallery ?? []);
  const connectionStatus = $derived($connectionState.status);
  const isBackendUnavailable = $derived(browser && navigator.onLine === false);

	  const streamSummary = $derived((() => {
	    const summary = { live: 0, degraded: 0, offline: 0, idle: 0, total: streamGallery.length };
	    for (const stream of streamGallery) {
	      summary[stream.status] = (summary[stream.status] ?? 0) + 1;
	    }
	    return summary;
	  })());

  let telemetrySample = $state<ResourceSample>(cloneSample(EMPTY_RESOURCE_SAMPLE));
  let telemetryHistory = $state<ResourceSample[]>([]);
  let unsubscribe: (() => void) | null = null;
  let stopDomainInvalidation: (() => void) | null = null;
  let cancelBootstrapRefresh: (() => void) | null = null;

  let pendingSamples: ResourceSample[] = [];
  let rafHandle: number | null = null;

  const flushPendingSamples = () => {
    if (!pendingSamples.length) return;
    const samples = pendingSamples;
    pendingSamples = [];
    telemetrySample = samples[samples.length - 1] ?? telemetrySample;
    telemetryHistory = [...telemetryHistory, ...samples].slice(-60);
    rafHandle = null;
  };

  if (browser) {
    unsubscribe = resourceTelemetryStore.subscribe((sample) => {
      pendingSamples.push(sample);
      if (rafHandle == null && typeof requestAnimationFrame === 'function') {
        rafHandle = requestAnimationFrame(flushPendingSamples);
      } else if (rafHandle == null) {
        // Fallback for non-raf environments
        rafHandle = setTimeout(flushPendingSamples, 0) as unknown as number;
      }
    });
  }

  onMount(() => {
    const cached = dashboardResource.read();
    if (cached?.data) {
      dashboard = clonePayload(cached.data);
      hasLoadedOnce = true;
    }
    stopDomainInvalidation = dashboardResource.subscribeInvalidations(() => {
      void refreshDashboard();
    }, { debounceMs: 250 });
    if (hasLoadedOnce) {
      cancelBootstrapRefresh = scheduleWhenIdle(() => {
        if (!document.hidden) {
          void refreshDashboard();
        }
      }, { timeoutMs: 1800, fallbackMs: 600 });
    } else {
      void refreshDashboard({ bootstrap: true });
    }
    if (browser) {
      scheduleDashboardRefresh();
    }
    return () => {};
  });

  onDestroy(() => {
    cancelBootstrapRefresh?.();
    cancelBootstrapRefresh = null;
    unsubscribe?.();
    stopDomainInvalidation?.();
    refreshTimer.cancel();
    if (rafHandle != null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(rafHandle);
    } else if (rafHandle != null) {
      clearTimeout(rafHandle);
    }
  });

  const cpuSeries = $derived(telemetryHistory.map((sample) => clampPercent(sample.cpu.usage_percent)));
  const memorySeries = $derived(telemetryHistory.map((sample) => memoryPercent(sample)));
  const gpuSeries = $derived(telemetryHistory.map((sample) => clampPercent(sample.gpu?.usage_percent ?? 0)));
  const cpuTempSeries = $derived(telemetryHistory.map((sample) => cpuTemp(sample)));
  const powerSeries = $derived(telemetryHistory.map((sample) => powerWatts(sample) ?? 0));
  const powerMax = $derived(25);
  const cpuTempMax = $derived((() => {
    const peak = cpuTempSeries.reduce((acc: number, value: number) => Math.max(acc, value), 0);
    const bucket = Math.ceil(peak / 10) * 10;
    return Math.max(80, bucket || 80);
  })());
  const gpuMemorySeries = $derived(telemetryHistory.map((sample) => gpuMemoryPercent(sample)));
  const diskSeries = $derived(telemetryHistory.map((sample) => diskPercent(sample)));
  const diskPartitions = $derived(telemetrySample.disks ?? []);
  const dataDiskMount = $derived(
    (() => {
      const disk = telemetrySample.disk;
      if (!disk) return null;
      const match = (telemetrySample.disks ?? []).find(
        (partition) =>
          partition.total_bytes === disk.total_bytes &&
          partition.used_bytes === disk.used_bytes &&
          partition.free_bytes === disk.free_bytes
      );
      return match?.mount ?? null;
    })()
  );
  const networkRxSeries = $derived(telemetryHistory.map((sample) => throughputMbps(sample.network?.rx_bytes_per_sec ?? null)));
  const networkTxSeries = $derived(telemetryHistory.map((sample) => throughputMbps(sample.network?.tx_bytes_per_sec ?? null)));
  const networkCombinedSeries = $derived(telemetryHistory.map((sample) => throughputCombinedMbps(sample)));
  const networkMax = $derived((() => {
    const peak = [...networkRxSeries, ...networkTxSeries].reduce((acc: number, value: number) => Math.max(acc, value), 0);
    const bucket = Math.ceil(peak / 5) * 5;
    return Math.max(5, bucket || 5);
  })());
  const gpuClockMhz = $derived(latestGpuFrequencyMhz(telemetrySample, telemetryHistory));
  const gpuClockLabel = $derived(formatFrequency(gpuClockMhz));
  const networkInterfaces = $derived(telemetrySample.network?.interfaces ?? []);
  const cpuThrottled = $derived(isCpuThermallyThrottled(telemetrySample));
  const cpuThrottleBadges = $derived(buildCpuThrottleBadges(telemetrySample));

  const usageTiles = $derived((() => {
    const gpuUsage = telemetrySample.gpu?.usage_percent;
    const gpuValue = gpuUsage == null ? '—' : clampPercent(gpuUsage);
    const gpuUnit = gpuUsage == null ? '' : '%';

    const cpuTempValue = telemetrySample.cpu.temperature_c;
    const cpuTempDisplay =
      typeof cpuTempValue === 'number' && Number.isFinite(cpuTempValue) && cpuTempValue > 0 ? cpuTempValue : '—';

    const tiles: UsageTileConfig[] = [
      {
        label: 'CPU',
        value: clampPercent(telemetrySample.cpu.usage_percent),
        trend: formatDelta(cpuSeries),
        sparkClass: 'text-primary-500',
        valueClass: 'text-primary-400',
        series: cpuSeries,
        max: 100,
        unit: '%',
        precision: 0,
        tooltip: formatCpuTooltip(telemetrySample),
        warning: cpuThrottled,
        warningLabel: 'Thermal'
      },
      {
        label: 'GPU',
        value: gpuValue,
        trend: (() => {
          const loadTrend = telemetrySample.gpu?.usage_percent == null ? 'Awaiting signal' : formatDelta(gpuSeries);
          const hasClock = typeof gpuClockMhz === 'number' && Number.isFinite(gpuClockMhz) && gpuClockMhz > 0;
          if (hasClock) {
            if (loadTrend === 'Awaiting signal' || loadTrend === 'stable') {
              return `Clock ${gpuClockLabel}`;
            }
            return `${loadTrend} · ${gpuClockLabel}`;
          }
          return loadTrend;
        })(),
        sparkClass: 'text-secondary-400',
        valueClass: 'text-secondary-300',
        series: gpuSeries,
        max: 100,
        unit: gpuUnit,
        precision: 0,
        tooltip: formatGpuTooltip(telemetrySample)
      },
      {
        label: 'CPU Temp',
        value: cpuTempDisplay,
        trend: formatTempTrend(cpuTempSeries),
        sparkClass: 'text-amber-400',
        valueClass: 'text-amber-200',
        series: cpuTempSeries,
        max: cpuTempMax,
        unit: '°C',
        precision: 0,
        tooltip: telemetrySample.cpu.temperature_c != null ? `CPU: ${telemetrySample.cpu.temperature_c.toFixed(1)}°C` : 'No CPU temperature sensor.'
      },
      {
        label: 'GPU Memory',
        value: gpuMemoryPercent(telemetrySample),
        trend: formatGpuMemoryTrend(telemetrySample),
        sparkClass: 'text-secondary-300',
        valueClass: 'text-secondary-200',
        series: gpuMemorySeries,
        max: 100,
        unit: '%',
        precision: 0,
        tooltip: formatGpuTooltip(telemetrySample)
      },
      {
        label: 'Memory',
        value: memoryPercent(telemetrySample),
        trend: formatMemoryTrend(telemetrySample),
        sparkClass: 'text-tertiary-400',
        valueClass: 'text-tertiary-300',
        series: memorySeries,
        max: 100,
        unit: '%',
        precision: 0,
        tooltip: formatMemoryTooltip(telemetrySample)
      },
      {
        label: 'Disk',
        value: diskPercent(telemetrySample),
        trend: formatDelta(diskSeries),
        sparkClass: 'text-primary-300',
        valueClass: 'text-primary-200',
        series: diskSeries,
        max: 100,
        unit: '%',
        precision: 0,
        tooltip: formatDiskTooltip(telemetrySample)
      },
      {
        label: 'Network',
        value: networkThroughput(telemetrySample).totalMbps,
        trend: formatNetworkTrend(telemetrySample),
        sparkClass: 'text-amber-300',
        valueClass: 'text-amber-200',
        series: networkCombinedSeries,
        max: networkMax,
        unit: 'MB/s',
        precision: 1,
        tooltip: (() => {
          const { rxMbps, txMbps, totalMbps } = networkThroughput(telemetrySample);
          return `RX: ${rxMbps.toFixed(2)} MB/s\nTX: ${txMbps.toFixed(2)} MB/s\nTotal: ${totalMbps.toFixed(2)} MB/s`;
        })()
      },
      {
        label: 'Power',
        value: powerWatts(telemetrySample) ?? 0,
        trend: formatPowerTrend(telemetrySample),
        sparkClass: 'text-rose-400',
        valueClass: 'text-rose-200',
        series: powerSeries,
        max: powerMax,
        unit: 'W',
        precision: 1,
        tooltip: formatPowerTooltip(telemetrySample)
      }
    ];
    return tiles;
  })());

  const streamCards = $derived<StreamPreviewItem[]>((() =>
    streamGallery.map((stream, idx) => ({
      id: `${stream.name}-${idx}`,
      name: stream.name,
      status: stream.status,
      recordingActive: stream.recordingActive,
      recordingSinceMs: stream.recordingSinceMs ?? null,
      captureSessionId: stream.captureSessionId,
      captureSessionAlias: stream.captureSessionAlias,
      cameraUid: stream.cameraUid
    }))
  )());

  const isInitialLoading = $derived(!hasLoadedOnce && isRefreshing);

  async function refreshDashboard(options: { bootstrap?: boolean } = {}): Promise<void> {
    if (!browser || isRefreshing) return;
    const { bootstrap = false } = options;
    if (isBackendUnavailable) {
      if (bootstrap && !hasLoadedOnce) {
        dataLoadError = 'Backend unavailable. Update the API base URL in Settings.';
        hasLoadedOnce = true;
      }
      scheduleDashboardRefresh();
      return;
    }
    isRefreshing = true;
    try {
      const payload = await dashboardResource.refresh();
      dashboard = clonePayload(payload);
      dataLoadError = null;
      refreshTimer.reset();
    } catch (error) {
      if (connectionStatus === 'online') {
        console.error('Failed to refresh dashboard', error);
      }
      dataLoadError = formatRequestError(error);
      dashboard = clonePayload(emptyDashboard);
      dashboardResource.invalidate();
      refreshTimer.bump();
    } finally {
      isRefreshing = false;
      if (bootstrap || !hasLoadedOnce) {
        hasLoadedOnce = true;
      }
      scheduleDashboardRefresh();
    }
  }

  function scheduleDashboardRefresh(delay = refreshTimer.getDelay()): void {
    if (!browser) return;
    refreshTimer.schedule(() => {
      void refreshDashboard();
    }, delay);
  }

  const cpuCoreSeries = $derived(buildCoreSeries(telemetrySample.cpu.cores ?? [], telemetryHistory));


</script>

<section class="flex min-h-0 flex-1 flex-col gap-6">
  <DashboardAlerts errorMessage={dataLoadError} />

  <div class="flex min-h-0 flex-1 flex-col gap-6">
      <DashboardSummarySection {isInitialLoading} {summaryStats} />

	      <div class="grid gap-6">
        <DashboardTelemetryPanel
          {usageTiles}
          {cpuThrottleBadges}
          {selectedMetric}
          onSelectMetric={(metric) => {
            selectedMetric = selectedMetric === metric ? null : metric;
          }}
          {cpuCoreSeries}
          {telemetrySample}
          {cpuTempSeries}
          {cpuTempMax}
          {diskSeries}
          {diskPartitions}
          {dataDiskMount}
          {networkInterfaces}
          {formatFrequency}
          {formatThroughput}
          {formatBytes}
          {throughputMbps}
          {diskPercent}
          {diskPartitionPercent}
        />

        <DashboardStreamsSection streams={streamCards} summary={streamSummary} />
      </div>
  </div>
</section>
