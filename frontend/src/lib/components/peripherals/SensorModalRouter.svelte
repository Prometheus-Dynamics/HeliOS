<script lang="ts">
  import type { PeripheralEntry } from '$lib/types/devices';
  import type { ImuStatus } from '$lib/types/systems';

  import ImuSensorModal from './ImuSensorModal.svelte';
  import CoralSensorModal from './CoralSensorModal.svelte';
  import GenericSensorModal from './GenericSensorModal.svelte';
  import FanSensorModal from './FanSensorModal.svelte';
  import LightingSensorModal from './LightingSensorModal.svelte';
  import AccelerometerSensorModal from './AccelerometerSensorModal.svelte';
  import GyroscopeSensorModal from './GyroscopeSensorModal.svelte';
  import MagnetometerSensorModal from './MagnetometerSensorModal.svelte';
  import CsiCameraSensorModal from './CsiCameraSensorModal.svelte';
  import PowerSensorModal from './PowerSensorModal.svelte';

  type PowerSource = {
    label: string;
    bus?: number | null;
    address?: string | null;
    watts?: number | null;
    volts?: number | null;
    amps?: number | null;
    shuntVolts?: number | null;
  };

  type PowerStatus = {
    watts: number | null;
    volts: number | null;
    amps: number | null;
    updatedAt: string | null;
    sources: PowerSource[];
    errors: string[];
  };

  type Props = {
    peripheral: PeripheralEntry | null;
    variant:
      | 'fan'
      | 'lighting'
      | 'imu'
      | 'coral'
      | 'accelerometer'
      | 'gyroscope'
      | 'magnetometer'
      | 'csi_camera'
      | 'power'
      | 'generic'
      | null;
    orientation?: { roll: number; pitch: number; yaw: number };
    imu?: ImuStatus | null;
    imuError?: string | null;
    sampleRate?: string;
    streamEnabled?: boolean;
    orientationLock?: boolean;
    firmwareStatus?: PeripheralEntry['firmware'] | null;
    firmwareSelection?: string;
    firmwareSelectionMissing?: boolean;
    firmwareApplyDisabled?: boolean;
    firmwareLoading?: boolean;
    firmwareBusy?: boolean;
    firmwareError?: string | null;
    firmwareProgressPhase?: 'idle' | 'queued' | 'flashing' | 'applying' | 'complete' | 'failed';
    firmwareProgressPct?: number;
    firmwareProgressLabel?: string | null;
    firmwareProgressDetail?: string | null;
    powerStatus?: PowerStatus | null;
    powerError?: string | null;
    onClose: () => void;
    onCalibrate?: (() => void) | null;
    onApplyFirmware?: () => void;
    onSampleRateChange?: (value: string) => void;
    onStreamEnabledChange?: (value: boolean) => void;
    onOrientationLockChange?: (value: boolean) => void;
    onFirmwareSelectionChange?: (value: string) => void;
    onRefresh?: () => void;
    onRefreshPower?: () => void;
    alertTitle?: string | null;
    alertMessage?: string | null;
    alertSeverity?: 'error' | 'warning' | null;
  };

  const {
    peripheral,
    variant,
    orientation = { roll: 0, pitch: 0, yaw: 0 },
    imu = null,
    imuError = null,
    sampleRate = '100ms',
    streamEnabled = true,
    orientationLock = false,
    firmwareStatus = null,
    firmwareSelection = '',
    firmwareSelectionMissing = false,
    firmwareApplyDisabled = false,
    firmwareLoading = false,
    firmwareBusy = false,
    firmwareError = null,
    firmwareProgressPhase = 'idle',
    firmwareProgressPct = 0,
    firmwareProgressLabel = null,
    firmwareProgressDetail = null,
    powerStatus = null,
    powerError = null,
    onClose,
    onCalibrate = null,
    onApplyFirmware,
    onSampleRateChange,
    onStreamEnabledChange,
    onOrientationLockChange,
    onFirmwareSelectionChange,
    onRefresh,
    onRefreshPower,
    alertTitle = null,
    alertMessage = null,
    alertSeverity = null
  }: Props = $props();

  const emptyPowerStatus: PowerStatus = {
    watts: null,
    volts: null,
    amps: null,
    updatedAt: null,
    sources: [],
    errors: []
  };
</script>

{#if peripheral}
  {#if variant === 'imu'}
    <ImuSensorModal
      {peripheral}
      {orientation}
      imu={imu}
      {imuError}
      {sampleRate}
      {streamEnabled}
      {orientationLock}
      {firmwareStatus}
      {firmwareSelection}
      {firmwareSelectionMissing}
      {firmwareApplyDisabled}
      {firmwareLoading}
      {firmwareBusy}
      {firmwareError}
      {firmwareProgressPhase}
      {firmwareProgressPct}
      {firmwareProgressLabel}
      {firmwareProgressDetail}
      onClose={onClose}
      onCalibrate={onCalibrate}
      onApplyFirmware={onApplyFirmware}
      onSampleRateChange={onSampleRateChange}
      onStreamEnabledChange={onStreamEnabledChange}
      onOrientationLockChange={onOrientationLockChange}
      onFirmwareSelectionChange={onFirmwareSelectionChange}
    />
  {:else if variant === 'lighting'}
    <LightingSensorModal {peripheral} onClose={onClose} onRefresh={onRefresh} />
  {:else if variant === 'fan'}
    <FanSensorModal {peripheral} onClose={onClose} onRefresh={onRefresh} />
  {:else if variant === 'accelerometer'}
    <AccelerometerSensorModal
      {peripheral}
      imu={imu}
      {imuError}
      onClose={onClose}
      onCalibrate={onCalibrate}
    />
  {:else if variant === 'gyroscope'}
    <GyroscopeSensorModal
      {peripheral}
      imu={imu}
      {imuError}
      onClose={onClose}
      onCalibrate={onCalibrate}
    />
  {:else if variant === 'magnetometer'}
    <MagnetometerSensorModal
      {peripheral}
      imu={imu}
      {imuError}
      onClose={onClose}
      onCalibrate={onCalibrate}
    />
  {:else if variant === 'csi_camera'}
    <CsiCameraSensorModal {peripheral} onClose={onClose} />
  {:else if variant === 'power'}
    <PowerSensorModal
      {peripheral}
      status={powerStatus ?? emptyPowerStatus}
      {powerError}
      onClose={onClose}
      onCalibrate={onCalibrate}
      onRefresh={onRefreshPower}
    />
  {:else if variant === 'coral'}
    <CoralSensorModal
      {peripheral}
      {firmwareStatus}
      {firmwareSelection}
      {firmwareSelectionMissing}
      {firmwareApplyDisabled}
      {firmwareLoading}
      {firmwareBusy}
      {firmwareError}
      {firmwareProgressPhase}
      {firmwareProgressPct}
      {firmwareProgressLabel}
      {firmwareProgressDetail}
      {alertTitle}
      {alertMessage}
      {alertSeverity}
      onClose={onClose}
      onApplyFirmware={onApplyFirmware}
      onFirmwareSelectionChange={onFirmwareSelectionChange}
    />
  {:else}
    <GenericSensorModal {peripheral} onClose={onClose} onCalibrate={onCalibrate} />
  {/if}
{/if}
