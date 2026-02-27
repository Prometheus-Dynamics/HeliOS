<script lang="ts">
  import { derived } from 'svelte/store';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
  import { faBolt, faDisplay, faMemory, faMicrochip, faRobot, faServer } from '@fortawesome/free-solid-svg-icons';
  import { connectionState } from '$lib/api/connection';
  import { resourceTelemetryStore, type ResourceSample } from '$lib/api/telemetry';
  import { robotConnection } from '$lib/api/robotConnection';

  let { compact = false }: { compact?: boolean } = $props();

  type TelemetryBar = {
    key: 'cpu' | 'gpu' | 'memory' | 'power';
    label: string;
    display: string;
    value: number;
    max: number;
    percent: number;
    barClass: string;
    muted?: boolean;
    warning?: boolean;
  };

  type CompactTelemetryIcon = {
    key: TelemetryBar['key'];
    label: string;
    display: string;
    percent: number;
    icon: IconDefinition;
    iconClass: string;
    muted: boolean;
  };

  const indicatorCopy: Record<
    string,
    { label: string; description: string; iconClass: string; pulse: boolean }
  > = {
    unknown: {
      label: 'Backend Checking',
      description: 'Probing the Helios backend.',
      iconClass: 'text-surface-500 drop-shadow-[0_0_10px_rgba(148,163,184,0.3)]',
      pulse: true
    },
    online: {
      label: 'Backend Online',
      description: 'We are connected to the Helios backend.',
      iconClass: 'text-success-400 drop-shadow-[0_0_10px_rgba(74,222,128,0.55)]',
      pulse: false
    },
    degraded: {
      label: 'Backend Unstable',
      description: 'Recent backend requests are failing or timing out.',
      iconClass: 'text-error-300 drop-shadow-[0_0_10px_rgba(248,113,113,0.5)]',
      pulse: true
    },
    offline: {
      label: 'Backend Offline',
      description: 'No connection to the Helios backend.',
      iconClass: 'text-error-400 drop-shadow-[0_0_10px_rgba(248,113,113,0.55)]',
      pulse: true
    }
  };

  const formatLatency = (value: number | null | undefined): string => {
    if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) {
      return '—';
    }
    if (value >= 100) {
      return `${Math.round(value)} ms`;
    }
    if (value >= 10) {
      return `${value.toFixed(1)} ms`;
    }
    return `${value.toFixed(2)} ms`;
  };

  const indicatorStore = derived(connectionState, ($connection) => indicatorCopy[$connection.status]);
  const statusLine = derived(connectionState, ($connection) => {
    if ($connection.status === 'unknown') {
      return $connection.reason ? $connection.reason : 'Checking backend';
    }
    if ($connection.status === 'online') {
      if ($connection.avgResponseMs == null) {
        return 'Waiting for first response';
      }
      return `Avg response ${formatLatency($connection.avgResponseMs)}`;
    }
    if ($connection.status === 'degraded') {
      return $connection.reason ? `Unstable · ${$connection.reason}` : 'Unstable connection';
    }
    return $connection.reason ? `Offline · ${$connection.reason}` : 'Offline';
  });
  const telemetryStore = resourceTelemetryStore;
  const telemetryBars = derived(telemetryStore, (sample) => buildTelemetryBars(sample));
  const compactTelemetryIcons = derived(telemetryBars, ($telemetryBars) =>
    $telemetryBars.map((metric): CompactTelemetryIcon => {
      const icon = metric.key === 'cpu'
        ? faMicrochip
        : metric.key === 'gpu'
          ? faDisplay
          : metric.key === 'memory'
            ? faMemory
            : faBolt;
      const iconClass = metric.key === 'cpu'
        ? metric.warning
          ? 'text-amber-300'
          : 'text-primary-400'
        : metric.key === 'gpu'
          ? 'text-secondary-400'
          : metric.key === 'memory'
            ? 'text-tertiary-400'
            : 'text-rose-400';
      return {
        key: metric.key,
        label: metric.label,
        display: metric.display,
        percent: Math.max(0, Math.min(100, metric.percent)),
        icon,
        iconClass,
        muted: Boolean(metric.muted)
      };
    })
  );

  const titleStore = derived(connectionState, ($connection) => {
    const indicator = indicatorCopy[$connection.status];
    const avgLabel = formatLatency($connection.avgResponseMs);
    const reasonLine = $connection.reason ? `Reason · ${$connection.reason}` : '';
    return `${indicator.label}
${indicator.description}
Avg response · ${avgLabel}
Last change · ${new Date($connection.lastChange).toLocaleTimeString()}
${reasonLine}`.trim();
  });

  const robotIndicatorCopy: Record<
    string,
    { label: string; description: string; iconClass: string; pulse: boolean }
  > = {
    connected: {
      label: 'Robot Connected',
      description: 'NT4 topics are reachable.',
      iconClass: 'text-success-400 drop-shadow-[0_0_10px_rgba(74,222,128,0.55)]',
      pulse: false
    },
    disconnected: {
      label: 'Robot Disconnected',
      description: 'Unable to reach NT4 on the configured target.',
      iconClass: 'text-error-400 drop-shadow-[0_0_10px_rgba(248,113,113,0.55)]',
      pulse: true
    },
    unconfigured: {
      label: 'Robot Unconfigured',
      description: 'Set team number or server override to target a roboRIO.',
      iconClass: 'text-surface-500 drop-shadow-[0_0_10px_rgba(148,163,184,0.3)]',
      pulse: false
    },
    subscriptions_disabled: {
      label: 'Robot Probe Disabled',
      description: 'Enable NT4 subscriptions to browse/inspect NetworkTables.',
      iconClass: 'text-amber-300 drop-shadow-[0_0_10px_rgba(251,191,36,0.45)]',
      pulse: false
    }
  };

  const robotIndicatorStore = derived(robotConnection, ($robot) => robotIndicatorCopy[$robot.status]);
  const robotStatusLine = derived(robotConnection, ($robot) => {
    const target = $robot.host && $robot.port ? `${$robot.host}:${$robot.port}` : 'No target';
    if ($robot.status === 'connected') return target;
    if ($robot.status === 'disconnected') return $robot.error ? `${target} · ${$robot.error}` : target;
    return target;
  });
  const robotTitleStore = derived(robotConnection, ($robot) => {
    const indicator = robotIndicatorCopy[$robot.status];
    const target = $robot.host && $robot.port ? `${$robot.host}:${$robot.port}` : 'none';
    const updated = new Date($robot.updatedAt).toLocaleTimeString();
    const err = $robot.error ? `Error · ${$robot.error}` : '';
    return `${indicator.label}
${indicator.description}
Target · ${target}
Updated · ${updated}
${err}`.trim();
  });

  function buildTelemetryBars(sample: ResourceSample): TelemetryBar[] {
    const cpuPercent = clampPercent(sample.cpu.usage_percent);
    const cpuThrottled = isCpuThermallyThrottled(sample);
    const gpuUsage = sample.gpu?.usage_percent;
    const gpuPercent = gpuUsage == null ? 0 : clampPercent(gpuUsage);
    const memoryUsage = memoryPercent(sample);
    const { watts, percent: powerPercent, max: powerMax } = computePowerStats(sample);

    return [
      {
        key: 'cpu',
        label: 'CPU',
        display: `${cpuPercent.toFixed(0)}%`,
        value: cpuPercent,
        max: 100,
        percent: cpuPercent,
        barClass: cpuThrottled ? 'bg-amber-400 shadow-[0_0_12px_rgba(251,191,36,0.55)]' : 'bg-primary-500',
        warning: cpuThrottled
      },
      {
        key: 'gpu',
        label: 'GPU',
        display: gpuUsage == null ? 'Awaiting signal' : `${gpuPercent.toFixed(0)}%`,
        value: gpuUsage == null ? 0 : gpuPercent,
        max: 100,
        percent: gpuPercent,
        barClass: 'bg-secondary-400',
        muted: gpuUsage == null
      },
      {
        key: 'memory',
        label: 'Memory',
        display: `${memoryUsage.toFixed(0)}%`,
        value: memoryUsage,
        max: 100,
        percent: memoryUsage,
        barClass: 'bg-tertiary-400'
      },
      {
        key: 'power',
        label: 'Power',
        display: watts == null ? 'Awaiting signal' : `${watts.toFixed(0)} W`,
        value: watts ?? 0,
        max: powerMax,
        percent: powerPercent,
        barClass: 'bg-rose-400',
        muted: watts == null
      }
    ];
  }

  function clampPercent(value: number | null | undefined): number {
    if (typeof value !== 'number' || !Number.isFinite(value)) return 0;
    return Math.max(0, Math.min(100, Number(value.toFixed(1))));
  }

  function memoryPercent(sample: ResourceSample): number {
    if (!sample.memory.total_bytes) return 0;
    const ratio = (sample.memory.used_bytes / sample.memory.total_bytes) * 100;
    return clampPercent(ratio);
  }

  function computePowerStats(sample: ResourceSample): { watts: number | null; percent: number; max: number } {
    const max = 25;
    const watts = sample.power?.watts;
    const volts = sample.power?.volts;
    const amps = sample.power?.amps;
    const derivedWatts = typeof volts === 'number' && Number.isFinite(volts) && typeof amps === 'number' && Number.isFinite(amps)
      ? volts * amps
      : null;

    if (
      (typeof watts !== 'number' || !Number.isFinite(watts) || Math.abs(watts) < 0.001) &&
      (typeof derivedWatts !== 'number' || !Number.isFinite(derivedWatts))
    ) {
      return { watts: null, percent: 0, max };
    }
    const safeWatts = Math.abs(
      typeof watts === 'number' && Number.isFinite(watts) && Math.abs(watts) >= 0.001 ? watts : (derivedWatts ?? 0)
    );
    const percent = Math.min(100, max > 0 ? (safeWatts / max) * 100 : 0);
    return { watts: safeWatts, percent, max };
  }

  function isCpuThermallyThrottled(sample: ResourceSample): boolean {
    const throttle = sample.cpu.throttle;
    return Boolean(throttle && (throttle.throttled || throttle.soft_temp_limit));
  }
