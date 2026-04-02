<script lang="ts">
  import type { PlatformFamily, SystemsRuntimeSnapshot } from '$lib/types/systems';

  type Props = {
    runtime: SystemsRuntimeSnapshot;
    runtimeLoading: boolean;
    runtimeError: string | null;
  };

  const { runtime, runtimeLoading, runtimeError }: Props = $props();

  const capabilityRows = $derived([
    ['Logs', runtime.capabilities.logs],
    ['Console', runtime.capabilities.console],
    ['Processes', runtime.capabilities.processes],
    ['Sensors', runtime.capabilities.sensors],
    ['I2C', runtime.capabilities.i2c],
    ['IMU', runtime.capabilities.imu],
    ['Updater', runtime.capabilities.updater],
    ['Resource guard', runtime.capabilities.resourceGuard],
    ['Active root', runtime.capabilities.activeRoot]
  ] as const);

  const formatPlatformFamily = (family: PlatformFamily): string => {
    switch (family) {
      case 'raspberry_pi':
        return 'Raspberry Pi';
      case 'generic_linux':
        return 'Generic Linux';
      default:
        return 'Unknown';
    }
  };

  const formatBool = (value: boolean): string => (value ? 'Enabled' : 'Disabled');

  const formatMs = (value: number | null | undefined): string => {
    if (typeof value !== 'number' || !Number.isFinite(value)) return 'n/a';
    return `${value.toLocaleString()} ms`;
  };

  const formatCount = (value: number | null | undefined): string => {
    if (typeof value !== 'number' || !Number.isFinite(value)) return 'n/a';
    return value.toLocaleString();
  };

  const formatBytes = (value: number | null | undefined): string => {
    if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) return 'n/a';
    const units = ['B', 'KiB', 'MiB', 'GiB'];
    let size = value;
    let unit = 0;
    while (size >= 1024 && unit < units.length - 1) {
      size /= 1024;
      unit += 1;
    }
    const digits = unit === 0 ? 0 : size >= 10 ? 1 : 2;
    return `${size.toFixed(digits)} ${units[unit]}`;
  };

  const formatLogFreshness = (): string => {
    const freshness = runtime.observability.logSourcesFreshness;
    const detail =
      freshness.lastSuccessAtMs && Number.isFinite(freshness.lastSuccessAtMs)
        ? `last ok ${new Date(freshness.lastSuccessAtMs).toLocaleTimeString()}`
        : 'no successful refresh yet';
    return `${freshness.state} · ${freshness.reason} · ${detail}`;
  };
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3 rounded border border-surface-800 bg-surface-950/30 p-3">
  {#if runtimeLoading}
    <div class="rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2 text-xs text-surface-400">
      Loading runtime policy and platform state…
    </div>
  {/if}

  {#if runtimeError}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
      {runtimeError}
    </div>
  {/if}

  <div class="grid gap-3 xl:grid-cols-[minmax(0,1.05fr)_minmax(0,1.25fr)]">
    <section class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
      <div class="flex items-start justify-between gap-3">
        <div>
          <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Platform</p>
          <h2 class="mt-2 text-lg font-semibold text-surface-50">{formatPlatformFamily(runtime.platform.family)}</h2>
          <p class="mt-1 text-sm text-surface-400">
            {runtime.platform.model ?? 'Model unavailable'} · {runtime.platform.architecture}
          </p>
        </div>
        <div
          class={`rounded-full border px-3 py-1 text-[0.7rem] uppercase tracking-[0.22em] ${
            runtime.observability.health.ok
              ? 'border-success-500/40 bg-success-500/10 text-success-200'
              : 'border-warning-500/40 bg-warning-500/10 text-warning-100'
          }`}
        >
          {runtime.observability.health.ok ? 'Healthy' : 'Degraded'}
        </div>
      </div>

      <div class="mt-4 grid gap-2 sm:grid-cols-2">
        {#each capabilityRows as [label, enabled] (label)}
          <div class="rounded border border-surface-800/70 bg-surface-900/50 px-3 py-2">
            <p class="text-micro uppercase tracking-[0.25em] text-surface-500">{label}</p>
            <p class={`mt-1 text-sm font-semibold ${enabled ? 'text-success-200' : 'text-surface-300'}`}>
              {enabled ? 'Available' : 'Unavailable'}
            </p>
          </div>
        {/each}
      </div>

      <div class="mt-4 grid gap-2 sm:grid-cols-2">
        <div class="rounded border border-surface-800/70 bg-surface-900/50 px-3 py-2">
          <p class="text-micro uppercase tracking-[0.25em] text-surface-500">OS</p>
          <p class="mt-1 text-sm text-surface-100">{runtime.observability.os.prettyName ?? 'Unknown OS'}</p>
          <p class="text-[0.72rem] text-surface-500">
            version {runtime.observability.os.versionId ?? 'n/a'} · build {runtime.observability.os.buildId ?? 'n/a'}
          </p>
        </div>
        <div class="rounded border border-surface-800/70 bg-surface-900/50 px-3 py-2">
          <p class="text-micro uppercase tracking-[0.25em] text-surface-500">Health snapshot</p>
          <p class="mt-1 text-sm text-surface-100">
            uptime {formatMs(runtime.observability.health.uptimeMs)}
          </p>
          <p class="text-[0.72rem] text-surface-500">
            version {runtime.observability.health.version || 'unknown'} · server {formatMs(runtime.observability.health.serverTimeMs)}
          </p>
        </div>
      </div>
    </section>

    <section class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
      <div class="grid gap-3 lg:grid-cols-2">
        <div class="rounded border border-surface-800/70 bg-surface-900/50 p-3">
          <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Tokio</p>
          <div class="mt-3 space-y-3 text-sm text-surface-200">
            <div>
              <p class="font-semibold text-surface-100">API</p>
              <p>workers {formatCount(runtime.policies.apiTokio.workerThreads)} · blocking {formatCount(runtime.policies.apiTokio.maxBlockingThreads)}</p>
              <p class="text-[0.72rem] text-surface-500">stack {formatBytes(runtime.policies.apiTokio.threadStackBytes)} · keepalive {formatMs(runtime.policies.apiTokio.blockingKeepAliveMs)}</p>
            </div>
            <div>
              <p class="font-semibold text-surface-100">Engine</p>
              <p>workers {formatCount(runtime.policies.engineTokio.workerThreads)} · blocking {formatCount(runtime.policies.engineTokio.maxBlockingThreads)}</p>
              <p class="text-[0.72rem] text-surface-500">stack {formatBytes(runtime.policies.engineTokio.threadStackBytes)} · keepalive {formatMs(runtime.policies.engineTokio.blockingKeepAliveMs)}</p>
            </div>
            <div>
              <p class="font-semibold text-surface-100">Peripherals</p>
              <p>workers {formatCount(runtime.policies.peripheralsTokio.workerThreads)} · blocking {formatCount(runtime.policies.peripheralsTokio.maxBlockingThreads)}</p>
              <p class="text-[0.72rem] text-surface-500">stack {formatBytes(runtime.policies.peripheralsTokio.threadStackBytes)} · keepalive {formatMs(runtime.policies.peripheralsTokio.blockingKeepAliveMs)}</p>
            </div>
          </div>
        </div>

        <div class="rounded border border-surface-800/70 bg-surface-900/50 p-3">
          <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Policies</p>
          <dl class="mt-3 space-y-2 text-sm text-surface-200">
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Log filter</dt>
              <dd class="font-mono text-surface-100">{runtime.policies.logFilter}</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Startup warm</dt>
              <dd>{formatCount(runtime.policies.startupCacheWarm.attempts)} attempts / {formatMs(runtime.policies.startupCacheWarm.initialDelayMs)}</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Log sources</dt>
              <dd>{formatMs(runtime.policies.logSources.cacheMs)} cache / {formatMs(runtime.policies.logSources.refreshTimeoutMs)} timeout</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">I2C inventory</dt>
              <dd>{formatMs(runtime.policies.i2cInventory.cacheTtlMs)} cache / {formatMs(runtime.policies.i2cInventory.timeoutMs)} timeout</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">IMU idle cadence</dt>
              <dd>{formatMs(runtime.policies.imu.idleIntervalMs)}</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Resource guard</dt>
              <dd>{formatBool(runtime.policies.resourceGuard.enabled)} / {formatMs(runtime.policies.resourceGuard.pollMs)}</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Styx capture overrides</dt>
              <dd>{runtime.policies.styxCapture.anyOverridden ? 'Present' : 'None'}</dd>
            </div>
          </dl>
        </div>
      </div>

      <div class="mt-3 grid gap-3 lg:grid-cols-2">
        <div class="rounded border border-surface-800/70 bg-surface-900/50 p-3">
          <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Observability</p>
          <dl class="mt-3 space-y-2 text-sm text-surface-200">
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Streams</dt>
              <dd>{formatCount(runtime.observability.streams.streamCount)} active / {formatCount(runtime.observability.streams.codecCount)} codecs</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Stream snapshot</dt>
              <dd>{runtime.observability.streams.stale ? 'Stale' : 'Fresh'} / rev {formatCount(runtime.observability.streams.revision)}</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Log sources</dt>
              <dd>{formatCount(runtime.observability.logSourceCount)} sources / rev {formatCount(runtime.observability.logSourcesRevision)}</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">API tools helper</dt>
              <dd>{runtime.observability.health.apiToolsHelperOk ? 'Healthy' : 'Missing'}</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Pressure active</dt>
              <dd>{runtime.observability.resourceGuard.pressureActive ? 'Yes' : 'No'}</dd>
            </div>
            <div class="flex items-center justify-between gap-3">
              <dt class="text-surface-400">Degraded streams</dt>
              <dd>{formatCount(runtime.observability.resourceGuard.degradedStreamCount)}</dd>
            </div>
          </dl>
        </div>

        <div class="rounded border border-surface-800/70 bg-surface-900/50 p-3">
          <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Freshness</p>
          <p class="mt-3 text-sm text-surface-100">{formatLogFreshness()}</p>
          <p class="mt-2 text-[0.72rem] text-surface-500">
            helper path {runtime.observability.health.apiToolsHelperPath || 'n/a'}
          </p>
          <div class="mt-3 grid grid-cols-2 gap-2 text-sm">
            <div class="rounded border border-surface-800/70 bg-surface-950/40 px-3 py-2">
              <p class="text-micro uppercase tracking-[0.25em] text-surface-500">Shadow recorder</p>
              <p class="mt-1 text-surface-100">{formatBool(runtime.observability.health.shadowRecorder)}</p>
            </div>
            <div class="rounded border border-surface-800/70 bg-surface-950/40 px-3 py-2">
              <p class="text-micro uppercase tracking-[0.25em] text-surface-500">Pipeline warm</p>
              <p class="mt-1 text-surface-100">{formatBool(runtime.observability.health.pipelineRegistryStartupWarm)}</p>
            </div>
            <div class="rounded border border-surface-800/70 bg-surface-950/40 px-3 py-2">
              <p class="text-micro uppercase tracking-[0.25em] text-surface-500">Pipeline prefetch</p>
              <p class="mt-1 text-surface-100">{formatBool(runtime.observability.health.pipelineRegistryPrefetch)}</p>
            </div>
            <div class="rounded border border-surface-800/70 bg-surface-950/40 px-3 py-2">
              <p class="text-micro uppercase tracking-[0.25em] text-surface-500">Active root</p>
              <p class="mt-1 text-surface-100">{runtime.observability.os.activeRoot ?? 'n/a'}</p>
            </div>
          </div>
        </div>
      </div>
    </section>
  </div>
</div>
