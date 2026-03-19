import { apiUrl } from '$lib/api/httpClient';
import { apiFetch, apiFetchResponse } from '$lib/api/core/http';
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

type ValidationEntry = {
  path?: string | null;
  code?: string | null;
  message?: string | null;
};

type ErrorPayload = {
  error?: string | null;
  message?: string | null;
  details?: string | null;
  issues?: ValidationEntry[] | null;
  warnings?: ValidationEntry[] | null;
};

function summarizeValidation(entries: ValidationEntry[] | null | undefined): string | null {
  if (!Array.isArray(entries) || entries.length === 0) return null;
  const messages = entries
    .map((entry) => {
      const message = String(entry?.message ?? '').trim();
      if (!message) return null;
      const path = String(entry?.path ?? '').trim();
      return path ? `${path}: ${message}` : message;
    })
    .filter((entry): entry is string => Boolean(entry));
  if (!messages.length) return null;
  const primary = messages.slice(0, 3).join('; ');
  const remaining = messages.length - 3;
  return remaining > 0 ? `${primary}; +${remaining} more` : primary;
}

async function readJsonOrThrow<T>(response: Response): Promise<T> {
  if (response.ok) {
    return (await response.json()) as T;
  }
  let message = `${response.status} ${response.statusText}`;
  try {
    const payload = (await response.json()) as ErrorPayload;
    const primary =
      String(payload?.error ?? '').trim() ||
      String(payload?.message ?? '').trim() ||
      String(payload?.details ?? '').trim();
    const issues = summarizeValidation(payload?.issues ?? null);
    message = primary && issues ? `${primary} ${issues}` : primary || issues || message;
  } catch {
    // ignore
  }
  throw new Error(message);
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
