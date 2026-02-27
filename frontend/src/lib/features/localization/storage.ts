import { get, writable } from 'svelte/store';
import { readJson, writeJson } from '$lib/utils/storage';
import type { CameraExtrinsics, CustomField } from './types';

export type LocalizationStorageStore = ReturnType<typeof createLocalizationStorageStore>;

const DEFAULT_CUSTOM_FIELDS_KEY = 'helios.localization.customFields.v1';
const DEFAULT_CAMERA_EXTRINSICS_KEY = 'helios.localization.cameraExtrinsics.v1';

export function createLocalizationStorageStore(options: {
  customFieldsKey?: string;
  cameraExtrinsicsKey?: string;
} = {}) {
  const customFieldsKey = options.customFieldsKey ?? DEFAULT_CUSTOM_FIELDS_KEY;
  const cameraExtrinsicsKey = options.cameraExtrinsicsKey ?? DEFAULT_CAMERA_EXTRINSICS_KEY;

  const customFields = writable<CustomField[]>([]);
  const cameraExtrinsics = writable<Record<string, CameraExtrinsics>>({});

  function loadCustomFieldsFromStorage(): CustomField[] {
    const parsed = readJson<unknown>(customFieldsKey, null);
    return parsed ? normalizeCustomFieldList(parsed) : [];
  }

  function persistCustomFields(fields: CustomField[]): void {
    writeJson(customFieldsKey, fields);
  }

  function loadCameraExtrinsicsFromStorage(): Record<string, CameraExtrinsics> {
    const parsed = readJson<unknown>(cameraExtrinsicsKey, null);
    return parsed ? normalizeCameraExtrinsicsMap(parsed) : {};
  }

  function persistCameraExtrinsics(value: Record<string, CameraExtrinsics>): void {
    writeJson(cameraExtrinsicsKey, value);
  }

  function loadFromStorage(): void {
    const fields = loadCustomFieldsFromStorage();
    const extrinsics = loadCameraExtrinsicsFromStorage();
    customFields.set(fields);
    cameraExtrinsics.set(extrinsics);
  }

  function setCustomFields(next: CustomField[]): void {
    customFields.set(next);
    persistCustomFields(next);
  }

  function updateCustomFields(updater: (current: CustomField[]) => CustomField[]): void {
    const next = updater(get(customFields));
    setCustomFields(next);
  }

  function setCameraExtrinsics(next: Record<string, CameraExtrinsics>): void {
    cameraExtrinsics.set(next);
    persistCameraExtrinsics(next);
  }

  function updateCameraExtrinsics(
    updater: (current: Record<string, CameraExtrinsics>) => Record<string, CameraExtrinsics>
  ): void {
    const next = updater(get(cameraExtrinsics));
    setCameraExtrinsics(next);
  }

  return {
    customFields,
    cameraExtrinsics,
    loadFromStorage,
    setCustomFields,
    updateCustomFields,
    setCameraExtrinsics,
    updateCameraExtrinsics
  };
}

function toNumber(value: unknown, fallback: number): number {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return fallback;
}

function normalizeCustomFieldOrigins(value: unknown) {
  if (!Array.isArray(value)) return [];
  const origins = value
    .map((entry) => {
      if (!entry || typeof entry !== 'object') return null;
      const record = entry as Record<string, unknown>;
      const id = String(record.id ?? '').trim();
      const name = String(record.name ?? '').trim() || 'Origin';
      const x = toNumber(record.x, 0);
      const z = toNumber(record.z, 0);
      const yawDeg = toNumber(record.yawDeg ?? record.yaw_deg, 0);
      if (!id) return null;
      return { id, name, x, z, yawDeg };
    })
    .filter((entry): entry is NonNullable<typeof entry> => Boolean(entry));
  return origins;
}

function normalizeCustomFieldList(value: unknown): CustomField[] {
  if (!Array.isArray(value)) return [];
  const fields: CustomField[] = [];
  for (const entry of value) {
    if (!entry || typeof entry !== 'object') continue;
    const record = entry as Record<string, unknown>;
    const id = String(record.id ?? '').trim();
    const name = String(record.name ?? '').trim() || 'Custom field';
    const width = toNumber(record.width, 0);
    const depth = toNumber(record.depth, 0);
    const mapIdRaw = String(record.mapId ?? record.map_id ?? '').trim();
    const mapId = mapIdRaw ? mapIdRaw : null;
    if (!id || width <= 0 || depth <= 0) continue;
    const origins = normalizeCustomFieldOrigins(record.origins);
    fields.push({
      id,
      name,
      width,
      depth,
      origins: origins.length ? origins : [{ id: 'center', name: 'Center', x: 0, z: 0, yawDeg: 0 }],
      mapId
    });
  }
  return fields;
}

function normalizeCameraExtrinsicsMap(value: unknown): Record<string, CameraExtrinsics> {
  if (!value || typeof value !== 'object') return {};
  const record = value as Record<string, unknown>;
  const out: Record<string, CameraExtrinsics> = {};
  for (const [keyRaw, entry] of Object.entries(record)) {
    const key = keyRaw.trim();
    if (!key || !entry || typeof entry !== 'object') continue;
    const obj = entry as Record<string, unknown>;
    const positionObj = obj.position && typeof obj.position === 'object' ? (obj.position as Record<string, unknown>) : {};
    const rotationObj = obj.rotation && typeof obj.rotation === 'object' ? (obj.rotation as Record<string, unknown>) : {};
    out[key] = {
      position: {
        x: toNumber(positionObj.x, 0),
        y: toNumber(positionObj.y, 0),
        z: toNumber(positionObj.z, 0)
      },
      rotation: {
        roll: toNumber(rotationObj.roll, 0),
        pitch: toNumber(rotationObj.pitch, 0),
        yaw: toNumber(rotationObj.yaw, 0)
      }
    };
  }
  return out;
}
