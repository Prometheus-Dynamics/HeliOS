import { formatBytes } from '$lib/features/media/utils';
import type { MediaUploadProgress } from '$lib/features/media/api';

export type UploadToastContext = {
  totalFiles: number;
  currentFile?: {
    index: number;
    name?: string;
  };
};

export function createProgressSnapshot(loaded: number, total?: number | null): MediaUploadProgress {
  const normalizedTotal = total && total > 0 ? total : undefined;
  return {
    loaded,
    total: normalizedTotal,
    percent: normalizedTotal ? Math.max(0, Math.min(100, (loaded / normalizedTotal) * 100)) : null
  };
}

export function normalizeProgress(progress: MediaUploadProgress, fallbackTotal?: number): MediaUploadProgress {
  const candidateTotal = Number.isFinite(progress.total) && progress.total ? progress.total : fallbackTotal;
  if (candidateTotal && candidateTotal > 0) {
    const safeLoaded = Math.max(0, Math.min(candidateTotal, progress.loaded));
    return createProgressSnapshot(safeLoaded, candidateTotal);
  }
  if (progress.percent != null) {
    return {
      loaded: progress.loaded,
      total: progress.total,
      percent: Math.max(0, Math.min(100, progress.percent))
    };
  }
  return progress;
}

export function combineProgress(
  progress: MediaUploadProgress,
  baseLoaded: number,
  totalBytes?: number
): MediaUploadProgress {
  const total = totalBytes && totalBytes > 0 ? totalBytes : progress.total;
  const loaded = baseLoaded + progress.loaded;
  let percent: number | null = progress.percent ?? null;
  if (total && total > 0) {
    percent = Math.max(0, Math.min(100, (loaded / total) * 100));
  }
  return {
    loaded,
    total: total ?? undefined,
    percent
  };
}

export function describeUploadProgress(progress: MediaUploadProgress | null): string | null {
  if (progress?.percent != null && Number.isFinite(progress.percent)) {
    return `${Math.round(Math.min(100, Math.max(0, progress.percent)))}%`;
  }
  if (progress?.total) {
    return `${formatBytes(progress.loaded)} / ${formatBytes(progress.total ?? 0, { fallback: '0 B' })}`;
  }
  return null;
}

export function formatUploadLabel(context: UploadToastContext, progress: MediaUploadProgress | null): string {
  const progressText = describeUploadProgress(progress);
  if (context.currentFile) {
    const ordinal =
      context.totalFiles > 1
        ? ` (${Math.min(context.totalFiles, context.currentFile.index + 1)} of ${context.totalFiles})`
        : '';
    const baseLabel = context.currentFile.name ?? `File ${context.currentFile.index + 1}`;
    return progressText ? `${baseLabel}${ordinal} • ${progressText}` : `${baseLabel}${ordinal}`;
  }
  const safeCount = Math.max(0, context.totalFiles);
  const fileText = `${safeCount} file${safeCount === 1 ? '' : 's'}`;
  return progressText ? `${fileText} • ${progressText}` : fileText;
}
