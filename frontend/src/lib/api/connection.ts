import { browser } from '$app/environment';
import { writable } from 'svelte/store';
import { apiUrl } from '$lib/api/httpClient';
import { updateBackendFeaturesFromHealthPayload } from '$lib/api/backendFeatures';
import { fetchWithRetry } from '$lib/api/requestUtils';

export type ConnectionStatus = 'unknown' | 'online' | 'offline' | 'degraded';

export type ConnectionSnapshot = {
  status: ConnectionStatus;
  reason: string | null;
  lastChange: number;
  avgResponseMs: number | null;
};

type WritableConnectionStore = ReturnType<typeof writable<ConnectionSnapshot>>;

type ConnectionEvent = {
  timestamp: number;
  ok: boolean;
  durationMs?: number;
};

const EVENT_RETENTION_MS = 30_000;
const RECENT_FAILURE_DEGRADED_MS = 6_000;
const FAILURE_RATIO_THRESHOLD = 0.35;
const MIN_RATIO_SAMPLE_COUNT = 4;
const OFFLINE_FAILURE_STREAK = 3;
const OFFLINE_WITHOUT_SUCCESS_MS = 8_000;
const MAX_FAILURE_STREAK = 10;

function initialStatus(): ConnectionSnapshot {
  const status: ConnectionStatus = browser ? (navigator.onLine ? 'unknown' : 'offline') : 'unknown';
  return {
    status,
    reason: status === 'unknown' ? 'Checking backend' : null,
    lastChange: Date.now(),
    avgResponseMs: null
  };
}

class ConnectionMonitor {
  private readonly store: WritableConnectionStore;
  private readonly events: ConnectionEvent[] = [];
  private failureStreak = 0;
  private lastSuccessAt: number | null;
  private lastFailureReason: string | null = null;
  private hasEverSucceeded = false;
  private snapshot: ConnectionSnapshot;
  readonly subscribe: WritableConnectionStore['subscribe'];

  constructor(store: WritableConnectionStore, seed: ConnectionSnapshot) {
    this.store = store;
    this.snapshot = seed;
    this.subscribe = store.subscribe;
    this.lastSuccessAt = null;
    if (browser) {
      window.addEventListener('online', () => {
        // "Browser online" does not imply the backend is reachable; wait for the heartbeat probe.
        this.markUnknown('browser-online');
      });
      window.addEventListener('offline', () => {
        this.markOffline('browser-offline');
      });
    }
  }

  private pushEvent(ok: boolean, durationMs?: number): number {
    const now = Date.now();
    const normalizedDuration =
      typeof durationMs === 'number' && Number.isFinite(durationMs) ? Math.max(durationMs, 0) : undefined;
    this.events.push({
      timestamp: now,
      ok,
      durationMs: normalizedDuration
    });
    this.pruneOldEvents(now);
    if (ok) {
      this.lastSuccessAt = now;
      this.hasEverSucceeded = true;
    }
    return now;
  }

  private pruneOldEvents(now: number): void {
    const cutoff = now - EVENT_RETENTION_MS;
    while (this.events.length && this.events[0]?.timestamp < cutoff) {
      this.events.shift();
    }
  }

  private computeAverageResponse(): number | null {
    const samples = this.events.filter((event) => event.ok && typeof event.durationMs === 'number');
    if (!samples.length) {
      return null;
    }
    const total = samples.reduce((sum, event) => sum + (event.durationMs ?? 0), 0);
    return total / samples.length;
  }

  private computeWindowStats(): { success: number; failure: number; total: number; failureRatio: number } {
    let success = 0;
    let failure = 0;
    for (const event of this.events) {
      if (event.ok) {
        success += 1;
      } else {
        failure += 1;
      }
    }
    const total = success + failure;
    return {
      success,
      failure,
      total,
      failureRatio: total ? failure / total : 0
    };
  }

  private failureCountSince(cutoff: number): number {
    let count = 0;
    for (const event of this.events) {
      if (!event.ok && event.timestamp >= cutoff) {
        count += 1;
      }
    }
    return count;
  }

