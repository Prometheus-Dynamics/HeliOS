import { get, writable, type Readable } from 'svelte/store';
import { OpenAPI } from '$lib/ts-bindings/http/client';
import { apiFetchResponse } from '$lib/api/core/http';
import { createDomainResource } from '$lib/api/domainResources';
import { extractMessage } from '$lib/api/errors';
import { fetchLogSources } from '$lib/api/deviceLogs';
import { buildErrorMessage } from '$lib/ui/errorPolicy';
import { createBackoffTimer } from '$lib/utils/backoff';
import { extractStreamError, parseLogFrame, parseNdjson } from './logFormatting';
import {
  LOG_SOURCES_CACHE_KEY,
  LOG_SOURCES_CACHE_MAX_MS,
  LOG_SOURCES_CACHE_STALE_MS,
  normalizeApiSources,
  normalizeCachedSources,
  selectPreferredStream
} from './logSources';
import { mergeLogEntries, syncLogEntryIds, trimLogEntries } from './logSelection';
import { createLogWorkerClient } from './useLogWorker';
import type { LogEntry, LogFilter, LogStreamSummary } from './types';

export type LogsState = {
  logStreams: LogStreamSummary[];
  logStreamsLoading: boolean;
  logStreamsError: string | null;
  selectedLogStream: string | null;
  logsLoading: boolean;
  logsError: string | null;
  logEntries: LogEntry[];
  filteredLogs: LogEntry[];
  logFilter: LogFilter;
  tailing: boolean;
  logDownloadWindow: string;
  logDownloadStatus: string | null;
  logDownloadError: string | null;
  logDownloadBusy: boolean;
};

const MAX_LOG_ENTRIES = 400;
const DEFAULT_TAIL_SNAPSHOT_LINES = 200;
const LOG_RECONNECT_BASE_MS = 2_000;
const LOG_RECONNECT_MAX_MS = 30_000;
const logSourcesResource = createDomainResource({
  key: LOG_SOURCES_CACHE_KEY,
  loader: fetchLogSources,
  staleMs: LOG_SOURCES_CACHE_STALE_MS,
  maxAgeMs: LOG_SOURCES_CACHE_MAX_MS,
  kinds: ['device', 'settings']
});

export type LogsStore = {
  state: Readable<LogsState>;
  start: () => void;
  destroy: () => void;
  setFilter: (filter: LogFilter) => void;
  setSelectedStream: (streamId: string | null) => void;
  setDownloadWindow: (value: string) => void;
  downloadSelectedLog: () => Promise<void>;
  refreshStreams: () => Promise<void>;
};

