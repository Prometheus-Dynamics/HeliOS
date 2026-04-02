import {
  connectRealtimeUpdatesStream,
  normalizeRealtimeUpdateKind,
  realtimeUpdateMatchesKind,
  type RealtimeUpdateDomain,
  type RealtimeUpdateEvent,
  type RealtimeUpdateKind
} from '$lib/api/realtimeUpdates';
import { invalidateSWR, invalidateSWRPrefix } from '$lib/utils/swrCache';
import { createBackoffTimer } from '$lib/utils/backoff';

export type DomainUpdateKind = RealtimeUpdateKind | RealtimeUpdateDomain;

type DomainListener = {
  kinds: Set<DomainUpdateKind>;
  handler: (event: RealtimeUpdateEvent) => void;
  debounceMs: number;
  timer: ReturnType<typeof setTimeout> | null;
  lastEvent: RealtimeUpdateEvent | null;
};

const DOMAIN_INVALIDATION_RULES: Record<DomainUpdateKind, { prefixes: string[]; keys: string[] }> = {
  api: { prefixes: ['dashboard:'], keys: [] },
  device: { prefixes: ['dashboard:', 'devices:', 'settings:', 'systems:'], keys: [] },
  'device.hardware': { prefixes: ['dashboard:', 'devices:', 'settings:'], keys: ['media:stream-labels:v1'] },
  'device.imu': { prefixes: ['dashboard:', 'systems:', 'localization:'], keys: [] },
  'device.settings': { prefixes: ['dashboard:', 'devices:', 'settings:', 'systems:'], keys: [] },
  imu: { prefixes: ['dashboard:', 'systems:', 'localization:'], keys: [] },
  localization: { prefixes: ['dashboard:', 'devices:', 'localization:'], keys: [] },
  'localization.config': { prefixes: ['dashboard:', 'localization:'], keys: [] },
  'localization.maps': { prefixes: ['localization:'], keys: [] },
  'localization.profiles': { prefixes: ['dashboard:', 'localization:'], keys: [] },
  'localization.sources': { prefixes: ['dashboard:', 'devices:', 'localization:'], keys: [] },
  media: { prefixes: ['dashboard:', 'media:'], keys: ['media:stream-labels:v1'] },
  'media.assets': { prefixes: ['dashboard:', 'media:'], keys: ['media:stream-labels:v1'] },
  'media.imu': { prefixes: ['media:', 'localization:'], keys: [] },
  'media.labels': { prefixes: ['media:'], keys: [] },
  'media.metadata': { prefixes: ['media:'], keys: ['media:stream-labels:v1'] },
  pipelines: { prefixes: ['dashboard:', 'devices:', 'localization:', 'pipelines:'], keys: [] },
  'pipelines.graphs': { prefixes: ['dashboard:', 'devices:', 'localization:', 'pipelines:'], keys: [] },
  settings: { prefixes: ['dashboard:', 'devices:', 'settings:', 'systems:'], keys: [] },
  'settings.device': { prefixes: ['dashboard:', 'devices:', 'settings:', 'systems:'], keys: [] },
  'settings.plugins': { prefixes: ['dashboard:', 'settings:'], keys: [] },
  'settings.updater': { prefixes: ['dashboard:', 'settings:'], keys: [] },
  streams: { prefixes: ['dashboard:', 'devices:', 'localization:', 'media:'], keys: ['media:stream-labels:v1'] },
  'streams.controls': { prefixes: ['devices:', 'pipelines:'], keys: [] },
  'streams.lifecycle': { prefixes: ['dashboard:', 'devices:', 'localization:', 'media:'], keys: ['media:stream-labels:v1'] },
  'streams.pipeline': { prefixes: ['dashboard:', 'devices:', 'localization:', 'pipelines:'], keys: ['media:stream-labels:v1'] }
};

const DOMAIN_INVALIDATION_KEYS = Object.keys(DOMAIN_INVALIDATION_RULES) as DomainUpdateKind[];

const listeners = new Map<symbol, DomainListener>();
const reconnectBackoff = createBackoffTimer({ baseMs: 750, maxMs: 15_000 });
const latestResourceRevisions = new Map<string, number>();

