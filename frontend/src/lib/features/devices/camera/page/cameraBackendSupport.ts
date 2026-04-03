import type {
  BackendHandle,
  CaptureDescriptor,
  ControlMeta,
  ProbedBackend,
  ProbedDevice,
  StreamInfo,
  StreamManifest,
  Mode,
  Interval
} from '$lib/api/client';

export const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

export const asTrimmedString = (value: unknown): string =>
  typeof value === 'string' ? value.trim() : '';

export const normalizeFormatKey = (value: unknown): string => {
  const trimmed = asTrimmedString(value);
  if (!trimmed) return '';
  return trimmed.split(/\s+/)[0]?.toUpperCase() ?? '';
};

export const asPositiveNumber = (value: unknown): number | null => {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric > 0 ? numeric : null;
};

export const asJpegQuality = (value: unknown): number | null => {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? Math.min(100, Math.max(1, Math.trunc(numeric))) : null;
};

export const asInterval = (value: unknown): Interval | null => {
  const record = asRecord(value);
  if (!record) return null;
  return typeof record.numerator === 'number' && typeof record.denominator === 'number'
    ? (record as unknown as Interval)
    : null;
};

export const identityRecordFor = (manifest?: StreamManifest | null): Record<string, unknown> | null =>
  asRecord(manifest?.identity) ?? asRecord(asRecord(manifest)?.identity);

export const captureRecordFor = (manifest?: StreamManifest | null): Record<string, unknown> | null =>
  asRecord(manifest?.capture) ?? asRecord(asRecord(manifest)?.capture);

export const resolvedCodecId = (value: { codecId?: string | null } | null | undefined): string =>
  asTrimmedString(value?.codecId);

export function isFileBackend(kind: unknown): boolean {
  return String(kind ?? '').toLowerCase() === 'file';
}

export function isNetcamBackend(kind: unknown): boolean {
  return String(kind ?? '').toLowerCase() === 'netcam';
}

export function extractFileHandle(handle: unknown): { fps?: number; loop_forever?: boolean; paths?: string[] } | null {
  const record = asRecord(handle);
  if (!record) return null;
  const direct = String(record.type ?? '').toLowerCase() === 'file' ? record : null;
  const legacy = asRecord(record.File);
  const resolved = direct ?? legacy;
  if (!resolved) return null;
  return {
    fps: asPositiveNumber(resolved.fps) ?? undefined,
    loop_forever: typeof resolved.loop_forever === "boolean" ? resolved.loop_forever : undefined,
    paths: Array.isArray(resolved.paths)
      ? resolved.paths.filter((path): path is string => typeof path === 'string' && path.trim().length > 0)
      : undefined
  };
}

function buildNetcamHandle(handle: unknown, fallbackUrl: string): BackendHandle {
  const record = asRecord(handle);
  const direct = record && String(record.type ?? '').toLowerCase() === 'netcam' ? record : null;
  const legacy = record ? asRecord(record.Netcam) : null;
  const resolved = direct ?? legacy;
  return {
    Netcam: {
      url: asTrimmedString(resolved?.url) || fallbackUrl,
      width: Math.max(0, Math.trunc(asPositiveNumber(resolved?.width) ?? 0)),
      height: Math.max(0, Math.trunc(asPositiveNumber(resolved?.height) ?? 0)),
      fps: Math.max(1, Math.trunc(asPositiveNumber(resolved?.fps) ?? 30))
    }
  };
}

function descriptorFromStreamOrManifest(stream: StreamInfo | null, manifest?: StreamManifest | null): CaptureDescriptor {
  const streamDescriptor = stream?.descriptor ?? null;
  const streamModes: Mode[] = Array.isArray(streamDescriptor?.modes)
    ? streamDescriptor.modes.filter(Boolean)
    : [];
  const streamControls: ControlMeta[] = Array.isArray(streamDescriptor?.controls)
    ? streamDescriptor.controls
    : [];
  if (streamModes.length) {
    return { modes: streamModes, controls: streamControls };
  }

  const captureMode = captureRecordFor(manifest)?.mode ?? null;
  const captureModeRecord = asRecord(captureMode);
  if (captureModeRecord) {
    const synthesized = {
      id: captureMode,
      format: captureModeRecord.format ?? null,
      intervals: captureModeRecord.interval ? [captureModeRecord.interval as Interval] : [],
      interval_stepwise: null
    } as unknown as Mode;
    return { modes: [synthesized], controls: streamControls };
  }

  return { modes: [], controls: streamControls };
}

export function buildFileHandle(handle: unknown): BackendHandle {
  const fileHandle = extractFileHandle(handle);
  return {
    File: {
      fps: Math.max(1, Math.trunc(fileHandle?.fps ?? 30)),
      loop_forever: fileHandle?.loop_forever ?? true,
      paths: fileHandle?.paths ?? []
    }
  };
}

export function buildNetcamBackend(stream: StreamInfo | null, manifest?: StreamManifest | null): ProbedDevice | null {
  if (!isNetcamBackend(manifest?.capture?.backend)) return null;
  const descriptor = descriptorFromStreamOrManifest(stream, manifest);
  const identity = identityRecordFor(manifest);
  const display =
    (asTrimmedString(identity?.alias) || null) ??
    (asTrimmedString(identity?.hardware_id) || null) ??
    'Netcam';
  const keys =
    Array.isArray(manifest?.capture?.device_keys) && manifest?.capture?.device_keys?.length
      ? manifest.capture.device_keys
      : Array.isArray(manifest?.identity?.keys) && manifest?.identity?.keys?.length
        ? manifest.identity.keys
        : ['netcam'];
  const backend: ProbedBackend = {
    descriptor,
    handle: buildNetcamHandle(manifest?.capture?.handle, keys[0] ?? ''),
    kind: 'Netcam',
    properties: []
  };
  return { identity: { display, keys }, backends: [backend] };
}

export function buildFileBackend(stream: StreamInfo | null, manifest?: StreamManifest | null): ProbedDevice | null {
  if (!isFileBackend(manifest?.capture?.backend)) return null;
  const descriptor = descriptorFromStreamOrManifest(stream, manifest);
  const identity = identityRecordFor(manifest);
  const display =
    (asTrimmedString(identity?.alias) || null) ??
    (asTrimmedString(identity?.hardware_id) || null) ??
    'Media library';
  const keys = Array.isArray(manifest?.identity?.keys) && manifest?.identity?.keys.length ? manifest.identity.keys : ['media-file'];
  const backend: ProbedBackend = {
    descriptor,
    handle: buildFileHandle(manifest?.capture?.handle),
    kind: 'File',
    properties: []
  };
  return { identity: { display, keys }, backends: [backend] };
}
