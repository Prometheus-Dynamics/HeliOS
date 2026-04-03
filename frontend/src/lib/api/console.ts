import { apiFetchResponse } from '$lib/api/core/http';
import type {
  ConsoleSessionListPayload,
  ConsoleSessionSummaryPayload,
  CreateConsoleSessionRequest
} from '$lib/api/client';
import type { ConsoleSessionSummary } from '$lib/types/console';

export class ConsoleSessionsNotSupportedError extends Error {
  readonly name = 'ConsoleSessionsNotSupportedError';
  constructor(message = 'Console session management API not supported by this device') {
    super(message);
  }
}

const JSON_HEADERS = {
  Accept: 'application/json',
  'Content-Type': 'application/json'
};

function mapSession(raw: ConsoleSessionSummaryPayload | null | undefined): ConsoleSessionSummary | null {
  const sessionId = typeof raw?.session_id === 'string' ? raw.session_id : '';
  if (!raw || sessionId.length === 0) {
    return null;
  }
  return {
    sessionId,
    shell: typeof raw.shell === 'string' ? raw.shell : 'shell',
    createdAt: typeof raw.created_at === 'string' ? raw.created_at : new Date().toISOString(),
    lastActivity: typeof raw.last_activity === 'string' ? raw.last_activity : raw.created_at ?? new Date().toISOString(),
    exitCode: typeof raw.exit_code === 'number' ? raw.exit_code : null,
    clientCount: typeof raw.client_count === 'number' ? raw.client_count : 0,
    cols: typeof raw.cols === 'number' ? raw.cols : 0,
    rows: typeof raw.rows === 'number' ? raw.rows : 0,
    closed: Boolean(raw.closed ?? false)
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
  const payload = await unwrapJson<ConsoleSessionListPayload>(response).catch((error) => {
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
  const payload = await unwrapJson<ConsoleSessionSummaryPayload>(response).catch((error) => {
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