let bridgeStarted = false;
let bridgeStop: (() => void) | null = null;

function hasBrowserRuntime(): boolean {
  return typeof window !== 'undefined' && typeof window.setTimeout === 'function';
}

function matchingInvalidationKinds(kind: string): DomainUpdateKind[] {
  const matches = DOMAIN_INVALIDATION_KEYS.filter((candidate) => realtimeUpdateMatchesKind(kind, candidate));
  if (!matches.length) return [];
  const maxSpecificity = Math.max(...matches.map((candidate) => candidate.split('.').length));
  return matches.filter((candidate) => candidate.split('.').length === maxSpecificity);
}

function applyDomainInvalidation(kind: string): void {
  for (const matchedKind of matchingInvalidationKinds(kind)) {
    const rule = DOMAIN_INVALIDATION_RULES[matchedKind];
    for (const prefix of rule.prefixes) {
      invalidateSWRPrefix(prefix);
    }
    for (const key of rule.keys) {
      invalidateSWR(key);
    }
  }
}

function notifyListeners(event: RealtimeUpdateEvent, kind: string): void {
  for (const listener of listeners.values()) {
    const shouldNotify = Array.from(listener.kinds).some((candidate) => realtimeUpdateMatchesKind(kind, candidate));
    if (!shouldNotify) continue;
    if (listener.debounceMs <= 0) {
      listener.handler(event);
      continue;
    }
    listener.lastEvent = event;
    if (listener.timer) continue;
    listener.timer = setTimeout(() => {
      const next = listener.lastEvent;
      listener.timer = null;
      listener.lastEvent = null;
      if (next) {
        listener.handler(next);
      }
    }, listener.debounceMs);
  }
}

function handleRealtimeUpdate(event: RealtimeUpdateEvent): void {
  const kind = normalizeRealtimeUpdateKind(event.kind);
  if (!kind) return;
  if (!shouldApplyRealtimeUpdate(event, kind)) return;
  applyDomainInvalidation(kind);
  notifyListeners(event, kind);
}

function shouldApplyRealtimeUpdate(event: RealtimeUpdateEvent, kind: string): boolean {
  const revision = Number(event.revision);
  if (!Number.isFinite(revision) || revision <= 0) {
    return true;
  }
  const resourceId = String(event.resource_id ?? '*').trim() || '*';
  const entity = String(event.entity ?? 'unknown').trim() || 'unknown';
  const key = `${kind}:${entity}:${resourceId}`;
  const previous = latestResourceRevisions.get(key) ?? 0;
  if (revision <= previous) {
    return false;
  }
  latestResourceRevisions.set(key, revision);
  return true;
}

function connectBridge(): void {
  if (!hasBrowserRuntime()) return;
  bridgeStop?.();
  bridgeStop = connectRealtimeUpdatesStream(
    {
      onOpen: () => {
        reconnectBackoff.reset();
      },
      onChange: (event) => {
        handleRealtimeUpdate(event);
      },
      onClose: () => {
        if (!bridgeStarted) return;
        const nextDelay = reconnectBackoff.bump();
        reconnectBackoff.schedule(() => {
          if (bridgeStarted) connectBridge();
        }, nextDelay);
      }
    },
    { heartbeatMs: 15_000 }
  );
}

export function startDomainInvalidationBridge(): void {
  if (!hasBrowserRuntime() || bridgeStarted) return;
  bridgeStarted = true;
  reconnectBackoff.reset();
  connectBridge();
}

export function subscribeDomainInvalidations(
  kinds: DomainUpdateKind[],
  handler: (event: RealtimeUpdateEvent) => void,
  options: { debounceMs?: number } = {}
): () => void {
  const token = Symbol('domain-invalidation-listener');
  listeners.set(token, {
    kinds: new Set(kinds),
    handler,
    debounceMs:
      typeof options.debounceMs === 'number' && Number.isFinite(options.debounceMs)
        ? Math.max(0, Math.floor(options.debounceMs))
        : 0,
    timer: null,
    lastEvent: null
  });
  startDomainInvalidationBridge();
  return () => {
    const listener = listeners.get(token);
    if (listener?.timer) {
      clearTimeout(listener.timer);
    }
    listeners.delete(token);
  };
}
