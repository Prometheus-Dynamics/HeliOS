import { apiUrl } from '$lib/api/httpClient';
import { apiFetch, apiFetchResponse } from '$lib/api/core/http';
import type { PipelineOutputSample } from '$lib/features/localization/pipelineSources';

export type LocalizationPipelineStatus = {
  configured: boolean;
  profileId?: string | null;
  lastError?: string | null;
  lastRunMs?: number | null;
};

function profileQuery(profileId?: string | null): string {
  const trimmed = typeof profileId === 'string' ? profileId.trim() : '';
  return trimmed ? `?profile_id=${encodeURIComponent(trimmed)}` : '';
}

async function fetchJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  const response = await apiFetchResponse(url, { method: 'GET', headers: { Accept: 'application/json' }, signal });
  if (response.status === 404) {
    throw new Error('Not found');
  }
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(text || `Request failed (${response.status})`);
  }
  return (await response.json()) as T;
}

export async function fetchLocalizationPipelineStatus(profileId?: string | null): Promise<LocalizationPipelineStatus> {
  const url = apiUrl(`/localization/pipeline/status${profileQuery(profileId)}`);
  return apiFetch<LocalizationPipelineStatus>(url, { method: 'GET', headers: { Accept: 'application/json' } });
}

export async function fetchLocalizationPipelineOutputs(profileId?: string | null): Promise<string[]> {
  const url = apiUrl(`/localization/pipeline/outputs${profileQuery(profileId)}`);
  return apiFetch<string[]>(url, { method: 'GET', headers: { Accept: 'application/json' } });
}

export async function fetchLocalizationPipelineOutputSample(
  outputKey: string,
  profileId?: string | null,
  signal?: AbortSignal
): Promise<PipelineOutputSample | null> {
  const url = apiUrl(
    `/localization/pipeline/outputs/${encodeURIComponent(outputKey)}${profileQuery(profileId)}`
  );
  try {
    return await fetchJson<PipelineOutputSample>(url, signal);
  } catch (error) {
    if (error instanceof Error && error.message === 'Not found') {
      return null;
    }
    throw error;
  }
}
