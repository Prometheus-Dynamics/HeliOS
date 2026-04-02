import { PeripheralsService } from '$lib/ts-bindings/http/client';
import { fetchPeerStreams } from '$lib/api/peers';
import { loadOwnedStreams } from '$lib/api/streamResources';
import { PeripheralsApi } from '$lib/api/peripheralsApi';
import { DeviceApi } from '$lib/api/deviceApi';
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
  const [metricsResult, peripheralsResult, streamsResult, peerStreamsResult] = await Promise.allSettled([
    DeviceApi.metrics({ timeoutMs: REQUEST_TIMEOUT_MS }),
    PeripheralsApi.listPeripherals({ timeoutMs: REQUEST_TIMEOUT_MS }),
    loadOwnedStreams({ preferCached: false }),
    fetchPeerStreams(REQUEST_TIMEOUT_MS)
  ]);

  const [usbResult, fanResult, lightingResult, i2cResult] =
    peripheralsResult.status === 'fulfilled'
      ? [null, null, null, null]
      : await Promise.allSettled([
          cancellableWithTimeout(() => PeripheralsService.listUsb(), REQUEST_TIMEOUT_MS),
          cancellableWithTimeout(() => PeripheralsService.fanStatus(), REQUEST_TIMEOUT_MS),
          cancellableWithTimeout(() => PeripheralsService.ledStatus(), REQUEST_TIMEOUT_MS),
          cancellableWithTimeout(() => PeripheralsService.listI2C(), I2C_TIMEOUT_MS)
        ]);

  const failedCount = [metricsResult, peripheralsResult, usbResult, fanResult, lightingResult, i2cResult, streamsResult, peerStreamsResult].filter(isRejected).length;
  const attemptedRequests = peripheralsResult.status === 'fulfilled' ? 4 : 8;
  if (failedCount === attemptedRequests) {
    throw new Error('All device data requests failed');
  }

  if (metricsResult.status === 'rejected') {
    console.warn('Device metrics request failed', metricsResult.reason);
  }
  if (peripheralsResult.status === 'rejected') {
    console.warn('Peripherals inventory request failed', peripheralsResult.reason);
  }
  if (isRejected(usbResult)) {
    console.warn('USB inventory request failed', usbResult.reason);
  }
  if (isRejected(fanResult)) {
    console.warn('Fan status request failed', fanResult.reason);
  }
  if (isRejected(lightingResult)) {
    console.warn('Lighting status request failed', lightingResult.reason);
  }
  if (isRejected(i2cResult)) {
    console.warn('I2C inventory request failed', i2cResult.reason);
  }
  if (streamsResult.status === 'rejected') {
    console.warn('Stream list request failed', streamsResult.reason);
  }
  if (peerStreamsResult.status === 'rejected') {
    console.warn('Peer stream list request failed', peerStreamsResult.reason);
  }

  const streamsRaw = streamsResult.status === 'fulfilled' ? streamsResult.value ?? [] : [];
  const streams = Array.isArray(streamsRaw) ? streamsRaw : [];
  const peerStreams = peerStreamsResult.status === 'fulfilled' ? peerStreamsResult.value?.streams ?? [] : [];
  const peripheralPayload = peripheralsResult.status === 'fulfilled' ? peripheralsResult.value : null;
  const peripheralFailure = firstFailureReason([peripheralsResult, usbResult, fanResult, lightingResult, i2cResult]);
  const errors = {
    peripherals: peripheralFailure
  };

  const cameras = buildCameraCards(streams, peerStreams);

  const metricsPayload = metricsResult.status === 'fulfilled' ? metricsResult.value : null;
  const health = extractHealth(metricsPayload);
  const imu = emptyImuStatus();
  const coralSensors = (peripheralPayload?.sensors ?? []).filter((entry) => {
    const driver = (entry?.driver_namespace ?? '').trim().toLowerCase();
    return driver === 'coral';
  });
  const peripherals = extractPeripherals(
    {
      coralSensors,
      usb: peripheralPayload?.usb ?? settledValue(usbResult, []),
      i2c: peripheralPayload?.i2c ?? settledValue(i2cResult, null),
      fan: peripheralPayload?.fan ?? settledValue(fanResult, null),
      lighting: peripheralPayload?.lighting ?? settledValue(lightingResult, null)
    },
    imu
  );
  const tasks = buildTasks(health);
  const summary = buildSummary(cameras, health, metricsPayload?.freshness ?? null);

  return {
    summary,
    cameras,
    peripherals,
    tasks,
    errors
  };
}

