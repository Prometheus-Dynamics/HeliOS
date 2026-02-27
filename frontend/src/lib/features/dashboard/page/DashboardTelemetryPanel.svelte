<script lang="ts">
  import { InlineSparkline, Panel, UsageTile } from '$lib';
  import { powerWatts } from './dashboardTelemetryUtils';
  import type { UsageTileConfig } from '$lib/components/types';
  import type { ResourceSample } from '$lib/api/telemetry';

  type ThrottleBadge = { key: string; label: string; kind: 'thermal' | 'undervoltage'; state: 'active' | 'history' };
  type CoreUsageSeries = { id: number; label: string; series: number[]; latest: number; frequencyMhz: number | null };

  type Props = {
    usageTiles: UsageTileConfig[];
    cpuThrottleBadges: ThrottleBadge[];
    selectedMetric: string | null;
    onSelectMetric: (metric: string) => void;
    cpuCoreSeries: CoreUsageSeries[];
    telemetrySample: ResourceSample;
    cpuTempSeries: number[];
    cpuTempMax: number;
    diskSeries: number[];
    diskPartitions: ResourceSample['disks'];
    dataDiskMount: string | null;
    networkInterfaces: NonNullable<ResourceSample['network']>['interfaces'];
    formatFrequency: (value: number | null | undefined) => string;
    formatThroughput: (value: number) => string;
    formatBytes: (value: number | null | undefined) => string;
    throughputMbps: (value: number | null | undefined) => number;
    diskPercent: (sample: ResourceSample) => number;
    diskPartitionPercent: (partition: ResourceSample['disks'][number]) => number;
  };

  const {
    usageTiles,
    cpuThrottleBadges,
    selectedMetric,
    onSelectMetric,
    cpuCoreSeries,
    telemetrySample,
    cpuTempSeries,
    cpuTempMax,
    diskSeries,
    diskPartitions,
    dataDiskMount,
    networkInterfaces,
    formatFrequency,
    formatThroughput,
    formatBytes,
    throughputMbps,
    diskPercent,
    diskPartitionPercent
  }: Props = $props();

  const metricButtonClass = (metricLabel: string): string => {
    const selected = selectedMetric === metricLabel;
    return selected
      ? 'ring-2 ring-primary-400/70'
      : 'ring-0 hover:ring-1 hover:ring-surface-700/60';
  };

  const throttleBadgeClass = (badge: ThrottleBadge): string => {
    if (badge.kind === 'undervoltage') {
      return badge.state === 'active'
        ? 'border-error-400/60 bg-error-500/15 text-error-200'
        : 'border-error-400/30 bg-error-500/5 text-error-300/70';
    }
    return badge.state === 'active'
      ? 'border-amber-400/60 bg-amber-500/15 text-amber-200'
      : 'border-amber-400/30 bg-amber-500/5 text-amber-300/70';
  };
</script>

