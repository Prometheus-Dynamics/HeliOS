type UnknownRecord = Record<string, unknown>;

function asRecord(value: unknown): UnknownRecord | null {
  if (!value || typeof value !== 'object') return null;
  return value as UnknownRecord;
}

function trimString(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

function pickFirstNonEmpty(candidates: unknown[]): string | null {
  for (const candidate of candidates) {
    const trimmed = trimString(candidate);
    if (trimmed) return trimmed;
  }
  return null;
}

/**
 * Resolve a user-friendly stream label with the priority:
 * alias -> hardware_id -> uuid.
 *
 * This intentionally prefers an explicit alias over any backend-provided `display` field
 * (which often defaults to hardware/uuid-ish strings).
 */
export function resolveStreamLabel(input: unknown, fallback = 'Stream'): string {
  const obj = asRecord(input);
  if (!obj) return fallback;

  const manifest = asRecord(obj.manifest);
  const identity = manifest ? asRecord((manifest as UnknownRecord).identity) : null;

  const alias =
    pickFirstNonEmpty([
      // DeviceService camera-layout shape
      obj.stream_alias,
      obj.streamAlias,
      // Engine streams (backend may include these fields even if TS bindings lag)
      identity?.alias,
    ]) ?? null;
  if (alias) return alias;

  const hardwareId =
    pickFirstNonEmpty([
      obj.hardware_id,
      obj.hardwareId,
      identity?.hardware_id,
      identity?.hardwareId,
      // OpenAPI-defined identity has { display, keys } only; treat display as a hardware-ish fallback.
      identity?.display,
      // DeviceService camera-layout always includes display_name; treat it as a hardware-ish fallback.
      obj.display_name,
      obj.displayName,
    ]) ?? null;
  if (hardwareId) return hardwareId;

  const uuid =
    pickFirstNonEmpty([
      // Engine streams
      obj.id,
      // DeviceService camera-layout
      obj.stream_id,
      obj.streamId,
      // Some backend payloads include `identity.id`.
      identity?.id,
    ]) ?? null;

  return uuid ?? fallback;
}

export function resolveStreamAlias(input: unknown): string | null {
  const obj = asRecord(input);
  if (!obj) return null;
  const manifest = asRecord(obj.manifest);
  const identity = manifest ? asRecord((manifest as UnknownRecord).identity) : null;

  return (
    pickFirstNonEmpty([
      obj.stream_alias,
      obj.streamAlias,
      identity?.alias,
    ]) ?? null
  );
}

export function resolveStreamUuid(input: unknown): string | null {
  const obj = asRecord(input);
  if (!obj) return null;
  const manifest = asRecord(obj.manifest);
  const identity = manifest ? asRecord((manifest as UnknownRecord).identity) : null;

  return (
    pickFirstNonEmpty([
      obj.id,
      obj.stream_id,
      obj.streamId,
      identity?.id,
    ]) ?? null
  );
}

