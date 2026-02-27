import type { MediaAsset } from './api';
import { assetOriginalSource } from './utils';

export type TensorEntryDescription = { name: string; shape: string };

export function describeTensorEntry(entry: string): TensorEntryDescription {
  const parts = entry.split(':');
  if (parts.length < 2) {
    return { name: entry.trim(), shape: '' };
  }
  const shape = parts.pop()?.trim() ?? '';
  const name = parts.join(':').trim();
  return { name, shape };
}

export function loadImageDimensions(src: string): Promise<{ width: number; height: number } | null> {
  if (typeof window === 'undefined') return Promise.resolve(null);
  return new Promise((resolve) => {
    const img = new Image();
    const cleanup = () => {
      img.onload = null;
      img.onerror = null;
    };
    img.onload = () => {
      cleanup();
      resolve({ width: img.naturalWidth, height: img.naturalHeight });
    };
    img.onerror = () => {
      cleanup();
      resolve(null);
    };
    img.src = src;
  });
}

export function loadVideoDimensions(src: string): Promise<{ width: number; height: number } | null> {
  if (typeof window === 'undefined') return Promise.resolve(null);
  return new Promise((resolve) => {
    const video = document.createElement('video');
    const cleanup = () => {
      video.onloadedmetadata = null;
      video.onerror = null;
      video.src = '';
    };
    video.preload = 'metadata';
    video.muted = true;
    video.playsInline = true;
    video.onloadedmetadata = () => {
      const width = video.videoWidth;
      const height = video.videoHeight;
      cleanup();
      if (width && height) resolve({ width, height });
      else resolve(null);
    };
    video.onerror = () => {
      cleanup();
      resolve(null);
    };
    video.src = src;
  });
}

export async function loadAssetDimensions(asset: MediaAsset, sourceUrl?: string): Promise<{ width: number; height: number } | null> {
  if (!asset) return null;
  const src = sourceUrl ?? assetOriginalSource(asset);
  if (!src) return null;
  if (asset.kind === 'image') return loadImageDimensions(src);
  if (asset.kind === 'video') return loadVideoDimensions(src);
  return null;
}

export async function hydrateAssetDimensions(
  asset: MediaAsset,
  options: { sourceUrl?: string } = {}
): Promise<MediaAsset | null> {
  if (!asset || (asset.width > 0 && asset.height > 0)) return null;
  const resolved = await loadAssetDimensions(asset, options.sourceUrl);
  if (!resolved) return null;
  return { ...asset, width: resolved.width, height: resolved.height };
}
