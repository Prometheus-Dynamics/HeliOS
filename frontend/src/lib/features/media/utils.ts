import type { MediaAsset } from './api';

export function formatBytes(
  value: number | null | undefined,
  options: { fallback?: string; units?: string[]; base?: number } = {}
): string {
  const fallback = options.fallback ?? '--';
  if (!Number.isFinite(value)) return fallback;
  const units = options.units ?? ['B', 'KB', 'MB', 'GB', 'TB'];
  const base = options.base ?? 1024;
  let size = Math.max(0, value);
  let idx = 0;
  while (size >= base && idx < units.length - 1) {
    size /= base;
    idx += 1;
  }
  const precision = size >= 100 || idx === 0 ? 0 : size >= 10 ? 1 : 2;
  return `${size.toFixed(precision)} ${units[idx]}`;
}

export function appendCacheBuster(url: string | undefined, updatedAt?: string): string {
  if (!url) return '';
  const timestamp = updatedAt ? Date.parse(updatedAt) : Date.now();
  const cacheKey = Number.isFinite(timestamp) && timestamp > 0 ? timestamp.toString() : `${Date.now()}`;
  const separator = url.includes('?') ? '&' : '?';
  return `${url}${separator}cb=${cacheKey}`;
}

export function assetPreviewSource(asset: MediaAsset): string {
  return appendCacheBuster(asset.previewUrl ?? asset.downloadUrl, asset.updatedAt);
}

export function assetPlaybackSource(asset: MediaAsset): string {
  return appendCacheBuster(asset.playbackUrl ?? asset.previewUrl ?? asset.downloadUrl, asset.updatedAt);
}

export function assetOriginalSource(asset: MediaAsset): string {
  return appendCacheBuster(asset.downloadUrl, asset.updatedAt);
}

export function assetMp4DownloadSource(asset: MediaAsset): string {
  return appendCacheBuster(asset.mp4DownloadUrl, asset.updatedAt);
}

export function isLikelyH265Codec(codec?: string | null): boolean {
  const normalized = (codec ?? '').trim().toLowerCase();
  return normalized === 'h265' || normalized === 'hevc' || normalized === 'hvc1' || normalized === 'hev1';
}

export function splitNameAndExtension(name: string): { base: string; extension: string } {
  const lastDot = name.lastIndexOf('.');
  if (lastDot <= 0 || lastDot === name.length - 1) {
    return { base: name, extension: '' };
  }
  return { base: name.slice(0, lastDot), extension: name.slice(lastDot) };
}

export function getAssetExtension(asset: MediaAsset): string {
  const primary = asset.primaryExtension?.trim();
  if (primary && primary.length) {
    return primary.startsWith('.') ? primary : `.${primary}`;
  }
  const { extension } = splitNameAndExtension(asset.name);
  return extension;
}

export function composeAssetName(asset: MediaAsset, baseName: string): string | null {
  const trimmedBase = baseName.trim();
  if (!trimmedBase.length) {
    return null;
  }
  const extension = getAssetExtension(asset);
  return extension ? `${trimmedBase}${extension}` : trimmedBase;
}

export function parseTensorSpec(spec?: string | null): { inputs: string[]; outputs: string[] } {
  const result = { inputs: [] as string[], outputs: [] as string[] };
  if (!spec) return result;
  const sections = spec.split('|').map((section) => section.trim()).filter(Boolean);
  for (const section of sections) {
    const colonIndex = section.indexOf(':');
    if (colonIndex === -1) continue;
    const label = section.slice(0, colonIndex).trim().toLowerCase();
    const content = section.slice(colonIndex + 1).trim();
    if (!content.length) continue;
    const entries = content.split(',').map((entry) => entry.trim()).filter(Boolean);
    if (!entries.length) continue;
    if (label.startsWith('input')) {
      result.inputs.push(...entries);
    } else if (label.startsWith('output')) {
      result.outputs.push(...entries);
    }
  }
  return result;
}

export function parseTagList(value: string): string[] {
  return value
    .split(',')
    .map((tag) => tag.trim())
    .filter(Boolean);
}

export function formatVideoCodec(codec?: string | null): string {
  const normalized = (codec ?? '').trim().toLowerCase();
  if (!normalized) return 'Unknown';
  if (normalized === 'h264' || normalized === 'avc' || normalized === 'avc1' || normalized === 'avc3') return 'H.264 (AVC)';
  if (normalized === 'h265' || normalized === 'hevc' || normalized === 'hvc1' || normalized === 'hev1') return 'H.265 (HEVC)';
  if (normalized === 'mjpeg' || normalized === 'mjpg') return 'MJPEG';
  return normalized.toUpperCase();
}

export type ModelAffinity = {
  runtime?: 'CPU' | 'TPU';
  precision?: 'INT8' | 'UINT8' | 'F16' | 'F32';
  quantized?: boolean;
};

export function deriveModelAffinity(tags?: string[] | null): ModelAffinity {
  const normalized = (tags ?? []).map((tag) => tag.trim()).filter(Boolean);
  if (!normalized.length) return {};
  let runtime: ModelAffinity['runtime'];
  let precision: ModelAffinity['precision'];
  let quantized = false;
  for (const tag of normalized) {
    const lower = tag.toLowerCase();
    if (!runtime) {
      if (lower === 'runtime:coral' || lower === 'edge-tpu' || lower === 'edgetpu' || lower === 'requires-edge-tpu') {
        runtime = 'TPU';
      } else if (lower === 'runtime:cpu') {
        runtime = 'CPU';
      }
    }
    if (!precision) {
      if (lower === 'precision:int8' || lower === 'int8') precision = 'INT8';
      else if (lower === 'precision:uint8' || lower === 'uint8') precision = 'UINT8';
      else if (lower === 'precision:f16' || lower === 'float16' || lower === 'f16') precision = 'F16';
      else if (lower === 'precision:f32' || lower === 'float32' || lower === 'f32') precision = 'F32';
    }
    if (!quantized && lower === 'quantized') {
      quantized = true;
    }
  }
  return { runtime, precision, quantized: quantized || undefined };
}
