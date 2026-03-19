import { browser } from '$app/environment';
import { getHttpClientBase } from '$lib/api/httpClient';

const WS_CONNECT_FAILURE_COOLDOWN_MS = 8_000;
const wsConnectFailureUntil = new Map<string, number>();

export type JsonSocket = {
  ready: () => boolean;
  send: (payload: Record<string, unknown>) => boolean;
  close: () => void;
};

export type JsonSocketHandlers = {
  onOpen?: () => void;
  onClose?: () => void;
  onError?: (error: string) => void;
};

export type WebSocketHandlers = {
  onOpen?: (event: Event) => void;
  onMessage?: (event: MessageEvent) => void;
  onClose?: (event: CloseEvent) => void;
  onError?: (error: string) => void;
};

export type ManagedWebSocket = {
  close: () => void;
  ready: () => boolean;
  socket: () => WebSocket | null;
  url: () => string | null;
};

export type WsUrlOptions = {
  base?: string;
  path: string | string[];
  allowRelative?: boolean;
  fallbackOrigin?: string;
};

export function canUseWebSockets(): boolean {
  return browser && typeof WebSocket !== 'undefined';
}

export function buildWsUrl(options: WsUrlOptions): string {
  const path = Array.isArray(options.path) ? options.path : [options.path];
  const pathString = joinPath('', ...path);
  const baseCandidate = (options.base ?? '').trim() || (options.fallbackOrigin ?? '').trim();

  if (baseCandidate) {
    try {
      const parsed = new URL(baseCandidate);
      parsed.protocol = parsed.protocol === 'https:' ? 'wss:' : 'ws:';
      parsed.pathname = joinPath(parsed.pathname, ...path);
      parsed.search = '';
      parsed.hash = '';
      return parsed.toString();
    } catch {
      // fall through to sanitized base handling
    }
  }

  if (!baseCandidate && options.allowRelative) {
    return pathString;
  }

  const sanitized = baseCandidate.replace(/^https?:\/\//, '').replace(/\/+$/, '');
  if (sanitized.length) {
    return `ws://${sanitized}${pathString}`;
  }

  return pathString;
}

export function buildWsUrlFromHttpBase(path: string | string[], options: Omit<WsUrlOptions, 'path' | 'base'> = {}): string {
  return buildWsUrl({
    base: getHttpClientBase(),
    path,
    allowRelative: options.allowRelative,
    fallbackOrigin: options.fallbackOrigin
  });
}

export function buildWsCandidateUrls(url: string): string[] {
  const candidates: string[] = [];
  const parsed = parseWsUrl(url);
  if (!parsed || !parsed.pathname.startsWith('/v1/ws')) {
    return uniqueUrls([url]);
  }

  candidates.push(url);

  if (parsed.port === '5800') {
    const directApi = new URL(parsed.toString());
    directApi.port = '5801';
    candidates.push(directApi.toString());
  }

  return uniqueUrls(candidates);
}

export function connectWebSocketWithFallback(
  url: string,
  handlers: WebSocketHandlers = {},
  options: { errorMessage?: string } = {}
): ManagedWebSocket | null {
  if (!canUseWebSockets()) return null;
  const candidates = buildWsCandidateUrls(url);
  const failureKey = buildWsFailureKey(candidates);
  if ((wsConnectFailureUntil.get(failureKey) ?? 0) > Date.now()) {
    handlers.onError?.(options.errorMessage ?? 'WebSocket connection failed');
    return null;
  }
  let activeSocket: WebSocket | null = null;
  let activeUrl: string | null = null;
  let closed = false;
  let opened = false;
  const markConnectFailure = () => {
    wsConnectFailureUntil.set(failureKey, Date.now() + WS_CONNECT_FAILURE_COOLDOWN_MS);
  };
  const clearConnectFailure = () => {
    wsConnectFailureUntil.delete(failureKey);
  };

  const openCandidate = (index: number): void => {
    if (closed) return;
    const candidate = candidates[index] ?? url;
    let socket: WebSocket;
    try {
      socket = new WebSocket(candidate);
    } catch (err) {
      if (index + 1 < candidates.length) {
        openCandidate(index + 1);
        return;
      }
      markConnectFailure();
      handlers.onError?.((err as Error)?.message ?? options.errorMessage ?? 'WebSocket connection failed');
      return;
    }

    activeSocket = socket;
    activeUrl = candidate;

    socket.onopen = (event) => {
      if (closed || activeSocket !== socket) return;
      clearConnectFailure();
      opened = true;
      handlers.onOpen?.(event);
    };

    socket.onmessage = (event) => {
      if (closed || activeSocket !== socket) return;
      handlers.onMessage?.(event);
    };

    socket.onerror = () => {
      if (closed || activeSocket !== socket) return;
      if (!opened && index + 1 < candidates.length) {
        try {
          socket.close();
        } catch {
          // ignore
        }
        openCandidate(index + 1);
        return;
      }
      if (!opened) {
        markConnectFailure();
      }
      handlers.onError?.(options.errorMessage ?? 'WebSocket connection failed');
    };

    socket.onclose = (event) => {
      if (closed || activeSocket !== socket) return;
      if (!opened && index + 1 < candidates.length) {
        openCandidate(index + 1);
        return;
      }
      activeSocket = null;
      activeUrl = null;
      handlers.onClose?.(event);
    };
  };

  openCandidate(0);

  return {
    close: () => {
      closed = true;
      const socket = activeSocket;
      activeSocket = null;
      activeUrl = null;
      if (!socket) return;
      if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
        socket.close();
      }
    },
    ready: () => {
      const socket = activeSocket;
      return socket?.readyState === WebSocket.OPEN;
    },
    socket: () => activeSocket,
    url: () => activeUrl
  };
}