  private deriveStatus(now: number, reasonHint: string | null): { status: ConnectionStatus; reason: string | null } {
    if (!this.hasEverSucceeded) {
      if (browser && navigator.onLine === false) {
        return { status: 'offline', reason: reasonHint ?? 'Browser offline' };
      }
      if (this.failureStreak > 0) {
        return { status: 'offline', reason: reasonHint ?? this.lastFailureReason ?? 'Backend unreachable' };
      }
      return { status: 'unknown', reason: reasonHint ?? 'Checking backend' };
    }

    const offlineReason = this.resolveOfflineReason(now, reasonHint);
    if (offlineReason) {
      return { status: 'offline', reason: offlineReason };
    }

    const degradedReason = this.resolveDegradedReason(now, reasonHint);
    if (degradedReason) {
      return { status: 'degraded', reason: degradedReason };
    }

    return { status: 'online', reason: null };
  }

  private resolveOfflineReason(now: number, reasonHint: string | null): string | null {
    if (browser && navigator.onLine === false) {
      return reasonHint ?? 'Browser offline';
    }
    if (this.failureStreak >= OFFLINE_FAILURE_STREAK) {
      return reasonHint ?? this.lastFailureReason ?? 'Repeated backend failures';
    }
    if (this.lastSuccessAt && now - this.lastSuccessAt > OFFLINE_WITHOUT_SUCCESS_MS) {
      const seconds = Math.round((now - this.lastSuccessAt) / 1000);
      return `No backend response for ${seconds}s`;
    }
    return null;
  }

  private resolveDegradedReason(now: number, reasonHint: string | null): string | null {
    const reason = reasonHint ?? this.lastFailureReason ?? 'Recent backend communication errors';
    if (this.failureStreak >= 2) {
      return reason;
    }
    const recentFailures = this.failureCountSince(now - RECENT_FAILURE_DEGRADED_MS);
    if (recentFailures >= 2) {
      return reason;
    }
    const stats = this.computeWindowStats();
    if (stats.total >= MIN_RATIO_SAMPLE_COUNT && stats.failureRatio >= FAILURE_RATIO_THRESHOLD) {
      const percentage = Math.round(stats.failureRatio * 100);
      return `${reason} (${percentage}% failure rate)`;
    }
    return null;
  }

  private updateSnapshot(now: number, reasonHint: string | null, explicitStatus: ConnectionStatus | null): void {
    const avgResponseMs = this.computeAverageResponse();
    const next = explicitStatus
      ? {
          status: explicitStatus,
          reason:
            explicitStatus === 'online'
              ? reasonHint
              : explicitStatus === 'unknown'
                ? reasonHint ?? 'Checking backend'
                : reasonHint ?? this.lastFailureReason
        }
      : this.deriveStatus(now, reasonHint);
    const lastChange =
      next.status !== this.snapshot.status || next.reason !== this.snapshot.reason ? now : this.snapshot.lastChange;
    const snapshot: ConnectionSnapshot = {
      status: next.status,
      reason: next.reason ?? null,
      lastChange,
      avgResponseMs
    };
    this.snapshot = snapshot;
    if (snapshot.status === 'online') {
      this.lastFailureReason = null;
    }
    this.store.set(snapshot);
  }

  markUnknown(reason: string | null = null): void {
    const now = Date.now();
    // Keep failure streak as-is; this is just UI state while we wait for a probe.
    this.updateSnapshot(now, reason, 'unknown');
  }

  markOnline(reason: string | null = null): void {
    const now = this.pushEvent(true);
    this.failureStreak = 0;
    this.updateSnapshot(now, reason, 'online');
  }

  markDegraded(reason: string | null = null): void {
    const now = this.pushEvent(false);
    this.failureStreak = Math.min(this.failureStreak + 1, MAX_FAILURE_STREAK);
    if (reason) {
      this.lastFailureReason = reason;
    }
    this.updateSnapshot(now, reason ?? this.lastFailureReason, 'degraded');
  }

  markOffline(reason: string | null = null): void {
    const now = this.pushEvent(false);
    this.failureStreak = Math.min(this.failureStreak + 1, MAX_FAILURE_STREAK);
    if (reason) {
      this.lastFailureReason = reason;
    }
    this.updateSnapshot(now, reason ?? this.lastFailureReason ?? 'Connection lost', 'offline');
  }

