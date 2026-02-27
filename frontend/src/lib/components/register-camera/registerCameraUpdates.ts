export function createDevicesUpdatesHandler(options: {
  connectDevicesUpdatesStream: (handlers: {
    onUpdate: () => void;
    onClose: () => void;
    onError: () => void;
  }) => () => void;
  onRefresh: () => void;
  refreshDebounceMs: number;
  reconnectMs: number;
}) {
  let wsRefreshTimer: number | null = null;
  let updatesCleanup: (() => void) | null = null;
  let updatesReconnectHandle: number | null = null;
  let updatesNonce = 0;

  function scheduleWsRefresh(): void {
    if (wsRefreshTimer != null) return;
    wsRefreshTimer = window.setTimeout(() => {
      wsRefreshTimer = null;
      if (document.hidden) return;
      options.onRefresh();
    }, options.refreshDebounceMs);
  }

  function scheduleDevicesUpdatesReconnect(): void {
    if (updatesReconnectHandle != null) return;
    updatesReconnectHandle = window.setTimeout(() => {
      updatesReconnectHandle = null;
      connectUpdates();
    }, options.reconnectMs);
  }

  function disconnectUpdates(): void {
    updatesNonce += 1;
    if (updatesReconnectHandle != null) {
      clearTimeout(updatesReconnectHandle);
      updatesReconnectHandle = null;
    }
    if (wsRefreshTimer != null) {
      clearTimeout(wsRefreshTimer);
      wsRefreshTimer = null;
    }
    updatesCleanup?.();
    updatesCleanup = null;
  }

  function connectUpdates(): void {
    const nonce = (updatesNonce += 1);
    if (updatesReconnectHandle != null) {
      clearTimeout(updatesReconnectHandle);
      updatesReconnectHandle = null;
    }
    updatesCleanup?.();
    updatesCleanup = null;
    updatesCleanup = options.connectDevicesUpdatesStream({
      onUpdate: () => {
        if (nonce !== updatesNonce) return;
        if (document.hidden) return;
        scheduleWsRefresh();
      },
      onClose: () => {
        if (nonce !== updatesNonce) return;
        scheduleDevicesUpdatesReconnect();
      },
      onError: () => {
        if (nonce !== updatesNonce) return;
        scheduleDevicesUpdatesReconnect();
      }
    });
  }

  return { connectUpdates, disconnectUpdates };
}