export function connectJsonSocket(
  url: string,
  handlers: JsonSocketHandlers = {},
  options: { errorMessage?: string } = {}
): JsonSocket | null {
  let isReady = false;
  const connection = connectWebSocketWithFallback(
    url,
    {
      onOpen: () => {
        isReady = true;
        handlers.onOpen?.();
      },
      onClose: () => {
        isReady = false;
        handlers.onClose?.();
      },
      onError: (message) => {
        isReady = false;
        handlers.onError?.(message);
      }
    },
    options
  );

  if (!connection) {
    return null;
  }

  return {
    ready: () => isReady,
    send: (payload: Record<string, unknown>) => sendJson(connection, payload),
    close: () => {
      connection.close();
      isReady = false;
    }
  };
}

export function sendJson(connection: ManagedWebSocket | null, payload: Record<string, unknown>): boolean {
  const socket = connection?.socket();
  if (!socket || socket.readyState !== WebSocket.OPEN) return false;
  socket.send(JSON.stringify(payload));
  return true;
}

function parseWsUrl(url: string): URL | null {
  try {
    const parsed = new URL(url);
    if (parsed.protocol === 'http:') parsed.protocol = 'ws:';
    if (parsed.protocol === 'https:') parsed.protocol = 'wss:';
    return parsed;
  } catch {
    if (!browser) return null;
    try {
      const origin = globalThis.location?.origin?.trim();
      if (!origin) return null;
      const parsed = new URL(url, origin);
      if (parsed.protocol === 'http:') parsed.protocol = 'ws:';
      if (parsed.protocol === 'https:') parsed.protocol = 'wss:';
      return parsed;
    } catch {
      return null;
    }
  }
}

function uniqueUrls(values: string[]): string[] {
  const deduped = new Set<string>();
  values.forEach((value) => {
    const trimmed = String(value ?? '').trim();
    if (trimmed.length) deduped.add(trimmed);
  });
  return Array.from(deduped);
}

function buildWsFailureKey(candidates: string[]): string {
  const endpoints = candidates
    .map((candidate) => parseWsUrl(candidate))
    .filter((value): value is URL => value instanceof URL)
    .map((value) => `${value.protocol}//${value.host}`);
  if (!endpoints.length) return candidates.join('|');
  return Array.from(new Set(endpoints)).sort().join('|');
}

function joinPath(basePath: string, ...segments: string[]): string {
  const parts = [basePath, ...segments]
    .filter((value) => value != null)
    .map((value) => String(value))
    .flatMap((value) => value.split('/'))
    .filter((value) => value.length > 0);
  return `/${parts.join('/')}`;
}