  recordSuccess(durationMs?: number): void {
    const now = this.pushEvent(true, durationMs);
    this.failureStreak = 0;
    this.updateSnapshot(now, null, null);
  }

  recordFailure(reason: string, durationMs?: number): void {
    const now = this.pushEvent(false, durationMs);
    this.failureStreak = Math.min(this.failureStreak + 1, MAX_FAILURE_STREAK);
    this.lastFailureReason = reason;
    if (browser && navigator.onLine === false) {
      this.updateSnapshot(now, reason, 'offline');
      return;
    }
    this.updateSnapshot(now, reason, null);
  }
}

const initialSnapshot = initialStatus();
const writableStore = writable<ConnectionSnapshot>(initialSnapshot);

export const connectionMonitor = new ConnectionMonitor(writableStore, initialSnapshot);

export const connectionState = {
  subscribe: writableStore.subscribe
};

// Dedicated reachability probe. This drives the status indicator and avoids conflating
// application-level 4xx/5xx errors with "backend is down".
const HEARTBEAT_INTERVAL_MS = 2000;
const HEARTBEAT_HIDDEN_INTERVAL_MS = 10_000;
const HEARTBEAT_TIMEOUT_MS = 1200;
const HEARTBEAT_FAILURE_REASON = 'Backend heartbeat failed';

let heartbeatStarted = false;
let heartbeatTimer: ReturnType<typeof setTimeout> | null = null;
let heartbeatInFlight = false;

function heartbeatIntervalMs(): number {
  if (!browser) return HEARTBEAT_INTERVAL_MS;
  return document.visibilityState === 'hidden' ? HEARTBEAT_HIDDEN_INTERVAL_MS : HEARTBEAT_INTERVAL_MS;
}

async function runHeartbeatOnce(): Promise<void> {
  if (!browser) return;
  if (heartbeatInFlight) return;
  heartbeatInFlight = true;

  const startedAt = typeof performance !== 'undefined' ? performance.now() : Date.now();
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), HEARTBEAT_TIMEOUT_MS);
  try {
    const response = await fetchWithRetry(
      apiUrl('/health'),
      {
        method: 'GET',
        headers: { Accept: 'application/json' },
        cache: 'no-store',
        signal: controller.signal
      },
      { timeoutMs: HEARTBEAT_TIMEOUT_MS, maxAttempts: 1 }
    );
    const durationMs = Math.max(0, (typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt);

    // Any HTTP response means the backend is reachable.
    if (response.ok) {
      try {
        // Piggyback feature flags on the heartbeat to avoid a separate round-trip.
        const payload = await response.json();
        updateBackendFeaturesFromHealthPayload(payload);
      } catch {
        // Ignore parse errors; heartbeat reachability is what matters here.
      }
      connectionMonitor.recordSuccess(durationMs);
    } else {
      connectionMonitor.recordFailure(`Heartbeat bad status (${response.status})`, durationMs);
    }
  } catch (error) {
    const durationMs = Math.max(0, (typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt);
    if (error instanceof DOMException && error.name === 'AbortError') {
      connectionMonitor.recordFailure('Heartbeat timed out', durationMs);
    } else {
      connectionMonitor.recordFailure(HEARTBEAT_FAILURE_REASON, durationMs);
    }
  } finally {
    clearTimeout(timeoutId);
    heartbeatInFlight = false;
  }
}

function scheduleNextHeartbeat(delayMs: number): void {
  if (!browser) return;
  if (heartbeatTimer) clearTimeout(heartbeatTimer);
  heartbeatTimer = setTimeout(async () => {
    heartbeatTimer = null;
    await runHeartbeatOnce();
    scheduleNextHeartbeat(heartbeatIntervalMs());
  }, Math.max(0, delayMs));
}

export function startBackendHeartbeat(): void {
  if (!browser) return;
  if (heartbeatStarted) return;
  heartbeatStarted = true;

  // Kick immediately, then on an adaptive interval.
  scheduleNextHeartbeat(0);

  document.addEventListener('visibilitychange', () => {
    // Re-check quickly when the tab becomes visible.
    if (document.visibilityState === 'visible') {
      scheduleNextHeartbeat(0);
    }
  });
}

if (browser) {
  startBackendHeartbeat();
}