<Panel tone="default">
  <svelte:fragment slot="actions">
    {#if cpuThrottleBadges.length}
      <div class="flex flex-wrap items-center gap-2">
        {#each cpuThrottleBadges as badge (badge.key)}
          <span
            class={`rounded-full border px-3 py-1 text-micro-tight font-semibold uppercase tracking-[0.3em] ${throttleBadgeClass(badge)}`}
            title={`${badge.label} ${badge.state === 'active' ? 'active' : 'since boot'}`}
          >
            {badge.label} · {badge.state === 'active' ? 'Active' : 'History'}
          </span>
        {/each}
      </div>
    {/if}
  </svelte:fragment>
  <div class="grid gap-3 md:grid-cols-3 xl:grid-cols-4">
    {#each usageTiles.filter((tile) => ['CPU', 'GPU', 'Memory', 'Disk', 'Network', 'Power', 'CPU Temp', 'GPU Memory'].includes(tile.label)) as tile (tile.label)}
      <button
        type="button"
        class={`block w-full rounded bg-transparent p-0 text-left transition [appearance:none] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-400/70 ${metricButtonClass(
          tile.label
        )}`}
        onclick={() => onSelectMetric(tile.label)}
      >
        <UsageTile {...tile} />
      </button>
    {/each}
  </div>

  {#if selectedMetric}
    <div class="mt-4 rounded border border-surface-800/70 bg-surface-950/30 p-4">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Detail</p>
          <p class="text-lg font-semibold text-surface-50">{selectedMetric}</p>
        </div>
        <p class="text-xs text-surface-500">Click the tile again to close.</p>
      </div>

      {#if selectedMetric === 'CPU'}
        {#if cpuCoreSeries.length}
          <div class="mt-4 max-h-[22rem] overflow-auto pr-1">
            <div class="grid gap-3 md:grid-cols-2">
              {#each cpuCoreSeries as core (core.id)}
                <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
                  <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0">
                      <p class="truncate text-sm font-semibold text-surface-100">{core.label}</p>
                      <p class="text-xs text-surface-500">{formatFrequency(core.frequencyMhz)}</p>
                    </div>
                    <p class="text-sm font-semibold text-primary-200">{core.latest.toFixed(0)}%</p>
                  </div>
                  <div class="mt-2 h-2 rounded bg-surface-800">
                    <div class="h-full rounded bg-primary-500" style={`width:${core.latest}%`}></div>
                  </div>
                  <div class="relative mt-2 h-14">
                    <InlineSparkline series={core.series} max={100} colorClass="text-primary-400" />
                  </div>
                </div>
              {/each}
            </div>
          </div>
        {:else}
          <p class="mt-3 text-sm text-surface-500">Awaiting per-core CPU telemetry.</p>
        {/if}
      {:else if selectedMetric === 'Network'}
        {#if !networkInterfaces.length}
          <p class="mt-3 text-sm text-surface-500">Awaiting network telemetry.</p>
        {:else}
          <div class="mt-4 space-y-2">
            {#each networkInterfaces as iface (iface.name)}
              <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <p class="text-sm font-semibold text-surface-100">{iface.name}</p>
                    <p class="text-[0.75rem] text-surface-500">{iface.mac ?? 'No MAC'}</p>
                  </div>
                  <span class="rounded-full border border-surface-700 bg-surface-900 px-3 py-1 text-[0.75rem] text-surface-300">
                    RX {formatThroughput(throughputMbps(iface.rx_bytes_per_sec))} / TX {formatThroughput(throughputMbps(iface.tx_bytes_per_sec))}
                  </span>
                </div>
                <p class="text-[0.75rem] text-surface-500">
                  Totals · RX {formatBytes(iface.total_rx_bytes)} / TX {formatBytes(iface.total_tx_bytes)}
                </p>
              </div>
            {/each}
          </div>
        {/if}
      {:else if selectedMetric === 'Disk'}
        {@const disk = telemetrySample.disk}
        {#if !disk || !disk.total_bytes}
          <p class="mt-3 text-sm text-surface-500">No disk telemetry available.</p>
        {:else}
          <div class="mt-4 grid gap-4 md:grid-cols-2">
            <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
              <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Capacity</p>
              <p class="mt-1 text-sm text-surface-200">
                Used {formatBytes(disk.used_bytes)} / {formatBytes(disk.total_bytes)} · Free {formatBytes(disk.free_bytes)}
              </p>
              <div class="mt-3 h-2 rounded bg-surface-800">
                <div class="h-full rounded bg-primary-500" style={`width:${diskPercent(telemetrySample)}%`}></div>
              </div>
            </div>
            <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
              <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Trend</p>
              <div class="relative mt-2 h-20">
                <InlineSparkline series={diskSeries} max={100} colorClass="text-primary-400" />
              </div>
            </div>
          </div>
          {#if diskPartitions.length}
            <div class="mt-4">
              <div class="flex flex-wrap items-center justify-between gap-2">
                <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Partitions</p>
                {#if dataDiskMount}
                  <span class="text-micro uppercase tracking-[0.25em] text-primary-300">Capacity from {dataDiskMount}</span>
                {/if}
              </div>
              <div class="mt-2 space-y-2">
                {#each diskPartitions as partition (partition.mount)}
                  {@const percent = diskPartitionPercent(partition)}
                  <div
                    class={`rounded border border-surface-800/70 bg-surface-950/40 p-3 ${partition.mount === dataDiskMount ? 'border-primary-500/60 bg-primary-500/10' : ''}`}
                  >
                    <div class="flex items-center justify-between gap-3">
                      <div class="min-w-0">
                        <p class="truncate text-sm font-semibold text-surface-100">{partition.mount}</p>
                        <p class="text-[0.75rem] text-surface-500">
                          Used {formatBytes(partition.used_bytes)} / {formatBytes(partition.total_bytes)} · Free {formatBytes(partition.free_bytes)}
                        </p>
                      </div>
                      <span class="rounded-full border border-surface-700 bg-surface-900 px-3 py-1 text-[0.7rem] text-surface-300">
                        {percent.toFixed(1)}%
                      </span>
                    </div>
                    <div class="mt-2 h-1.5 rounded bg-surface-800">
                      <div class="h-full rounded bg-primary-500" style={`width:${percent}%`}></div>
                    </div>
                  </div>
                {/each}
              </div>
            </div>
          {:else}
            <p class="mt-3 text-sm text-surface-500">No partition list available.</p>
          {/if}
        {/if}
      {:else if selectedMetric === 'Memory'}
        <div class="mt-4 grid gap-3 md:grid-cols-3">
          <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Used</p>
            <p class="mt-1 text-sm font-semibold text-surface-100">{formatBytes(telemetrySample.memory.used_bytes)}</p>
          </div>
          <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Free</p>
            <p class="mt-1 text-sm font-semibold text-surface-100">{formatBytes(telemetrySample.memory.free_bytes)}</p>
          </div>
          <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Total</p>
            <p class="mt-1 text-sm font-semibold text-surface-100">{formatBytes(telemetrySample.memory.total_bytes)}</p>
          </div>
        </div>
      {:else if selectedMetric === 'GPU'}
        {@const gpu = telemetrySample.gpu}
        {#if !gpu}
          <p class="mt-3 text-sm text-surface-500">No GPU telemetry available.</p>
        {:else}
          <div class="mt-4 grid gap-3 md:grid-cols-3">
            <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
              <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Util</p>
              <p class="mt-1 text-sm font-semibold text-surface-100">{gpu.usage_percent != null ? `${gpu.usage_percent.toFixed(1)}%` : '—'}</p>
            </div>
            <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
              <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Clock</p>
              <p class="mt-1 text-sm font-semibold text-surface-100">{formatFrequency(gpu.frequency_mhz)}</p>
            </div>
            <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
              <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Temp</p>
              <p class="mt-1 text-sm font-semibold text-surface-100">{gpu.temperature_c != null ? `${gpu.temperature_c.toFixed(1)}°C` : '—'}</p>
            </div>
          </div>
        {/if}
      {:else if selectedMetric === 'Power'}
        {@const watts = powerWatts(telemetrySample)}
        <div class="mt-4 grid gap-3 md:grid-cols-3">
          <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Watts</p>
            <p class="mt-1 text-sm font-semibold text-surface-100">
              {watts != null ? `${watts.toFixed(2)} W` : '—'}
            </p>
          </div>
          <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Volts</p>
            <p class="mt-1 text-sm font-semibold text-surface-100">
              {telemetrySample.power?.volts != null ? `${telemetrySample.power.volts.toFixed(3)} V` : '—'}
            </p>
          </div>
          <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Amps</p>
            <p class="mt-1 text-sm font-semibold text-surface-100">
              {telemetrySample.power?.amps != null ? `${telemetrySample.power.amps.toFixed(3)} A` : '—'}
            </p>
          </div>
        </div>
      {:else if selectedMetric === 'CPU Temp'}
        <div class="mt-4 grid gap-4 md:grid-cols-2">
          <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Current</p>
            <p class="mt-1 text-sm font-semibold text-surface-100">
              {telemetrySample.cpu.temperature_c != null ? `${telemetrySample.cpu.temperature_c.toFixed(1)}°C` : '—'}
            </p>
          </div>
          <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Trend</p>
            <div class="relative mt-2 h-20">
              <InlineSparkline series={cpuTempSeries} max={cpuTempMax} colorClass="text-amber-400" />
            </div>
          </div>
        </div>
      {:else if selectedMetric === 'GPU Memory'}
        {@const memory = telemetrySample.gpu?.memory}
        {#if !memory || !memory.total_bytes}
          <p class="mt-3 text-sm text-surface-500">No GPU memory telemetry available.</p>
        {:else}
          <div class="mt-4 grid gap-3 md:grid-cols-3">
            <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
              <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Used</p>
              <p class="mt-1 text-sm font-semibold text-surface-100">{formatBytes(memory.used_bytes)}</p>
            </div>
            <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
              <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Free</p>
              <p class="mt-1 text-sm font-semibold text-surface-100">{formatBytes(memory.free_bytes)}</p>
            </div>
            <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
              <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Total</p>
              <p class="mt-1 text-sm font-semibold text-surface-100">{formatBytes(memory.total_bytes)}</p>
            </div>
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</Panel>
