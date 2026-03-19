export type StreamPreviewFormat = 'mjpeg' | 'h264' | 'h265' | 'unknown';

type CacheEntry = {
  value: StreamPreviewFormat | null;
  fetchedAt: number;
  inFlight: Promise<StreamPreviewFormat | null> | null;
};

const DEFAULT_PREVIEW_FORMAT_TTL_MS = 30_000;
const previewFormatCache = new Map<string, CacheEntry>();

export async function resolveStreamPreviewFormat(
  key: string,
  loader: () => Promise<StreamPreviewFormat | null>,
  options: { ttlMs?: number } = {}
): Promise<StreamPreviewFormat | null> {
  const ttlMs = normalizeTtl(options.ttlMs);
  const now = Date.now();
  const existing = previewFormatCache.get(key) ?? null;

  if (existing?.value && now - existing.fetchedAt < ttlMs) {
    return existing.value;
  }

  if (existing?.inFlight) {
    return existing.inFlight;
  }

  const pending = loader()
    .then((value) => {
      previewFormatCache.set(key, {
        value,
        fetchedAt: Date.now(),
        inFlight: null
      });
      return value;
    })
    .catch((error) => {
      if (existing?.value && now - existing.fetchedAt < ttlMs * 4) {
        previewFormatCache.set(key, {
          value: existing.value,
          fetchedAt: existing.fetchedAt,
          inFlight: null
        });
        return existing.value;
      }
      previewFormatCache.delete(key);
      throw error;
    });

  previewFormatCache.set(key, {
    value: existing?.value ?? null,
    fetchedAt: existing?.fetchedAt ?? 0,
    inFlight: pending
  });

  return pending;
}

export function invalidateStreamPreviewFormat(key?: string): void {
  if (typeof key === 'string' && key.trim().length > 0) {
    previewFormatCache.delete(key);
    return;
  }
  previewFormatCache.clear();
}

function normalizeTtl(ttlMs?: number): number {
  if (typeof ttlMs !== 'number' || !Number.isFinite(ttlMs)) {
    return DEFAULT_PREVIEW_FORMAT_TTL_MS;
  }
  return Math.max(1_000, Math.floor(ttlMs));
}
