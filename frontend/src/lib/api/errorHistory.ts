import { apiFetchResponse } from '$lib/api/core/http';
import type { ErrorHistoryEntry, ErrorHistoryResponse } from '$lib/ts-bindings/http/client';

export type { ErrorHistoryEntry, ErrorHistoryResponse };

export async function fetchErrorHistory(limit: number = 200, sinceMs?: number): Promise<ErrorHistoryEntry[]> {
  const url = new URL('http://localhost/v1/errors');
  if (limit > 0) {
    url.searchParams.set('limit', String(limit));
  }
  if (typeof sinceMs === 'number' && Number.isFinite(sinceMs)) {
    url.searchParams.set('since_ms', String(Math.max(0, Math.floor(sinceMs))));
  }
  const path = `${url.pathname}${url.search}`;
  const response = await apiFetchResponse(path, undefined, { timeoutMs: 8000, maxAttempts: 2 }).catch((error) => {
    throw error;
  });
  if (response.status === 404) {
    return [];
  }
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(text?.trim().length ? text : `Error history request failed (${response.status})`);
  }
  const payload = (await response.json().catch(() => ({ items: [] }))) as ErrorHistoryResponse;
  return Array.isArray(payload.items) ? payload.items : [];
}

export async function clearErrorHistory(): Promise<void> {
  const response = await apiFetchResponse('/errors', { method: 'DELETE' }, { timeoutMs: 8000, maxAttempts: 2 });
  if (response.status === 404) {
    return;
  }
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(text?.trim().length ? text : `Error history clear failed (${response.status})`);
  }
}
