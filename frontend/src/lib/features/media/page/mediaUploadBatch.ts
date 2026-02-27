import type { MediaAssetType, MediaUploadProgress } from '$lib/features/media/api';
import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';

export type UploadToastContext = {
  totalFiles: number;
  currentFile?: {
    index: number;
    name?: string;
  };
};

export async function uploadFileBatch(
  files: File[],
  unsupported: File[],
  context: {
    setUploading: (value: boolean) => void;
    clearUploadState: () => void;
    createFileListFromArray: (files: File[]) => FileList;
    detectMediaKind: (file: File) => MediaAssetType | null;
    inferKindFromFile: (file: File) => MediaAssetType;
    fileDisplayName: (file: File) => string;
    sumFileSizes: (files: File[]) => number;
    createProgressSnapshot: (loaded: number, total?: number | null) => MediaUploadProgress;
    normalizeProgress: (progress: MediaUploadProgress, fallbackTotal?: number) => MediaUploadProgress;
    combineProgress: (progress: MediaUploadProgress, baseLoaded: number, totalBytes?: number) => MediaUploadProgress;
    uploadMediaAsset: (options: {
      files: FileList;
      kind: MediaAssetType;
      onProgress: (progress: MediaUploadProgress) => void;
    }) => Promise<unknown>;
    showUploadToast: (progress: MediaUploadProgress | null, toastContext: UploadToastContext) => void;
    resolveUploadToast: (success: boolean, description: string) => void;
    refreshAssets: (options: { resetPage?: boolean; includeCounts?: boolean }) => Promise<void>;
    toaster: {
      error: (payload: { title: string; description: string }) => void;
    };
  }
): Promise<void> {
  const errors = unsupported.map((file) => `${context.fileDisplayName(file)}: unsupported file type`);
  if (!files.length) {
    if (errors.length) {
      reportError({
        title: errors.length === 1 ? 'Upload failed' : 'Some uploads failed',
        description: errors.join('\n')
      });
    }
    return;
  }
  context.setUploading(true);
  context.clearUploadState();
  const totalFiles = files.length;
  const totalBytes = context.sumFileSizes(files);
  let uploadedCount = 0;
  let completedBytes = 0;
  const queueContext: UploadToastContext = { totalFiles };
  if (totalFiles > 0) {
    context.showUploadToast(context.createProgressSnapshot(0, totalBytes), queueContext);
  }
  try {
    for (let index = 0; index < totalFiles; index += 1) {
      const file = files[index];
      const fileLabel = context.fileDisplayName(file);
      const fileBytes = Number.isFinite(file.size) ? file.size : 0;
      const fileTotal = fileBytes > 0 ? fileBytes : undefined;
      const fileContext: UploadToastContext = {
        totalFiles,
        currentFile: { index, name: fileLabel }
      };
      const baseAggregate = context.combineProgress(
        context.createProgressSnapshot(0, fileTotal),
        completedBytes,
        totalBytes
      );
      context.showUploadToast(baseAggregate, fileContext);
      try {
        await context.uploadMediaAsset({
          files: context.createFileListFromArray([file]),
          kind: context.detectMediaKind(file) ?? context.inferKindFromFile(file),
          onProgress: (progress) => {
            const normalized = context.normalizeProgress(progress, fileTotal);
            const aggregate = context.combineProgress(normalized, completedBytes, totalBytes);
            context.showUploadToast(aggregate, fileContext);
          }
        });
        uploadedCount += 1;
      } catch (error) {
        errors.push(`${context.fileDisplayName(file)}: ${buildErrorMessage({ error, fallback: 'Upload failed.' })}`);
      } finally {
        completedBytes += fileBytes;
      }
    }
  } finally {
    context.setUploading(false);
  }
  if (uploadedCount) {
    context.resolveUploadToast(true, `Uploaded ${uploadedCount} file${uploadedCount === 1 ? '' : 's'}`);
    await context.refreshAssets({ resetPage: true, includeCounts: true });
  } else {
    context.resolveUploadToast(false, errors.length ? 'Upload failed' : 'No files uploaded');
  }
  if (errors.length) {
    context.toaster.error({
      title: `${errors.length} file${errors.length === 1 ? '' : 's'} failed`,
      description: errors.join('\n')
    });
  }
}
