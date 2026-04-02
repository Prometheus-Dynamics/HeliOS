import { describe, expect, test } from 'bun:test';

import {
  DEVICE_IMU_EXTERNAL_SOURCE_ID,
  DEVICE_IMU_EXTERNAL_STREAM_ID,
  hasDeviceImuExternalStreamPrefix,
  isDeviceImuCameraUid,
  isDeviceImuExternalStreamId
} from './externalSourceIds';

describe('localization external source ids', () => {
  test('keeps the generated device IMU ids stable', () => {
    expect(DEVICE_IMU_EXTERNAL_SOURCE_ID).toBe('imu');
    expect(DEVICE_IMU_EXTERNAL_STREAM_ID).toBe('external:imu');
  });

  test('matches exact and prefixed device IMU stream ids', () => {
    expect(isDeviceImuExternalStreamId('external:imu')).toBe(true);
    expect(isDeviceImuExternalStreamId(' external:imu ')).toBe(true);
    expect(isDeviceImuExternalStreamId('external:imu:pose')).toBe(false);

    expect(hasDeviceImuExternalStreamPrefix('external:imu')).toBe(true);
    expect(hasDeviceImuExternalStreamPrefix('external:imu:pose')).toBe(true);
    expect(hasDeviceImuExternalStreamPrefix('external:media-imu-123')).toBe(false);
  });

  test('matches the device IMU camera uid only', () => {
    expect(isDeviceImuCameraUid('imu')).toBe(true);
    expect(isDeviceImuCameraUid(' IMU ')).toBe(true);
    expect(isDeviceImuCameraUid('camera-1')).toBe(false);
  });
});
