import { apiFetchCachedJson } from '$lib/api/core/http';
import { getHttpClientBase } from '$lib/api/httpClient';
import { buildWsUrlFromHttpBase, canUseWebSockets } from '$lib/api/core/ws';
import { cacheResourceData, type ResourceCacheContext, type ResourceCacheResult } from '$lib/api/resourceCache';

export type LogSourceKind = 'journal_system' | 'journal_unit' | 'dmesg' | 'file';

export type SystemdUnitStatus = {
  active_state?: string | null;
  sub_state?: string | null;
  unit_file_state?: string | null;
  description?: string | null;
  fragment_path?: string | null;
  main_pid?: number | null;
  exec_main_status?: number | null;
};

export type LogSource = {
  id: string;
  label: string;
  group: string;
  kind: LogSourceKind;
  unit?: string | null;
  path?: string | null;
  important: boolean;
  status?: SystemdUnitStatus | null;
};

export async function fetchLogSources(context: ResourceCacheContext<LogSource[]> = {}): Promise<LogSource[] | ResourceCacheResult<LogSource[]>> {
  const payload = await apiFetchCachedJson<unknown>('/device/logs/sources', context);
  if (payload.status === 'not_modified') {
    return payload as ResourceCacheResult<LogSource[]>;
  }
  const sources = Array.isArray(payload.data) ? (payload.data as LogSource[]) : [];
  return cacheResourceData(sources, {
    etag: payload.etag ?? null,
    revision: payload.revision ?? null
  });
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
