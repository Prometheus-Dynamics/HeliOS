import { apiUrl } from '$lib/api/httpClient';
import { apiFetch, apiFetchResponse } from '$lib/api/core/http';
import { summarizeErrorBody } from '$lib/api/errors';
import { normalizeUploadError, uploadSizeHeaders } from '$lib/api/uploadIntegrity';

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
  const text = await response.text().catch(() => '');
  throw new Error(summarizeErrorBody(text, `${response.status} ${response.statusText}`));
}

export async function listFieldMaps(signal?: AbortSignal): Promise<FieldMapSummary[]> {
  return apiFetch<FieldMapSummary[]>(apiUrl('/localization/maps'), { signal, cache: 'no-store' });
}

export async function fetchFieldMap(id: string, signal?: AbortSignal): Promise<FieldMapDocument> {
  return apiFetch<FieldMapDocument>(apiUrl(`/localization/maps/${encodeURIComponent(id)}`), { signal, cache: 'no-store' });
}

export async function uploadLimelightFmap(file: File, signal?: AbortSignal): Promise<FieldMapSummary> {
  const form = new FormData();
  form.append('file', file, file.name);

  let response: Response;
  try {
    response = await apiFetchResponse(apiUrl('/localization/maps/upload'), {
      method: 'POST',
      body: form,
      headers: uploadSizeHeaders(file),
      signal
    });
  } catch (error) {
    throw normalizeUploadError(error, 'Field map upload');
  }

  return await readJsonOrThrow<FieldMapSummary>(response);
}
