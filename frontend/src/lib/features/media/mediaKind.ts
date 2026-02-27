export type MediaAssetType = 'image' | 'video' | 'model' | 'archive' | 'firmware' | 'data' | 'unknown';

export const MEDIA_KIND_LABELS: Record<MediaAssetType, string> = {
  image: 'Image',
  video: 'Video',
  model: 'AI model',
  archive: 'Archive',
  firmware: 'Update image',
  data: 'Data file',
  unknown: 'Unknown file'
};

export const MEDIA_KIND_OPTIONS: MediaAssetType[] = [
  'image',
  'video',
  'model',
  'archive',
  'firmware',
  'data',
  'unknown'
];

const IMAGE_EXTENSIONS = new Set(['jpg', 'jpeg', 'png', 'bmp', 'gif', 'tif', 'tiff', 'webp', 'heic', 'avif']);
const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'm4v', 'mkv', 'avi', 'wmv', 'webm']);
const MODEL_EXTENSIONS = new Set(['tflite', 'onnx', 'pt', 'pth', 'pb', 'mlmodel', 'engine', 'rknn', 'safetensors']);
const ARCHIVE_EXTENSIONS = new Set(['zip', 'tar', 'gz', 'tgz', 'bz2', 'tbz', 'tbz2', 'xz', 'txz', '7z', 'rar', 'zst']);
const COMPRESSION_EXTENSIONS = new Set(['gz', 'bz2', 'tbz', 'tbz2', 'xz', 'txz', 'zst', 'zip', '7z', 'rar']);
const FIRMWARE_EXTENSIONS = new Set(['img', 'swu', 'fw', 'fwimg', 'bin', 'hex', 'dfu', 'upd', 'update', 'ota', 'squashfs']);
const DATA_EXTENSIONS = new Set(['fmap', 'fmaps', 'map', 'json', 'yaml', 'yml', 'csv', 'tsv', 'txt', 'npy', 'npz', 'xml', 'log']);

const ARCHIVE_MIME_TYPES = new Set([
  'application/zip',
  'application/x-zip-compressed',
  'application/x-7z-compressed',
  'application/x-rar-compressed',
  'application/gzip',
  'application/x-gzip',
  'application/x-tar',
  'application/x-bzip2',
  'application/x-xz',
  'application/zstd'
]);

const DATA_MIME_TYPES = new Set([
  'application/json',
  'application/x-yaml',
  'application/yaml',
  'text/plain',
  'text/csv',
  'text/tab-separated-values',
  'text/yaml',
  'text/markdown'
]);

export type MediaKindInput = {
  name?: string | null;
  contentType?: string | null;
  kindHint?: string | null;
  hasModelMetadata?: boolean;
};

function normalizeContentType(value?: string | null): string {
  return (value ?? '').trim().toLowerCase();
}

function normalizeKind(value?: string | null): MediaAssetType | null {
  const normalized = (value ?? '').trim().toLowerCase();
  if (!normalized) return null;
  if (normalized === 'image') return 'image';
  if (normalized === 'video') return 'video';
  if (normalized === 'model' || normalized === 'ai' || normalized === 'ai_model' || normalized === 'ai-model') return 'model';
  if (normalized === 'archive' || normalized === 'zip') return 'archive';
  if (
    normalized === 'firmware' ||
    normalized === 'update' ||
    normalized === 'update-image' ||
    normalized === 'update_image' ||
    normalized === 'ota' ||
    normalized === 'disk-image' ||
    normalized === 'disk_image'
  ) {
    return 'firmware';
  }
  if (normalized === 'data' || normalized === 'dataset' || normalized === 'field-map' || normalized === 'field_map') return 'data';
  if (normalized === 'unknown' || normalized === 'file' || normalized === 'other') return 'unknown';
  return null;
}

function getExtensionChain(name?: string | null): string[] {
  const trimmed = (name ?? '').trim();
  if (!trimmed) return [];
  const parts = trimmed.split('.').filter(Boolean);
  if (parts.length < 2) return [];
  return parts.slice(-3).map((part) => part.toLowerCase());
}

function classifyByName(name?: string | null): MediaAssetType {
  const chain = getExtensionChain(name);
  if (!chain.length) return 'unknown';
  const extension = chain[chain.length - 1] ?? '';
  const prev = chain.length >= 2 ? chain[chain.length - 2] : '';
  if (ARCHIVE_EXTENSIONS.has(extension)) {
    if (COMPRESSION_EXTENSIONS.has(extension) && prev) {
      if (prev === 'tar') return 'archive';
      if (FIRMWARE_EXTENSIONS.has(prev)) return 'firmware';
      if (DATA_EXTENSIONS.has(prev)) return 'data';
      if (MODEL_EXTENSIONS.has(prev)) return 'model';
      if (IMAGE_EXTENSIONS.has(prev)) return 'image';
      if (VIDEO_EXTENSIONS.has(prev)) return 'video';
    }
    return 'archive';
  }
  if (IMAGE_EXTENSIONS.has(extension)) return 'image';
  if (VIDEO_EXTENSIONS.has(extension)) return 'video';
  if (MODEL_EXTENSIONS.has(extension)) return 'model';
  if (FIRMWARE_EXTENSIONS.has(extension)) return 'firmware';
  if (DATA_EXTENSIONS.has(extension)) return 'data';
  return 'unknown';
}

export function classifyMediaKind(input: MediaKindInput): MediaAssetType {
  const hinted = normalizeKind(input.kindHint);
  if (hinted) return hinted;

  const contentType = normalizeContentType(input.contentType);
  if (contentType.startsWith('image/')) return 'image';
  if (contentType.startsWith('video/')) return 'video';

  if (input.hasModelMetadata) return 'model';

  const byName = classifyByName(input.name);
  if (ARCHIVE_MIME_TYPES.has(contentType)) {
    return byName !== 'unknown' && byName !== 'archive' ? byName : 'archive';
  }
  if (DATA_MIME_TYPES.has(contentType)) {
    return byName !== 'unknown' ? byName : 'data';
  }

  return byName;
}

export function mediaKindLabel(kind: MediaAssetType): string {
  return MEDIA_KIND_LABELS[kind] ?? 'Unknown file';
}