export function createLogsStore(): LogsStore {
  const state = writable<LogsState>({
    logStreams: [],
    logStreamsLoading: false,
    logStreamsError: null,
    selectedLogStream: null,
    logsLoading: false,
    logsError: null,
    logEntries: [],
    filteredLogs: [],
    logFilter: 'all',
    tailing: false,
    logDownloadWindow: '15m',
    logDownloadStatus: null,
    logDownloadError: null,
    logDownloadBusy: false
  });

  const logWorker = createLogWorkerClient({
    transformFallback: ({ type, payload, streamId }) =>
      type === 'snapshot' ? parseNdjson(payload, streamId) : parseLogFrame(payload, streamId)
  });

  const logEntryIds = new Set<string>();
  let pendingLogEntries: LogEntry[] = [];
  let logFlushHandle: number | null = null;
  let logFlushMode: 'raf' | 'timeout' | null = null;
  let logEventSource: EventSource | null = null;
  let tailingStream: string | null = null;
  let stopInvalidations: (() => void) | null = null;
  const logReconnectTimer = createBackoffTimer({ baseMs: LOG_RECONNECT_BASE_MS, maxMs: LOG_RECONNECT_MAX_MS });
  let filterRequestId = 0;

  function updateState(next: Partial<LogsState>) {
    state.update((current) => ({ ...current, ...next }));
  }

  function resetLogEntries(entries: LogEntry[]): void {
    const trimmed = trimLogEntries(entries, MAX_LOG_ENTRIES);
    syncLogEntryIds(trimmed, logEntryIds);
    updateState({ logEntries: trimmed });
    scheduleFilter();
  }

  function scheduleFilter(): void {
    const requestId = ++filterRequestId;
    const snapshot = get(state);
    void logWorker.filter(snapshot.logEntries, snapshot.logFilter).then((entries) => {
      if (requestId !== filterRequestId) return;
      updateState({ filteredLogs: entries });
    });
  }

  function cancelLogFlush(): void {
    if (logFlushHandle == null) return;
    if (logFlushMode === 'raf' && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(logFlushHandle);
    } else {
      clearTimeout(logFlushHandle);
    }
    logFlushHandle = null;
    logFlushMode = null;
  }

  function clearPendingLogEntries(): void {
    pendingLogEntries = [];
    cancelLogFlush();
  }

  function flushPendingLogEntries(): void {
    if (!pendingLogEntries.length) {
      logFlushHandle = null;
      logFlushMode = null;
      return;
    }
    const current = get(state).logEntries.slice();
    const result = mergeLogEntries(current, pendingLogEntries, logEntryIds, MAX_LOG_ENTRIES);
    pendingLogEntries = [];
    logFlushHandle = null;
    logFlushMode = null;
    if (!result.changed) return;
    updateState({ logEntries: result.entries });
    scheduleFilter();
  }

  function scheduleLogFlush(): void {
    if (logFlushHandle != null) return;
    const run = () => flushPendingLogEntries();
    if (typeof requestAnimationFrame === 'function') {
      logFlushHandle = requestAnimationFrame(run);
      logFlushMode = 'raf';
    } else {
      logFlushHandle = setTimeout(run, 16) as unknown as number;
      logFlushMode = 'timeout';
    }
  }

  function appendLogEntries(entries: LogEntry[]): void {
    if (!entries.length) return;
    pendingLogEntries.push(...entries);
    scheduleLogFlush();
  }

  function resetLogReconnectBackoff(): void {
    logReconnectTimer.reset();
  }

  function bumpLogReconnectBackoff(): number {
    return logReconnectTimer.bump();
  }

  function scheduleLogReconnect(streamId: string, delay = logReconnectTimer.getDelay()): void {
    logReconnectTimer.schedule(() => {
      if (tailingStream !== streamId) return;
      void startLogTail(streamId);
    }, delay);
  }

  function stopLogTail(options?: { preserveEntries?: boolean; keepStream?: boolean }): void {
    const preserveEntries = options?.preserveEntries ?? false;
    const keepStream = options?.keepStream ?? false;
    if (logEventSource) {
      logEventSource.close();
      logEventSource = null;
    }
    logReconnectTimer.cancel();
    clearPendingLogEntries();
    updateState({ tailing: false, logsLoading: false });
    if (!keepStream) {
      tailingStream = null;
    }
    if (!preserveEntries) {
      resetLogEntries([]);
    } else {
      syncLogEntryIds(get(state).logEntries, logEntryIds);
    }
  }

  function ingestLogFrame(raw: string, fallbackStreamId: string): void {
    void logWorker.transformFrame(raw, fallbackStreamId).then((entries) => {
      if (entries.length) {
        appendLogEntries(entries);
      }
    });
  }

  async function fetchLogSnapshot(streamId: string, lineCount: number): Promise<LogEntry[]> {
    const params = new URLSearchParams();
    params.set('lines', lineCount.toString());
    const url = `${OpenAPI.BASE}/logs/${streamId}/tail?${params.toString()}`;
    const response = await apiFetchResponse(url);
    if (!response.ok) {
      const text = await response.text().catch(() => '');
      throw new Error(extractMessage(text) || `Tail failed (${response.status})`);
    }
    const text = await response.text();
    return logWorker.transformSnapshot(text, streamId);
  }

  async function startLogTail(streamId: string | null): Promise<void> {
    if (!streamId) return;
    stopLogTail({ preserveEntries: false });
    updateState({
      selectedLogStream: streamId,
      logsError: null,
      logDownloadStatus: null,
      logDownloadError: null,
      tailing: true,
      logsLoading: true
    });
    tailingStream = streamId;
    resetLogEntries([]);
    clearPendingLogEntries();

    if (typeof window === 'undefined' || typeof EventSource === 'undefined') {
      updateState({ logsError: 'EventSource is not supported in this environment.', tailing: false, logsLoading: false });
      return;
    }

    let since: string | null = null;

    try {
      const snapshot = await fetchLogSnapshot(streamId, Math.min(500, Math.max(10, Math.round(DEFAULT_TAIL_SNAPSHOT_LINES))));
      if (tailingStream !== streamId) return;
      resetLogEntries(snapshot);
      const lastEntry = snapshot[snapshot.length - 1];
      if (lastEntry?.timestamp) {
        since = lastEntry.timestamp;
      }
    } catch (err) {
      updateState({ logsError: buildErrorMessage({ error: err, fallback: 'Unable to load logs.' }) });
    } finally {
      updateState({ logsLoading: false });
    }

    const params = new URLSearchParams();
    if (since) {
      params.set('since', since);
    } else {
      params.set('lines', Math.min(500, Math.max(10, Math.round(DEFAULT_TAIL_SNAPSHOT_LINES))).toString());
    }
    const url = `${OpenAPI.BASE}/logs/${streamId}/events?${params.toString()}`;

    try {
      const source = new EventSource(url);
      logEventSource = source;

      source.onopen = () => {
        resetLogReconnectBackoff();
        updateState({ logsLoading: false });
      };

      source.addEventListener('log', (event: MessageEvent<string>) => {
        if (tailingStream !== streamId) return;
        updateState({ logsLoading: false, tailing: true });
        ingestLogFrame(event.data, streamId);
      });

      source.addEventListener('reset', (event: MessageEvent<string>) => {
        if (tailingStream !== streamId) return;
        const payload = event.data;
        if (!payload) {
          resetLogEntries([]);
          clearPendingLogEntries();
          return;
        }
        try {
          const frame = JSON.parse(payload) as { stream_id?: string | null };
          if (!frame.stream_id || frame.stream_id === streamId) {
            resetLogEntries([]);
            clearPendingLogEntries();
          }
        } catch {
          resetLogEntries([]);
          clearPendingLogEntries();
        }
      });

      source.addEventListener('log_error', (event: MessageEvent<string>) => {
        if (tailingStream !== streamId) return;
        updateState({ logsError: extractStreamError(event.data) });
        stopLogTail({ preserveEntries: true, keepStream: true });
        const nextDelay = bumpLogReconnectBackoff();
        scheduleLogReconnect(streamId, nextDelay);
      });

      source.onerror = () => {
        const current = get(state);
        if (!current.logsError) {
          updateState({ logsError: 'Log stream disconnected. Retry to resume.' });
        }
        stopLogTail({ preserveEntries: true, keepStream: true });
        const nextDelay = bumpLogReconnectBackoff();
        scheduleLogReconnect(streamId, nextDelay);
      };
    } catch (err) {
      updateState({ logsError: buildErrorMessage({ error: err, fallback: 'Unable to connect to the log stream.' }) });
      stopLogTail({ keepStream: true });
      const nextDelay = bumpLogReconnectBackoff();
      scheduleLogReconnect(streamId, nextDelay);
    }
  }

  async function loadLogStreams(): Promise<void> {
    updateState({ logStreamsLoading: true, logStreamsError: null });
    try {
      const cached = logSourcesResource.read();
      if (cached?.data?.length) {
        updateState({
          logStreams: normalizeCachedSources(cached.data)
        });
      }
      const sources = await logSourcesResource.refresh();
      const normalized = normalizeApiSources(sources ?? []);
      updateState({ logStreams: normalized });
      if (normalized.length === 0) {
        stopLogTail();
        updateState({ selectedLogStream: null });
        resetLogEntries([]);
        return;
      }
      const current = get(state).selectedLogStream;
      const preferred = selectPreferredStream(current, normalized);
      if (preferred) {
        updateState({ selectedLogStream: preferred });
        resetLogReconnectBackoff();
        void startLogTail(preferred);
      }
    } catch (err) {
      updateState({ logStreamsError: buildErrorMessage({ error: err, fallback: 'Unable to load log streams.' }) });
      stopLogTail();
      updateState({ logStreams: [] });
      resetLogEntries([]);
      logSourcesResource.invalidate();
    } finally {
      updateState({ logStreamsLoading: false });
    }
  }

  function setFilter(filter: LogFilter): void {
    updateState({ logFilter: filter });
    scheduleFilter();
  }

  function setSelectedStream(streamId: string | null): void {
    updateState({ selectedLogStream: streamId });
    resetLogReconnectBackoff();
    void startLogTail(streamId);
  }

  function setDownloadWindow(value: string): void {
    updateState({ logDownloadWindow: value });
  }

  async function downloadSelectedLog(): Promise<void> {
    const snapshot = get(state);
    if (!snapshot.selectedLogStream) return;
    updateState({ logDownloadBusy: true, logDownloadError: null, logDownloadStatus: null });
    try {
      const params = new URLSearchParams();
      const windowExpr = snapshot.logDownloadWindow.trim();
      if (windowExpr) {
        params.set('window', windowExpr);
      }
      const query = params.toString();
      const response = await apiFetchResponse(
        `${OpenAPI.BASE}/logs/${snapshot.selectedLogStream}/download${query ? `?${query}` : ''}`
      );
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(extractMessage(text) || `Download failed (${response.status})`);
      }
      const blob = await response.blob();
      const disposition = response.headers.get('content-disposition') ?? '';
      const match = disposition.match(/filename="(.+?)"/i);
      const filename = match?.[1] ?? `${snapshot.selectedLogStream}.tar.gz`;
      const blobUrl = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = blobUrl;
      anchor.download = filename;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(blobUrl);
      updateState({ logDownloadStatus: `Downloaded ${filename}` });
    } catch (err) {
      updateState({ logDownloadError: buildErrorMessage({ error: err, fallback: 'Unable to download logs.' }) });
    } finally {
      updateState({ logDownloadBusy: false });
    }
  }

  function start(): void {
    stopInvalidations?.();
    stopInvalidations = logSourcesResource.subscribeInvalidations(() => {
      void loadLogStreams();
    }, { debounceMs: 250 });
    void loadLogStreams();
  }

  function destroy(): void {
    stopInvalidations?.();
    stopInvalidations = null;
    stopLogTail();
    logWorker.destroy();
  }

  return {
    state,
    start,
    destroy,
    setFilter,
    setSelectedStream,
    setDownloadWindow,
    downloadSelectedLog,
    refreshStreams: loadLogStreams
  };
}
