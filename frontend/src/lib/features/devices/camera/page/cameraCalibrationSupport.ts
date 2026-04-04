import type { IpaStatus } from '$lib/features/devices/camera/cameraCalibrationTypes';

export type CalibrationImportSource = {
  id: string;
  label: string;
  calibration: import('$lib/features/devices/camera/cameraCalibrationTypes').CalibrationParams;
};

type IpaStatusFile = IpaStatus['files'][number];

export const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

export const asFiniteNumber = (value: unknown): number | null => {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : null;
};

export const asFixedNumberArray = (value: unknown, expectedLength: number): number[] | null => {
  if (!Array.isArray(value) || value.length !== expectedLength) return null;
  const normalized = value.map((entry) => Number(entry));
  return normalized.every((entry) => Number.isFinite(entry)) ? normalized : null;
};

export const asCcmMatrix = (value: unknown): number[][] | null => {
  if (!Array.isArray(value) || value.length !== 3) return null;
  const rows = value.map((entry) => asFixedNumberArray(entry, 3));
  return rows.every((entry): entry is number[] => entry !== null) ? rows : null;
};

export const ccmMatrixFromFlat = (value: number[]): number[][] => [
  [value[0] ?? 1, value[1] ?? 0, value[2] ?? 0],
  [value[3] ?? 0, value[4] ?? 1, value[5] ?? 0],
  [value[6] ?? 0, value[7] ?? 0, value[8] ?? 1]
];

const normalizeIpaStatusFile = (value: unknown): IpaStatusFile | null => {
  const record = asRecord(value);
  if (!record) return null;
  const target = typeof record.target === 'string' ? record.target.trim() : '';
  const path = typeof record.path === 'string' ? record.path.trim() : '';
  if (!target || !path) return null;
  return {
    target,
    path,
    exists: Boolean(record.exists),
    ccm: record.ccm === null ? null : (asFixedNumberArray(record.ccm, 9) ?? undefined),
    ccmCt: record.ccmCt === null ? null : (asFiniteNumber(record.ccmCt) ?? undefined)
  };
};

export const normalizeIpaStatus = (value: unknown): IpaStatus | null => {
  const record = asRecord(value);
  if (!record) return null;
  const files = Array.isArray(record.files)
    ? record.files.map((entry) => normalizeIpaStatusFile(entry)).filter((entry): entry is IpaStatusFile => entry !== null)
    : [];
  return { files };
};
