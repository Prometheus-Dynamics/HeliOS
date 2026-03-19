import { apiFetchResponse } from '$lib/api/core/http';
import type { ConsoleSessionSummary } from '$lib/types/console';

export class ConsoleSessionsNotSupportedError extends Error {
  readonly name = 'ConsoleSessionsNotSupportedError';
  constructor(message = 'Console session management API not supported by this device') {
    super(message);
  }
}

type ConsoleSessionListResponse = {
  sessions?: Array<Partial<ConsoleSessionSummary>>;
};

type CreateConsoleSessionRequest = {
  cols?: number;
  rows?: number;
};

const JSON_HEADERS = {
  Accept: 'application/json',
  'Content-Type': 'application/json'
};

function mapSession(raw: Partial<ConsoleSessionSummary> | null | undefined): ConsoleSessionSummary | null {
  const rawId = (raw as Record<string, unknown>)?.sessionId ?? (raw as Record<string, unknown>)?.session_id;
  const sessionId = typeof rawId === 'string' ? rawId : '';
  if (!raw || typeof raw !== 'object' || sessionId.length === 0) {
    return null;
  }
  const createdAt = (raw as Record<string, unknown>).createdAt ?? (raw as Record<string, unknown>).created_at ?? new Date().toISOString();
  const lastActivity =
    (raw as Record<string, unknown>).lastActivity ?? (raw as Record<string, unknown>).last_activity ?? createdAt ?? new Date().toISOString();
  const exitCode = (raw as Record<string, unknown>).exitCode ?? (raw as Record<string, unknown>).exit_code ?? null;
  const clientCount = (raw as Record<string, unknown>).clientCount ?? (raw as Record<string, unknown>).client_count ?? 0;
  const cols = (raw as Record<string, unknown>).cols;
  const rows = (raw as Record<string, unknown>).rows;
  const shell = (raw as Record<string, unknown>).shell;
  return {
    sessionId,
    shell: typeof shell === 'string' ? shell : 'shell',
    createdAt: typeof createdAt === 'string' ? createdAt : new Date().toISOString(),
    lastActivity: typeof lastActivity === 'string' ? lastActivity : new Date().toISOString(),
    exitCode: typeof exitCode === 'number' ? exitCode : null,
    clientCount: typeof clientCount === 'number' ? clientCount : 0,
    cols: typeof cols === 'number' ? cols : Number(cols) || 0,
    rows: typeof rows === 'number' ? rows : Number(rows) || 0,
    closed: Boolean((raw as Record<string, unknown>).closed ?? false)
  };
}

function unwrapJson<T>(response: Response): Promise<T> {
  if (!response.ok) {
    if (response.status === 404) {
      throw new ConsoleSessionsNotSupportedError();
    }
    throw new Error(`Request failed (${response.status})`);
  }
  return response.json() as Promise<T>;
}

export async function fetchConsoleSessions(): Promise<ConsoleSessionSummary[]> {
  const response = await apiFetchResponse('/console/sessions', {
    method: 'GET',
    headers: JSON_HEADERS
  });
  const payload = await unwrapJson<ConsoleSessionListResponse>(response).catch((error) => {
    throw new Error(`Unable to load console sessions: ${error instanceof Error ? error.message : String(error)}`);
  });
  const sessions = payload.sessions ?? [];
  return sessions.map(mapSession).filter((entry): entry is ConsoleSessionSummary => entry != null);
}

export async function createConsoleSession(request: CreateConsoleSessionRequest = {}): Promise<ConsoleSessionSummary> {
  const response = await apiFetchResponse('/console/sessions', {
    method: 'POST',
    headers: JSON_HEADERS,
    body: request
  });
  const payload = await unwrapJson<ConsoleSessionSummary>(response).catch((error) => {
    throw new Error(`Unable to create console session: ${error instanceof Error ? error.message : String(error)}`);
  });
  const mapped = mapSession(payload);
  if (!mapped) {
    throw new Error('Console session response missing session identifier');
  }
  return mapped;
}

export async function deleteConsoleSession(sessionId: string): Promise<void> {
  const response = await apiFetchResponse(`/console/sessions/${sessionId}`, {
    method: 'DELETE',
    headers: {
      Accept: 'application/json'
    }
  });
  if (response.status === 404) {
    throw new ConsoleSessionsNotSupportedError();
  }
  if (!response.ok) {
    const message = await response.text().catch(() => '');
    throw new Error(message || `Failed to close session (${response.status})`);
  }
}
