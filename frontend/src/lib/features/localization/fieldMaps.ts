import { apiUrl } from '$lib/api/httpClient';

export type FieldMapSummary = {
  id: string;
  name: string;
  widthM: number;
  depthM: number;
  markerCount: number;
  sourceKind: string;
};

export type FieldQuaternion = {
  x: number;
  y: number;
  z: number;
  w: number;
};

export type FieldMapMarker = {
  id: number;
  family: string;
  sizeM: number;
  position: [number, number, number];
  quaternion: FieldQuaternion;
  headingDeg?: number | null;
  tagBits?: { width: number; border: number; rows: string[] } | null;
  unique: boolean;
};

export type FieldMapOverlay = {
  dataUrl: string;
  mimeType?: string | null;
  opacity?: number | null;
  widthM?: number | null;
  depthM?: number | null;
  offsetXM?: number | null;
  offsetZM?: number | null;
  rotationDeg?: number | null;
};

export type FieldMapDocument = {
  schemaVersion: number;
  id: string;
  name: string;
  widthM: number;
  depthM: number;
  markers: FieldMapMarker[];
  source: { kind: string } & Record<string, unknown>;
  overlay?: FieldMapOverlay | null;
};

async function readJsonOrThrow<T>(response: Response): Promise<T> {
  if (response.ok) {
    return (await response.json()) as T;
  }
  let message = `${response.status} ${response.statusText}`;
  try {
    const payload = (await response.json()) as { error?: string; message?: string };
    message = payload.error || payload.message || message;
  } catch {
    // ignore
  }
  throw new Error(message);
}

export async function listFieldMaps(signal?: AbortSignal): Promise<FieldMapSummary[]> {
  const response = await fetch(apiUrl('/localization/maps'), { signal, cache: 'no-store' });
  return await readJsonOrThrow<FieldMapSummary[]>(response);
}

export async function fetchFieldMap(id: string, signal?: AbortSignal): Promise<FieldMapDocument> {
  const response = await fetch(apiUrl(`/localization/maps/${encodeURIComponent(id)}`), { signal, cache: 'no-store' });
  return await readJsonOrThrow<FieldMapDocument>(response);
}

export async function uploadLimelightFmap(file: File, signal?: AbortSignal): Promise<FieldMapSummary> {
  const form = new FormData();
  form.append('file', file, file.name);

  const response = await fetch(apiUrl('/localization/maps/upload'), {
    method: 'POST',
    body: form,
    signal
  });

  return await readJsonOrThrow<FieldMapSummary>(response);
}
