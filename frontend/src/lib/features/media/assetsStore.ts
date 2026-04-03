import { derived, get, writable, type Readable } from 'svelte/store';
import { DeviceService, type CameraLayoutResponse } from '$lib/api/client';
import { createDomainResource } from '$lib/api/domainResources';
import { createMediaListStore, type MediaListStore } from './store';
import { createSelectionStore, type SelectionStore } from './selectionStore';
import { resolveStreamLabel } from '$lib/utils/streamLabels';

export type MediaStreamLabelsState = {
  labels: Record<string, string>;
  loading: boolean;
  error: string | null;
};

export type MediaAssetsStore = {
  list: MediaListStore;
  selection: SelectionStore;
  streamLabels: Readable<Record<string, string>>;
  streamLabelsState: Readable<MediaStreamLabelsState>;
  availableStreams: Readable<string[]>;
  refreshStreamLabels: () => Promise<void>;
  rememberStream: (id: string | null | undefined) => void;
  streamLabel: (id: string) => string;
  destroy: () => void;
};

const DEFAULT_STREAM_LABELS_CACHE_KEY = 'media:stream-labels:v1';
const DEFAULT_STREAM_LABELS_CACHE_STALE_MS = 30_000;
const DEFAULT_STREAM_LABELS_CACHE_MAX_MS = 120_000;

function defaultStreamLabelLoader(): Promise<Record<string, string>> {
  return DeviceService.getCameraLayout().then((response: CameraLayoutResponse) => {
    const next: Record<string, string> = {};
    const cams = Array.isArray(response.cameras) ? response.cameras : [];
    for (const cam of cams) {
      const id = typeof cam?.stream_id === 'string' ? cam.stream_id.trim() : '';
      if (!id) continue;
      const label = resolveStreamLabel(cam, id);
      next[id] = label;
    }
    return next;
  });
}

export function createMediaAssetsStore(options: {
  listOptions?: Parameters<typeof createMediaListStore>[0];
  streamLabels?: {
    cacheKey?: string;
    staleMs?: number;
    maxAgeMs?: number;
    loader?: () => Promise<Record<string, string>>;
  };
} = {}): MediaAssetsStore {
  const list = createMediaListStore(options.listOptions ?? {});
  const selection = createSelectionStore();

  const streamLabelsState = writable<MediaStreamLabelsState>({
    labels: {},
    loading: false,
    error: null
  });

  const extraStreams = writable<string[]>([]);

  const availableStreams = derived([list.state, extraStreams], ([$state, extras]) => {
    const unique = new Set<string>();
    const counts = $state.counts;
    if (counts?.byCameraSource) {
      Object.keys(counts.byCameraSource).forEach((source) => unique.add(source));
    }
    $state.assets.forEach((asset) => {
      const source = asset.cameraSource?.trim();
      if (source) unique.add(source);
    });
    extras.forEach((source) => {
      const trimmed = source.trim();
      if (trimmed) unique.add(trimmed);
    });
    return Array.from(unique);
  });

  const streamLabels = derived(streamLabelsState, ($state) => $state.labels);
  const streamLabelsResource = createDomainResource({
    key: options.streamLabels?.cacheKey ?? DEFAULT_STREAM_LABELS_CACHE_KEY,
    loader: options.streamLabels?.loader ?? defaultStreamLabelLoader,
    staleMs: options.streamLabels?.staleMs ?? DEFAULT_STREAM_LABELS_CACHE_STALE_MS,
    maxAgeMs: options.streamLabels?.maxAgeMs ?? DEFAULT_STREAM_LABELS_CACHE_MAX_MS,
    kinds: ['device', 'media', 'streams'],
    matches: (event) =>
      event.kind === 'device.hardware' ||
      event.kind === 'streams.lifecycle' ||
      event.kind === 'streams.pipeline' ||
      event.kind === 'media.metadata'
  });

  async function refreshStreamLabels(): Promise<void> {
    streamLabelsState.update((current) => ({ ...current, loading: true, error: null }));

    try {
      const cached = streamLabelsResource.read();
      if (cached?.data && Object.keys(cached.data).length) {
        streamLabelsState.update((current) => ({ ...current, labels: cached.data }));
      }
      const labels = await streamLabelsResource.refresh();
      streamLabelsState.update(() => ({ labels, loading: false, error: null }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load stream labels.';
      streamLabelsState.update((current) => ({ ...current, loading: false, error: message, labels: {} }));
      streamLabelsResource.invalidate();
    }
  }

  function rememberStream(id: string | null | undefined): void {
    const key = String(id ?? '').trim();
    if (!key) return;
    extraStreams.update((current) => (current.includes(key) ? current : [...current, key]));
  }

  function streamLabel(id: string): string {
    const key = id.trim();
    if (!key) return '';
    return get(streamLabelsState).labels[key] ?? key;
  }

  function destroy(): void {
    list.destroy();
  }

  return {
    list,
    selection,
    streamLabels,
    streamLabelsState,
    availableStreams,
    refreshStreamLabels,
    rememberStream,
    streamLabel,
    destroy
  };
}
