import { DeviceService, PeripheralsService } from '$lib/ts-bindings/http/client';
import { StreamsApi } from '$lib/api/streamsApi';
import { emptyImuStatus } from '$lib/api/systemsPage';
import { cancellableWithTimeout } from '$lib/api/requestUtils';
import { formatFailureReason } from '$lib/api/pagePayload/request';
import { I2C_TIMEOUT_MS, REQUEST_TIMEOUT_MS } from './constants';
import { buildCameraCards, buildSummary, buildTasks, extractHealth, extractPeripherals } from './mappers';
import type { DevicesPayload, DevicesPeripheralsSnapshot } from './types';

/**
 * Compose the device dashboard payload from multiple backend endpoints.
 */
export async function fetchDevicesPageData(): Promise<DevicesPayload> {
  const [metricsResult, peripheralsResult, camerasResult, usbResult, fanResult, lightingResult, i2cResult, streamsResult] = await Promise.allSettled([
    cancellableWithTimeout(() => DeviceService.metrics(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.listPeripherals(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.listCameras(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.listUsb(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.fanStatus(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.ledStatus(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.listI2C(), I2C_TIMEOUT_MS),
    StreamsApi.listStreams({ timeoutMs: REQUEST_TIMEOUT_MS })
  ]);

  const failedCount = [metricsResult, peripheralsResult, camerasResult, usbResult, fanResult, lightingResult, i2cResult, streamsResult].filter((result) => result.status === 'rejected').length;
  if (failedCount === 8) {
    throw new Error('All device data requests failed');
  }

  if (metricsResult.status === 'rejected') {
    console.warn('Device metrics request failed', metricsResult.reason);
  }
  if (peripheralsResult.status === 'rejected') {
    console.warn('Peripherals inventory request failed', peripheralsResult.reason);
  }
  if (camerasResult.status === 'rejected') {
    console.warn('Camera discovery request failed', camerasResult.reason);
  }
  if (usbResult.status === 'rejected') {
    console.warn('USB inventory request failed', usbResult.reason);
  }
  if (fanResult.status === 'rejected') {
    console.warn('Fan status request failed', fanResult.reason);
  }
  if (lightingResult.status === 'rejected') {
    console.warn('Lighting status request failed', lightingResult.reason);
  }
  if (i2cResult.status === 'rejected') {
    console.warn('I2C inventory request failed', i2cResult.reason);
  }
  if (streamsResult.status === 'rejected') {
    console.warn('Stream list request failed', streamsResult.reason);
  }

  const streamsRaw = streamsResult.status === 'fulfilled' ? streamsResult.value ?? [] : [];
  const streams = Array.isArray(streamsRaw) ? streamsRaw : [];
  const peripheralFailure =
    [camerasResult, usbResult, fanResult, lightingResult, i2cResult]
      .filter((result) => result.status === 'rejected')
      .map((result) => formatFailureReason((result as PromiseRejectedResult).reason))[0] ?? null;
  const errors = {
    peripherals: peripheralFailure
  };

  const cameras = buildCameraCards(streams);

  const health = extractHealth(metricsResult.status === 'fulfilled' ? metricsResult.value : null);
  const imu = emptyImuStatus();
  const coralSensors = (peripheralsResult.status === 'fulfilled' ? peripheralsResult.value?.sensors ?? [] : []).filter((entry) => {
    const driver = (entry?.driver_namespace ?? '').trim().toLowerCase();
    return driver === 'coral';
  });
  const peripherals = extractPeripherals(
    {
      coralSensors,
      usb: usbResult.status === 'fulfilled' ? usbResult.value ?? [] : [],
      i2c: i2cResult.status === 'fulfilled' ? i2cResult.value ?? null : null,
      fan: fanResult.status === 'fulfilled' ? fanResult.value ?? null : null,
      lighting: lightingResult.status === 'fulfilled' ? lightingResult.value ?? null : null
    },
    imu
  );
  const tasks = buildTasks(health);
  const summary = buildSummary(cameras, health);

  return {
    summary,
    cameras,
    peripherals,
    tasks,
    errors
  };
}

export async function fetchDevicesCamerasSnapshot(): Promise<DevicesPayload['cameras']> {
  const streamsRaw = await StreamsApi.listStreams({ timeoutMs: REQUEST_TIMEOUT_MS });
  const streams = Array.isArray(streamsRaw) ? streamsRaw : [];
  return buildCameraCards(streams);
}

export async function fetchDevicesPeripheralsSnapshot(): Promise<DevicesPeripheralsSnapshot> {
  const [peripheralsResult, usbResult, fanResult, lightingResult, i2cResult] = await Promise.allSettled([
    cancellableWithTimeout(() => PeripheralsService.listPeripherals(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.listUsb(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.fanStatus(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.ledStatus(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.listI2C(), I2C_TIMEOUT_MS)
  ]);

  const failures = [peripheralsResult, usbResult, fanResult, lightingResult, i2cResult].filter((result) => result.status === 'rejected');
  if (failures.length === 5) {
    throw new Error('Peripherals data unavailable');
  }

  if (peripheralsResult.status === 'rejected') {
    console.warn('Peripherals inventory request failed', peripheralsResult.reason);
  }
  if (usbResult.status === 'rejected') {
    console.warn('USB inventory request failed', usbResult.reason);
  }
  if (fanResult.status === 'rejected') {
    console.warn('Fan status request failed', fanResult.reason);
  }
  if (lightingResult.status === 'rejected') {
    console.warn('Lighting status request failed', lightingResult.reason);
  }
  if (i2cResult.status === 'rejected') {
    console.warn('I2C inventory request failed', i2cResult.reason);
  }

  const peripheralFailure = failures.map((result) => formatFailureReason((result as PromiseRejectedResult).reason))[0] ?? null;
  const coralSensors = (peripheralsResult.status === 'fulfilled' ? peripheralsResult.value?.sensors ?? [] : []).filter((entry) => {
    const driver = (entry?.driver_namespace ?? '').trim().toLowerCase();
    return driver === 'coral';
  });
  const peripherals = extractPeripherals(
    {
      coralSensors,
      usb: usbResult.status === 'fulfilled' ? usbResult.value ?? [] : [],
      i2c: i2cResult.status === 'fulfilled' ? i2cResult.value ?? null : null,
      fan: fanResult.status === 'fulfilled' ? fanResult.value ?? null : null,
      lighting: lightingResult.status === 'fulfilled' ? lightingResult.value ?? null : null
    },
    emptyImuStatus()
  );

  return { peripherals, error: peripheralFailure };
}
