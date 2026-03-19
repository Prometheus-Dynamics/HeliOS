import { extractError, extractMessage, summarizeErrorBody } from '$lib/api/errors';
import { apiFetch } from '$lib/api/core/http';
import type { UploadUpdateResponse } from '$lib/ts-bindings/http/client';

export const REQUESTED_BY = 'deck-ui';

export async function downloadSnapshotArchive(id: string): Promise<Response> {
  return apiFetch<Response>(`/device/snapshots/${encodeURIComponent(id)}/download`, { responseMode: 'response' });
}

export async function uploadOtaImage(file: File, headers?: HeadersInit): Promise<UploadUpdateResponse> {
  const form = new FormData();
  form.append('file', file);
  return apiFetch<UploadUpdateResponse>('/ota/upload', {
    method: 'POST',
    body: form,
    headers,
  });
}

export { extractError, extractMessage, summarizeErrorBody };
export { apiFetch };
