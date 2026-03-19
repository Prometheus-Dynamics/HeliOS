<script lang="ts">
  import { createEventDispatcher, onDestroy } from 'svelte';
  import { get } from 'svelte/store';

  import { type DeviceSensorKind, type FirmwareUpdatePayload } from '$lib/api/deviceSensorsStream';
  import { createFirmwareController } from '$lib/features/sensors/firmwareController';
  import { createSensorTelemetryController } from '$lib/features/sensors/telemetryController';
  import { createSensorStreamStore } from '$lib/features/sensors/useSensorStream';
  import SensorModalRouter from './peripherals/SensorModalRouter.svelte';
  import type { PeripheralEntry } from '$lib/types/devices';

  type Props = {
    peripheral: PeripheralEntry | null;
  };

  const { peripheral = null }: Props = $props();

  const dispatch = createEventDispatcher<{
    close: void;
    calibrate: { peripheral: PeripheralEntry };
    refresh: void;
  }>();

  const isOpen = $derived(Boolean(peripheral));
  let isLightingPeripheral = $state(false);
  let isFanPeripheral = $state(false);
  let isImuSensor = $state(false);
  let isAccelSensor = $state(false);
  let isGyroSensor = $state(false);
  let isMagSensor = $state(false);
  let isPowerSensor = $state(false);

  const SENSOR_POLL_MS = 100;
  const SENSOR_STREAM_MS = 100;
  const SENSOR_STREAM_FALLBACK_GRACE_MS = 1500;
  const SENSOR_STREAM_STALE_MS = 750;
  const SENSOR_STREAM_RECONNECT_MS = 500;

  let sensorsStreamFailed = $state(false);
  let sensorStreamKindsKey: string | null = null;
  let sensorStreamKinds: DeviceSensorKind[] = [];

  const telemetry = createSensorTelemetryController({ pollMs: SENSOR_POLL_MS });
  const telemetryState = telemetry.state;
  const firmware = createFirmwareController();
  const firmwareState = firmware.state;

  const sensorStream = createSensorStreamStore({
    kinds: [],
    intervalMs: SENSOR_STREAM_MS,
    enabled: false,
    onImu: (payload) => {
      telemetry.handleImuStream(payload);
    },
    onPower: (payload) => {
      telemetry.handlePowerStream(payload);
    },
    onFirmware: (payload: FirmwareUpdatePayload) => {
      firmware.handleStreamUpdate(payload, peripheral);
      const statusKey = (payload?.status ?? '').trim().toLowerCase();
      if (statusKey === 'complete' || statusKey === 'failed') {
        dispatch('refresh');
      }
    },
    onError: (message) => {
      handleStreamError(message);
    },
    onClose: () => {
      handleStreamClose();
    }
  });
  const sensorStreamState = sensorStream.state;

  const modalVariant = $derived(resolveModalVariant());

  $effect(() => {
    sensorStream.setEnabled(false);
    sensorStream.setKinds([]);
    sensorStreamKinds = [];
    sensorStreamKindsKey = null;
    telemetry.stopImuPolling();
    telemetry.stopPowerPolling();
    sensorsStreamFailed = false;

    const typeLabel = (peripheral?.type ?? '').toLowerCase();
    const namespaceLabel = (peripheral?.driverNamespace ?? '').toLowerCase();
    const driverIdLabel = (peripheral?.driverCameraId ?? '').toLowerCase();
    const nameLabel = (peripheral?.name ?? '').toLowerCase();
    const hasFirmwareCard = Boolean(peripheral?.firmware);
    const isFanKind = namespaceLabel.includes('fan') || typeLabel.includes('cooling') || nameLabel.includes('fan');
    const isLightingKind =
      typeLabel.includes('lighting') ||
      typeLabel.includes('led') ||
      namespaceLabel.includes('lighting') ||
      namespaceLabel.includes('led') ||
      nameLabel.includes('lighting') ||
      nameLabel.includes('led');
    const isCoralKind =
      hasFirmwareCard ||
      namespaceLabel.includes('coral') ||
      typeLabel.includes('accelerator') ||
      driverIdLabel.includes('18d1:9301') ||
      driverIdLabel.includes('18d1:9302') ||
      driverIdLabel.includes('1a6e:089a') ||
      driverIdLabel.includes('coral::') ||
      (peripheral?.hardwareId ?? '').toLowerCase().includes('coral::') ||
      (peripheral?.icon?.label ?? '').toLowerCase().includes('coral') ||
      nameLabel.includes('coral') ||
      nameLabel.includes('tpu');
    const isImuKind = typeLabel.includes('imu') || namespaceLabel.includes('imu');
    const isAccelKind = typeLabel.includes('accelerometer');
    const isGyroKind = typeLabel.includes('gyro');
    const isMagKind = namespaceLabel.includes('bmm') || nameLabel.includes('magnetometer') || typeLabel.includes('magnetometer');
    const isPowerKind = typeLabel.includes('power') || namespaceLabel.includes('ina');
    isFanPeripheral = isFanKind && !isLightingKind && !isCoralKind;
    isLightingPeripheral = isLightingKind;
    isAccelSensor = isAccelKind && !isLightingKind && !isCoralKind;
    isGyroSensor = !isAccelSensor && isGyroKind && !isLightingKind && !isCoralKind;
    isMagSensor = !isAccelSensor && !isGyroSensor && isMagKind && !isLightingKind && !isCoralKind;
    isPowerSensor = isPowerKind && !isLightingKind && !isCoralKind;
    isImuSensor = isImuKind && !isCoralKind && !isLightingKind && !isFanKind && !isAccelSensor && !isGyroSensor && !isMagSensor && !isPowerSensor;
    telemetry.setPeripheral(peripheral);
    firmware.setPeripheral(peripheral);
  });

  $effect(() => {
    const needsImu = isOpen && (modalVariant === 'imu' || modalVariant === 'accelerometer' || modalVariant === 'gyroscope' || modalVariant === 'magnetometer');
    const needsPower = isOpen && modalVariant === 'power';
    const needsFirmware = isOpen && Boolean(peripheral?.firmware);

    const wantsStream = isOpen && $telemetryState.streamEnabled && !sensorsStreamFailed && (needsImu || needsPower || needsFirmware);
    const streamState = $sensorStreamState;
    const kinds: DeviceSensorKind[] = [];
    if (needsImu) kinds.push('imu');
    if (needsPower) kinds.push('power');
    if (needsFirmware) kinds.push('firmware');

    if (wantsStream) {
      telemetry.stopImuPolling();
      telemetry.stopPowerPolling();
      const key = kinds.slice().sort().join(',');
      if (sensorStreamKindsKey !== key) {
        sensorStreamKindsKey = key;
        sensorStreamKinds = kinds;
        sensorStream.setKinds(kinds);
      }
      if (!streamState.enabled) {
        sensorStream.setEnabled(true);
      }

      if (streamState.lastMessageAt == null) {
        setTimeout(() => {
          if (!isOpen || !get(telemetryState).streamEnabled || sensorsStreamFailed) return;
          if (get(sensorStreamState).lastMessageAt != null) return;
          if (needsImu) telemetry.startImuPolling();
          if (needsPower) telemetry.startPowerPolling();
        }, SENSOR_STREAM_FALLBACK_GRACE_MS);
      }
      return;
    }

    if (streamState.enabled) {
      sensorStream.setEnabled(false);
    }
    if (needsImu) telemetry.startImuPolling();
    else telemetry.stopImuPolling();

    if (needsPower) telemetry.startPowerPolling();
    else telemetry.stopPowerPolling();
  });

  // Watchdog: if the websocket stops sending for a short period, reconnect so graphs don't stall.
  $effect(() => {
    const needsImu = isOpen && (modalVariant === 'imu' || modalVariant === 'accelerometer' || modalVariant === 'gyroscope' || modalVariant === 'magnetometer');
    const needsPower = isOpen && modalVariant === 'power';
    const needsFirmware = isOpen && Boolean(peripheral?.firmware);
    const wantsStream = isOpen && $telemetryState.streamEnabled && !sensorsStreamFailed && (needsImu || needsPower || needsFirmware);
    if (!wantsStream) return;
    if (!$sensorStreamState.enabled) return;

    const id = setInterval(() => {
      if (!isOpen || !get(telemetryState).streamEnabled || sensorsStreamFailed) return;
      const last = get(sensorStreamState).lastMessageAt;
      if (last == null) return;
      if (Date.now() - last <= SENSOR_STREAM_STALE_MS) return;
      sensorStream.start();
    }, 250);

    return () => clearInterval(id);
  });

  onDestroy(() => {
    sensorStream.destroy();
    telemetry.destroy();
    firmware.destroy();
  });

  function handleStreamError(message: string) {
    sensorsStreamFailed = true;
    telemetry.handleStreamError(sensorStreamKinds, message);
    if (sensorStreamKinds.includes('firmware')) {
      firmware.handleStreamError();
    }
    sensorStream.setEnabled(false);
  }

  function handleStreamClose() {
    if (!isOpen || !get(telemetryState).streamEnabled || sensorsStreamFailed) return;
    if (sensorStreamKinds.length === 0) return;
    setTimeout(() => {
      if (!isOpen || !get(telemetryState).streamEnabled || sensorsStreamFailed) return;
      sensorStream.start();
    }, SENSOR_STREAM_RECONNECT_MS);
  }

  function close() {
    dispatch('close');
  }

  async function handleApplyFirmware(): Promise<void> {
    const ok = await firmware.applyFirmware(peripheral);
    if (ok) dispatch('refresh');
  }

  async function handleSampleRateChange(value: string) {
    const ok = await telemetry.updateSampleRate(value);
    if (ok) dispatch('refresh');
  }

  const handleStreamEnabledChange = (value: boolean) => {
    telemetry.setStreamEnabled(value);
  };
  const handleOrientationLockChange = (value: boolean) => {
    telemetry.setOrientationLock(value);
  };
  const handleFirmwareSelectionChange = (value: string) => {
    firmware.setSelection(value);
  };

  function resolveModalVariant():
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
    | null {
    if (!peripheral) return null;
    const isCoral = (() => {
      const typeLabel = (peripheral.type ?? '').toLowerCase();
      const namespaceLabel = (peripheral.driverNamespace ?? '').toLowerCase();
      const nameLabel = (peripheral.name ?? '').toLowerCase();
      const driverIdLabel = (peripheral.driverCameraId ?? '').toLowerCase();
      const hwIdLabel = (peripheral.hardwareId ?? '').toLowerCase();
      const iconLabel = (peripheral.icon?.label ?? '').toLowerCase();
      const hasFirmware = Boolean(peripheral.firmware);
      return (
        hasFirmware ||
        namespaceLabel.includes('coral') ||
        typeLabel.includes('accelerator') ||
        nameLabel.includes('coral') ||
        nameLabel.includes('tpu') ||
        iconLabel.includes('coral') ||
        driverIdLabel.includes('coral::') ||
        hwIdLabel.includes('coral::') ||
        driverIdLabel.includes('18d1:9301') ||
        driverIdLabel.includes('18d1:9302') ||
        driverIdLabel.includes('1a6e:089a')
      );
    })();
    if (isLightingPeripheral) return 'lighting';
    if (isFanPeripheral) return 'fan';
    if (isCoral) return 'coral';
    if (peripheral.driverNamespace === 'i2c') {
      const typeLabel = (peripheral.type ?? '').toLowerCase();
      const nameLabel = (peripheral.name ?? '').toLowerCase();
      const isCamera = typeLabel.includes('camera') || Boolean(nameLabel.match(/\b(ov|imx)\d+\b/));
      if (isCamera) return 'csi_camera';
    }
    if (isAccelSensor) return 'accelerometer';
    if (isGyroSensor) return 'gyroscope';
    if (isMagSensor) return 'magnetometer';
    if (isPowerSensor) return 'power';
    if (isImuSensor) return 'imu';
    return 'generic';
  }

