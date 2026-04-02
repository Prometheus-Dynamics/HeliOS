import { apiFetchCachedJson } from '$lib/api/core/http';
import { getHttpClientBase } from '$lib/api/httpClient';
import { buildWsUrlFromHttpBase, canUseWebSockets } from '$lib/api/core/ws';
import { cacheResourceData, type ResourceCacheContext, type ResourceCacheResult } from '$lib/api/resourceCache';
import type { LogSource, LogSourceKind, LogSourcesResponse, ReadModelFreshness, SystemdUnitStatus } from '$lib/ts-bindings/http/client';

export type { LogSource, LogSourceKind, LogSourcesResponse, ReadModelFreshness, SystemdUnitStatus };

export async function fetchLogSources(
  context: ResourceCacheContext<LogSourcesResponse> = {}
): Promise<LogSourcesResponse | ResourceCacheResult<LogSourcesResponse>> {
  const payload = await apiFetchCachedJson<LogSourcesResponse>('/device/logs/sources', context);
  if (payload.status === 'not_modified') {
    return payload as ResourceCacheResult<LogSourcesResponse>;
  }

  const nextPayload = payload.data;
  if (!nextPayload || typeof nextPayload !== 'object') {
    throw new Error('Log sources unavailable');
  }

  return cacheResourceData(
    {
      ...nextPayload,
      sources: Array.isArray(nextPayload.sources) ? nextPayload.sources : []
    },
    {
    etag: payload.etag ?? null,
    revision: payload.revision ?? null
    }
  );
}

export function buildLogsSocketUrl(sourceId: string, options: { lines?: number; follow?: boolean } = {}): string {
  const lines = options.lines ?? 200;
  const follow = options.follow ?? true;
  const baseUrl = buildWsUrlFromHttpBase(['v1', 'ws', 'logs']);
  try {
    const parsed = new URL(baseUrl);
    parsed.search = '';
    parsed.searchParams.set('source', sourceId);
    parsed.searchParams.set('lines', String(lines));
    parsed.searchParams.set('follow', follow ? 'true' : 'false');
    return parsed.toString();
  } catch {
    const params = new URLSearchParams({
      source: sourceId,
      lines: String(lines),
      follow: follow ? 'true' : 'false'
    });
    return `${baseUrl}?${params.toString()}`;
  }
}

export function buildLogsDownloadUrl(sourceId: string, options: { lines?: number } = {}): string {
  const base = getHttpClientBase();
  const lines = options.lines;

  try {
    const parsed = new URL(base);
    parsed.pathname = joinPath(parsed.pathname, 'v1', 'device', 'logs', 'download');
    parsed.search = '';
    parsed.searchParams.set('source', sourceId);
    if (typeof lines === 'number' && Number.isFinite(lines) && lines > 0) {
      parsed.searchParams.set('lines', String(Math.floor(lines)));
    }
    parsed.hash = '';
    return parsed.toString();
  } catch {
    const sanitized = base.replace(/\/+$/, '');
    const params = new URLSearchParams({ source: sourceId });
    if (typeof lines === 'number' && Number.isFinite(lines) && lines > 0) {
      params.set('lines', String(Math.floor(lines)));
    }
    return `${sanitized}/v1/device/logs/download?${params.toString()}`;
  }
}

function joinPath(basePath: string, ...segments: string[]): string {
  const parts = [basePath, ...segments]
    .filter((value) => value != null)
    .map((value) => String(value))
    .flatMap((value) => value.split('/'))
    .filter((value) => value.length > 0);
  return `/${parts.join('/')}`;
}

export { canUseWebSockets };