export async function fetchDevicesCamerasSnapshot(): Promise<DevicesPayload['cameras']> {
  const [streamsResult, peerStreamsResult] = await Promise.allSettled([
    loadOwnedStreams({ preferCached: false }),
    fetchPeerStreams(REQUEST_TIMEOUT_MS)
  ]);
  const streamsRaw = streamsResult.status === 'fulfilled' ? streamsResult.value ?? [] : [];
  const streams = Array.isArray(streamsRaw) ? streamsRaw : [];
  const peerStreams = peerStreamsResult.status === 'fulfilled' ? peerStreamsResult.value?.streams ?? [] : [];
  return buildCameraCards(streams, peerStreams);
}

export async function fetchDevicesPeripheralsSnapshot(): Promise<DevicesPeripheralsSnapshot> {
  const peripheralsResult = await Promise.allSettled([PeripheralsApi.listPeripherals({ timeoutMs: REQUEST_TIMEOUT_MS })]).then(([result]) => result);
  if (peripheralsResult.status === 'fulfilled') {
    const coralSensors = (peripheralsResult.value?.sensors ?? []).filter((entry) => {
      const driver = (entry?.driver_namespace ?? '').trim().toLowerCase();
      return driver === 'coral';
    });
    const peripherals = extractPeripherals(
      {
        coralSensors,
        usb: peripheralsResult.value?.usb ?? [],
        i2c: peripheralsResult.value?.i2c ?? null,
        fan: peripheralsResult.value?.fan ?? null,
        lighting: peripheralsResult.value?.lighting ?? null
      },
      emptyImuStatus()
    );
    return { peripherals, error: null };
  }

  console.warn('Peripherals inventory request failed', peripheralsResult.reason);
  const [usbResult, fanResult, lightingResult, i2cResult] = await Promise.allSettled([
    cancellableWithTimeout(() => PeripheralsService.listUsb(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.fanStatus(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.ledStatus(), REQUEST_TIMEOUT_MS),
    cancellableWithTimeout(() => PeripheralsService.listI2C(), I2C_TIMEOUT_MS)
  ]);

  const failures = [usbResult, fanResult, lightingResult, i2cResult].filter(isRejected);
  if (failures.length === 4) {
    throw new Error('Peripherals data unavailable');
  }

  if (isRejected(usbResult)) {
    console.warn('USB inventory request failed', usbResult.reason);
  }
  if (isRejected(fanResult)) {
    console.warn('Fan status request failed', fanResult.reason);
  }
  if (isRejected(lightingResult)) {
    console.warn('Lighting status request failed', lightingResult.reason);
  }
  if (isRejected(i2cResult)) {
    console.warn('I2C inventory request failed', i2cResult.reason);
  }

  const peripheralFailure = firstFailureReason([peripheralsResult, usbResult, fanResult, lightingResult, i2cResult]);
  const coralSensors: never[] = [];
  const peripherals = extractPeripherals(
    {
      coralSensors,
      usb: settledValue(usbResult, []),
      i2c: settledValue(i2cResult, null),
      fan: settledValue(fanResult, null),
      lighting: settledValue(lightingResult, null)
    },
    emptyImuStatus()
  );

  return { peripherals, error: peripheralFailure };
}

function isRejected(result: PromiseSettledResult<unknown> | null): result is PromiseRejectedResult {
  return result?.status === 'rejected';
}

function settledValue<T>(result: PromiseSettledResult<T> | null, fallback: T): T {
  if (!result || result.status !== 'fulfilled') {
    return fallback;
  }
  return (result.value ?? fallback) as T;
}

function firstFailureReason(results: Array<PromiseSettledResult<unknown> | null>): string | null {
  for (const result of results) {
    if (result?.status === 'rejected') {
      return formatFailureReason(result.reason);
    }
  }
  return null;
}
