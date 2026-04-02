import type { StreamInfo } from '$lib/ts-bindings/http/client';

export type StreamPreviewFormat = 'mjpeg' | 'h264' | 'h265' | 'unknown';
const previewFormatCache = new Map<string, Promise<StreamPreviewFormat | null>>();

export function normalizeStreamPreviewFormat(raw: unknown): StreamPreviewFormat {
  switch (String(raw ?? '').trim().toLowerCase()) {
    case 'mjpeg':
      return 'mjpeg';
    case 'h264':
      return 'h264';
    case 'h265':
      return 'h265';
    default:
      return 'unknown';
  }
}

export function streamPreviewFormatFromStreamInfo(stream: StreamInfo | null | undefined): StreamPreviewFormat {
  return normalizeStreamPreviewFormat(stream?.preview_format);
}

export function streamPreviewFormatFromPeerSummary(stream: { previewFormat?: unknown } | null | undefined): StreamPreviewFormat {
  return normalizeStreamPreviewFormat(stream?.previewFormat);
}

export async function resolveStreamPreviewFormat(
  key: string,
  loader: () => Promise<StreamPreviewFormat | null>
): Promise<StreamPreviewFormat | null> {
  const normalizedKey = key.trim();
  if (!normalizedKey) {
    return loader();
  }
  const cached = previewFormatCache.get(normalizedKey);
  if (cached) {
    return cached;
  }
  const task = loader()
    .then((value) => {
      if (value == null) {
        previewFormatCache.delete(normalizedKey);
        return null;
      }
      return normalizeStreamPreviewFormat(value);
    })
    .catch((error) => {
      previewFormatCache.delete(normalizedKey);
      throw error;
    });
  previewFormatCache.set(normalizedKey, task);
  return task;
}

export function invalidateStreamPreviewFormat(key?: string): void {
  if (typeof key === 'string' && key.trim()) {
    previewFormatCache.delete(key.trim());
    return;
  }
  previewFormatCache.clear();
}
