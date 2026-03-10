<script lang="ts">
  import { browser } from '$app/environment';
  import type { PeripheralEntry, SensorOrientation } from '$lib/types/devices';
  import type { ImuStatus } from '$lib/types/systems';
  import { createLazySvelteComponentLoader } from '$lib/utils/lazySvelteComponent';

  import SensorModalShell from './SensorModalShell.svelte';
  import PeripheralStats from './PeripheralStats.svelte';
  import FirmwareUpdaterPanel from './FirmwareUpdaterPanel.svelte';
  import SensorTelemetryPanel from './SensorTelemetryPanel.svelte';

  type ImuOrientationViewerComponent = (typeof import('$lib/components/ImuOrientationViewer.svelte'))['default'];

  const imuOrientationViewerLoader = createLazySvelteComponentLoader<ImuOrientationViewerComponent>(
    () => import('$lib/components/ImuOrientationViewer.svelte')
  );

  type FirmwareStatus = PeripheralEntry['firmware'];

  type Props = {
    peripheral: PeripheralEntry;
    orientation: SensorOrientation;
    imu?: ImuStatus | null;
    imuError?: string | null;
    sampleRate: string;
    streamEnabled: boolean;
    orientationLock: boolean;
    firmwareStatus: FirmwareStatus | null;
    firmwareSelection: string;
    firmwareSelectionMissing: boolean;
    firmwareApplyDisabled: boolean;
    firmwareLoading: boolean;
    firmwareBusy: boolean;
    firmwareError: string | null;
    firmwareProgressPhase: 'idle' | 'queued' | 'flashing' | 'applying' | 'complete' | 'failed';
    firmwareProgressPct: number;
    firmwareProgressLabel: string | null;
    firmwareProgressDetail: string | null;
    onClose: () => void;
    onCalibrate?: (() => void) | null;
    onApplyFirmware?: () => void;
    onSampleRateChange?: (value: string) => void;
    onStreamEnabledChange?: (value: boolean) => void;
    onOrientationLockChange?: (value: boolean) => void;
    onFirmwareSelectionChange?: (value: string) => void;
  };

  let {
    peripheral,
    orientation,
    imu = null,
    imuError = null,
    sampleRate,
    streamEnabled,
    orientationLock,
    firmwareStatus,
    firmwareSelection,
    firmwareSelectionMissing,
    firmwareApplyDisabled,
    firmwareLoading,
    firmwareBusy,
    firmwareError,
    firmwareProgressPhase,
    firmwareProgressPct,
    firmwareProgressLabel,
    firmwareProgressDetail,
    onClose,
    onCalibrate = null,
    onApplyFirmware,
    onSampleRateChange,
    onStreamEnabledChange,
    onOrientationLockChange,
    onFirmwareSelectionChange
  }: Props = $props();

  let ImuOrientationViewerComponent = $state<ImuOrientationViewerComponent | null>(
    imuOrientationViewerLoader.current()
  );

  const statusLabel = $derived(() => {
    if (!imu) return 'No samples yet';
    if (imu.lastError && imu.lastError.trim().length) return `Error · ${imu.lastError}`;
    return imu.hasSample ? 'Active' : 'Idle · awaiting samples';
  });

  let lastHistoryAtMs = $state<number>(0);
  let rollSeries = $state<number[]>([]);
  let pitchSeries = $state<number[]>([]);
  let yawSeries = $state<number[]>([]);
  const HISTORY_MAX = 180;

  $effect(() => {
    if (!imu?.hasSample) return;
    const now = Date.now();
    if (now - lastHistoryAtMs < 40) return;
    lastHistoryAtMs = now;
    rollSeries = [...rollSeries, orientation.roll].slice(-HISTORY_MAX);
    pitchSeries = [...pitchSeries, orientation.pitch].slice(-HISTORY_MAX);
    yawSeries = [...yawSeries, orientation.yaw].slice(-HISTORY_MAX);
  });

  async function ensureImuOrientationViewer(): Promise<void> {
    ImuOrientationViewerComponent ??= await imuOrientationViewerLoader.load();
  }

  $effect(() => {
    if (!browser) return;
    void ensureImuOrientationViewer();
  });
</script>

<SensorModalShell {peripheral} {onClose}>
  {#snippet viewer()}
    <div class="space-y-4">
      <div class="rounded-xl border border-surface-800 bg-surface-950/70 p-4 shadow-inner shadow-black/30">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div class="space-y-1">
            <p class="text-micro uppercase tracking-[0.3em] text-surface-500">IMU</p>
            <p class="text-base font-semibold text-surface-100">{statusLabel}</p>
            {#if imuError && !(imu?.lastError && imu.lastError.trim().length)}
              <p class="text-xs text-error-300">{imuError}</p>
            {/if}
          </div>
          <div class="text-right text-xs text-surface-500">
            <p>Interval {sampleRate}</p>
            <p class="text-micro uppercase tracking-[0.25em] text-surface-600">Updated {imu?.updatedAt ?? '—'}</p>
          </div>
        </div>
      </div>

      {#if ImuOrientationViewerComponent}
        <ImuOrientationViewerComponent orientation={orientation} />
      {:else}
        <div class="flex min-h-[18rem] items-center justify-center rounded-xl border border-surface-800 bg-surface-950/50 text-xs text-surface-500">
          Loading IMU viewer…
        </div>
      {/if}
      <PeripheralStats {peripheral} calibrationLabel="Interval" calibrationValue="5ms" />
    </div>
  {/snippet}

  {#snippet config()}
    <SensorTelemetryPanel
      {peripheral}
      {sampleRate}
      {streamEnabled}
      {orientationLock}
      rollSeries={rollSeries}
      pitchSeries={pitchSeries}
      yawSeries={yawSeries}
      onSampleRateChange={onSampleRateChange}
      onStreamEnabledChange={onStreamEnabledChange}
      onOrientationLockChange={onOrientationLockChange}
    >
      {#snippet firmware()}
        <FirmwareUpdaterPanel
          title="Firmware"
          {firmwareStatus}
          firmwareSelection={firmwareSelection}
          {firmwareSelectionMissing}
          {firmwareApplyDisabled}
          {firmwareLoading}
          {firmwareBusy}
          {firmwareError}
          {firmwareProgressPhase}
          {firmwareProgressPct}
          {firmwareProgressLabel}
          {firmwareProgressDetail}
          onApplyFirmware={onApplyFirmware}
          onFirmwareSelectionChange={onFirmwareSelectionChange}
        />
      {/snippet}
    </SensorTelemetryPanel>
  {/snippet}

  {#snippet footer()}
    <div class="flex flex-wrap gap-3 text-[0.75rem] uppercase tracking-[0.2em] text-surface-400">
      <p class="w-full text-micro text-surface-500">Orientation</p>
      <div class="flex flex-wrap gap-3 text-[0.85rem] text-surface-300">
        <span>
          Roll <strong class="ml-1 text-base font-semibold" style="color: var(--axis-roll)">{orientation.roll.toFixed(1)}°</strong>
        </span>
        <span>
          Pitch <strong class="ml-1 text-base font-semibold" style="color: var(--axis-pitch)">{orientation.pitch.toFixed(1)}°</strong>
        </span>
        <span>
          Yaw <strong class="ml-1 text-base font-semibold" style="color: var(--axis-yaw)">{orientation.yaw.toFixed(1)}°</strong>
        </span>
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-3">
      {#if onCalibrate}
        <button class="btn btn-xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={onCalibrate}>
          Calibrate
        </button>
      {/if}
    </div>
  {/snippet}
</SensorModalShell>