</script>

<div class={`${compact ? '' : 'px-2'} text-surface-400`} aria-live="polite">
  {#if compact}
    <div class="flex w-full flex-col items-center gap-2">
      <span
        class="inline-flex h-8 w-8 items-center justify-center rounded-md border border-surface-700/80 bg-surface-900/70"
        title={$titleStore}
      >
        <FaIcon
          icon={faServer}
          class={`h-4 w-4 ${$indicatorStore.iconClass} ${$indicatorStore.pulse ? 'animate-pulse' : ''}`}
          ariaHidden="true"
        />
        <span class="sr-only">{$indicatorStore.label}</span>
      </span>
      <span
        class="inline-flex h-8 w-8 items-center justify-center rounded-md border border-surface-700/80 bg-surface-900/70"
        title={$robotTitleStore}
      >
        <FaIcon
          icon={faRobot}
          class={`h-4 w-4 ${$robotIndicatorStore.iconClass} ${$robotIndicatorStore.pulse ? 'animate-pulse' : ''}`}
          ariaHidden="true"
        />
        <span class="sr-only">{$robotIndicatorStore.label}</span>
      </span>
      <div class="mx-auto grid h-12 w-12 grid-cols-2 grid-rows-2 place-items-center gap-1">
        {#each $compactTelemetryIcons as metric (metric.key)}
          <span
            class="inline-flex h-[1.375rem] w-[1.375rem] items-center justify-center rounded-md border border-surface-700/80 bg-surface-900/70"
            title={`${metric.label} · ${metric.display}`}
            role="progressbar"
            aria-label={`${metric.label} ${metric.display}`}
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={Math.round(metric.percent)}
          >
            <span class="relative h-4 w-4">
              <FaIcon icon={metric.icon} class="absolute inset-0 h-4 w-4 text-surface-700/80" ariaHidden="true" />
              <span class="absolute inset-0 overflow-hidden" style={`clip-path: inset(${100 - metric.percent}% 0 0 0);`}>
                <FaIcon
                  icon={metric.icon}
                  class={`h-4 w-4 ${metric.iconClass} ${metric.muted ? 'opacity-40' : 'drop-shadow-[0_0_8px_rgba(148,163,184,0.3)]'}`}
                  ariaHidden="true"
                />
              </span>
            </span>
            <span class="sr-only">{metric.label} {metric.display}</span>
          </span>
        {/each}
      </div>
    </div>
  {:else}
    <div class="min-w-0" title={$titleStore}>
      <div class="flex min-w-0 items-center gap-2.5">
        <span class="inline-flex h-3.5 w-3.5 shrink-0 items-center justify-center" aria-hidden="true">
          <FaIcon
            icon={faServer}
            class={`h-3.5 w-3.5 ${$indicatorStore.iconClass} ${$indicatorStore.pulse ? 'animate-pulse' : ''}`}
          />
        </span>
        <div class="flex min-w-0 flex-col leading-tight">
          <span class="helios-connection-label truncate font-semibold uppercase text-surface-200">
            {$indicatorStore.label}
          </span>
          <span class="helios-connection-detail truncate text-surface-500">
            {$statusLine}
          </span>
        </div>
      </div>
    </div>
    <div class="mt-3 flex min-w-0 items-center gap-2.5" title={$robotTitleStore}>
      <span class="inline-flex h-3.5 w-3.5 shrink-0 items-center justify-center" aria-hidden="true">
        <FaIcon
          icon={faRobot}
          class={`h-3.5 w-3.5 ${$robotIndicatorStore.iconClass} ${$robotIndicatorStore.pulse ? 'animate-pulse' : ''}`}
        />
      </span>
      <div class="flex min-w-0 flex-col leading-tight">
        <span class="helios-connection-label truncate font-semibold uppercase text-surface-200">
          {$robotIndicatorStore.label}
        </span>
        <span class="helios-connection-detail truncate text-surface-500">
          {$robotStatusLine}
        </span>
      </div>
    </div>
    <div class="mt-4 space-y-2">
      {#each $telemetryBars as metric (metric.key)}
        <div class="space-y-1">
          <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.25em]">
            <span class={metric.warning ? 'text-amber-300' : 'text-surface-500'}>{metric.label}</span>
            <span class={`text-micro-tight ${metric.muted ? 'text-surface-600' : 'text-surface-200'}`}>
              {metric.display}
            </span>
          </div>
          <div class="h-1.5 w-full overflow-hidden rounded-full bg-surface-800/80">
            <div
              class={`h-full rounded-full transition-[width] duration-500 ease-out ${metric.barClass} ${
                metric.muted ? 'opacity-40' : ''
              }`}
              style={`width: ${metric.percent}%;`}
              role="progressbar"
              aria-label={`${metric.label} ${metric.display}`}
              aria-valuemin="0"
              aria-valuemax={metric.max}
              aria-valuenow={metric.value}
            ></div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .helios-connection-label {
    font-size: 0.58rem;
    line-height: 1.2;
    letter-spacing: 0.2em;
  }

  .helios-connection-detail {
    font-size: 0.54rem;
    line-height: 1.15;
    letter-spacing: 0.11em;
  }

  @media (max-height: 980px), (max-width: 1400px) {
    .helios-connection-label {
      font-size: 0.54rem;
      letter-spacing: 0.17em;
    }

    .helios-connection-detail {
      font-size: 0.5rem;
      letter-spacing: 0.08em;
    }
  }

  @media (max-height: 860px), (max-width: 1220px) {
    .helios-connection-label {
      font-size: 0.5rem;
      letter-spacing: 0.14em;
    }

    .helios-connection-detail {
      font-size: 0.47rem;
      letter-spacing: 0.06em;
    }
  }
</style>
