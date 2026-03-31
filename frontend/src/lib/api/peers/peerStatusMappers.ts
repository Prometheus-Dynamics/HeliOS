import type { PeerEndpoint } from '$lib/ts-bindings/http/client';
import type { PeerEndpointSummary, PeerStatus } from '$lib/types/peer';

export function normalizeStatus(value: unknown): PeerStatus {
  if (value === 'joining' || value === 'online' || value === 'offline' || value === 'unreachable') {
    return value;
  }
  if (typeof value === 'string') {
    const lowered = value.toLowerCase();
    if (lowered === 'joining' || lowered === 'online' || lowered === 'offline' || lowered === 'unreachable') {
      return lowered;
    }
  }
  return 'joining';
}

export function normalizeEndpoint(entry: unknown): PeerEndpointSummary | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as Partial<PeerEndpoint>;
  const host = typeof record.host === 'string' ? record.host.trim() : '';
  if (!host) {
    return null;
  }
  const port = typeof record.port === 'number' && Number.isFinite(record.port) ? record.port : undefined;
  return { host, port };
}

export function normalizeEndpoints(entries: unknown): PeerEndpointSummary[] {
  if (!Array.isArray(entries)) {
    return [];
  }
  return entries.map(normalizeEndpoint).filter((endpoint): endpoint is PeerEndpointSummary => Boolean(endpoint));
}

export function normalizeCapabilities(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.filter((item): item is string => typeof item === 'string' && item.trim().length > 0).map((item) => item.trim());
}
