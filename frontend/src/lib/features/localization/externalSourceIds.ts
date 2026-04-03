import {
  DEVICE_IMU_EXTERNAL_SOURCE_ID,
  LOCALIZATION_EXTERNAL_STREAM_IDS
} from '$lib/contracts/runtimeContracts';

export { DEVICE_IMU_EXTERNAL_SOURCE_ID };

export const DEVICE_IMU_EXTERNAL_STREAM_ID = LOCALIZATION_EXTERNAL_STREAM_IDS.DEVICE_IMU;

function normalize(value: string | null | undefined): string {
  return String(value ?? '').trim().toLowerCase();
}

export function isDeviceImuExternalStreamId(value: string | null | undefined): boolean {
  return normalize(value) === DEVICE_IMU_EXTERNAL_STREAM_ID;
}

export function hasDeviceImuExternalStreamPrefix(value: string | null | undefined): boolean {
  return normalize(value).startsWith(DEVICE_IMU_EXTERNAL_STREAM_ID);
}

export function isDeviceImuCameraUid(value: string | null | undefined): boolean {
  return normalize(value) === DEVICE_IMU_EXTERNAL_SOURCE_ID;
}
