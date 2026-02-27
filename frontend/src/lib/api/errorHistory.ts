import { getHttpClientBase } from '$lib/api/httpClient';
import { fetchWithRetry } from '$lib/api/requestUtils';

export type ErrorHistoryEntry = {
  id: string;
  status?: number | null;
  code: string;
  error: string;
  details?: string | null;
  timestamp_ms: number;
  source?: string | null;
  operation?: string | null;
  request_id?: string | null;
  trace_id?: string | null;
  retryable?: boolean | null;
  remediation?: string | null;
  reported_by?: string | null;
  transport?: string | null;
};

export type ErrorHistoryResponse = {
  items: ErrorHistoryEntry[];
};

export async function fetchErrorHistory(limit: number = 200, sinceMs?: number): Promise<ErrorHistoryEntry[]> {
  const base = getHttpClientBase();
  const url = new URL(`${base}/v1/errors`);
  if (limit > 0) {
    url.searchParams.set('limit', String(limit));
  }
  if (typeof sinceMs === 'number' && Number.isFinite(sinceMs)) {
    url.searchParams.set('since_ms', String(Math.max(0, Math.floor(sinceMs))));
  }
  const response = await fetchWithRetry(url.toString(), { headers: { accept: 'application/json' } }, { timeoutMs: 8000, maxAttempts: 2 });
  if (!response.ok) {
    throw new Error(`Failed to fetch error history (${response.status})`);
  }
  const payload = (await response.json()) as ErrorHistoryResponse;
  return Array.isArray(payload.items) ? payload.items : [];
}

export async function clearErrorHistory(): Promise<void> {
  const base = getHttpClientBase();
  const response = await fetchWithRetry(`${base}/v1/errors`, { method: 'DELETE' }, { timeoutMs: 8000, maxAttempts: 2 });
  if (!response.ok) {
    throw new Error(`Failed to clear error history (${response.status})`);
  }
}