</script>

{#if isOpen && peripheral}
  <SensorModalRouter
    {peripheral}
    variant={modalVariant}
    orientation={$telemetryState.orientation}
    imu={$telemetryState.imuStatus}
    imuError={$telemetryState.imuError}
    sampleRate={$telemetryState.sampleRate}
    streamEnabled={$telemetryState.streamEnabled}
    orientationLock={$telemetryState.orientationLock}
    firmwareStatus={$firmwareState.status}
    firmwareSelection={$firmwareState.selection}
    firmwareSelectionMissing={$firmwareState.selectionMissing}
    firmwareApplyDisabled={$firmwareState.applyDisabled}
    firmwareLoading={$firmwareState.loading}
    firmwareBusy={$firmwareState.busy}
    firmwareError={$firmwareState.error}
    firmwareProgressPhase={$firmwareState.progressPhase}
    firmwareProgressPct={$firmwareState.progressPct}
    firmwareProgressLabel={$firmwareState.progressLabel}
    firmwareProgressDetail={$firmwareState.progressDetail}
    powerStatus={$telemetryState.powerStatus}
    powerError={$telemetryState.powerError}
    alertTitle={peripheral.firmwareAlert?.title ?? null}
    alertMessage={peripheral.firmwareAlert?.description ?? null}
    alertSeverity={peripheral.firmwareAlert?.severity ?? null}
    onClose={close}
    onCalibrate={null}
    onApplyFirmware={handleApplyFirmware}
    onSampleRateChange={handleSampleRateChange}
    onStreamEnabledChange={handleStreamEnabledChange}
    onOrientationLockChange={handleOrientationLockChange}
    onFirmwareSelectionChange={handleFirmwareSelectionChange}
    onRefresh={() => dispatch('refresh')}
    onRefreshPower={() => void telemetry.refreshPower()}
  />
{/if}
