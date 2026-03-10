import { browser } from '$app/environment';
import { connectWebSocketWithFallback, sendJson, type JsonSocketHandlers, type ManagedWebSocket } from '$lib/api/core/ws';

export type SharedJsonSocket = {
  ready: () => boolean;
  send: (payload: Record<string, unknown>) => boolean;
  close: () => void;
};

type SharedJsonSocketSubscriber = {
  token: symbol;
  handlers: JsonSocketHandlers;
};

type SharedJsonSocketEntry = {
  key: string;
  connection: ManagedWebSocket;
  subscribers: Map<symbol, SharedJsonSocketSubscriber>;
  connected: boolean;
};

const sharedSockets = new Map<string, SharedJsonSocketEntry>();

export function connectSharedJsonSocket(
  key: string,
  url: string,
  handlers: JsonSocketHandlers = {},
  options: { errorMessage?: string } = {}
): SharedJsonSocket | null {
  if (!browser) return null;

  const shared = getOrCreateSharedSocket(key, url, options);
  if (!shared) {
    handlers.onError?.(options.errorMessage ?? 'WebSocket connection failed');
    return null;
  }

  const subscriber: SharedJsonSocketSubscriber = {
    token: Symbol(key),
    handlers
  };
  shared.subscribers.set(subscriber.token, subscriber);

  if (shared.connected) {
    queueMicrotask(() => {
      if (shared.subscribers.has(subscriber.token)) {
        subscriber.handlers.onOpen?.();
      }
    });
  }

  return {
    ready: () => shared.connected && shared.connection.ready(),
    send: (payload) => sendJson(shared.connection, payload),
    close: () => {
      if (!shared.subscribers.delete(subscriber.token)) return;
      if (shared.subscribers.size > 0) return;
      closeSharedSocket(shared);
    }
  };
}

function getOrCreateSharedSocket(
  key: string,
  url: string,
  options: { errorMessage?: string }
): SharedJsonSocketEntry | null {
  const existing = sharedSockets.get(key);
  if (existing) {
    return existing;
  }

  const shared: SharedJsonSocketEntry = {
    key,
    connection: null as unknown as ManagedWebSocket,
    subscribers: new Map(),
    connected: false
  };

  const connection = connectWebSocketWithFallback(
    url,
    {
      onOpen: () => {
        shared.connected = true;
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onOpen?.();
        }
      },
      onError: (message) => {
        const errorMessage = message || options.errorMessage || 'WebSocket connection failed';
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onError?.(errorMessage);
        }
      },
      onClose: () => {
        shared.connected = false;
        if (sharedSockets.get(shared.key) === shared) {
          sharedSockets.delete(shared.key);
        }
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onClose?.();
        }
      }
    },
    options
  );

  if (!connection) {
    return null;
  }

  shared.connection = connection;
  sharedSockets.set(key, shared);
  return shared;
}

function closeSharedSocket(shared: SharedJsonSocketEntry): void {
  if (sharedSockets.get(shared.key) === shared) {
    sharedSockets.delete(shared.key);
  }
  shared.connected = false;
  shared.connection.close();
}
