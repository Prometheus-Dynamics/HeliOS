import { MEDIA_KIND_OPTIONS } from '$lib/features/media/mediaKind';
import type { MediaAsset } from '$lib/features/media/api';

export function normalizeMediaName(value: string): string {
  const trimmed = String(value ?? '').trim();
  if (!trimmed) return '';
  const parts = trimmed.split('/');
  return parts[parts.length - 1] ?? '';
}

export function selectedMediaNamesFromText(value: string, dedupeSet = new Set<string>()): string[] {
  dedupeSet.clear();
  for (const line of String(value ?? '').split('\n')) {
    const name = normalizeMediaName(line);
    if (name) dedupeSet.add(name);
  }
  return Array.from(dedupeSet);
}

export function toMediaPath(mediaRoot: string, name: string): string {
  return `${mediaRoot}/${name}`;
}

export function buildMediaPickerSelection(names: string[]): Record<string, boolean> {
  const next: Record<string, boolean> = {};
  names.forEach((name) => {
    next[name] = true;
  });
  return next;
}

export function toggleMediaPickerSelectionState(selected: Record<string, boolean>, name: string) {
  const key = normalizeMediaName(name);
  if (!key) return null;
  const next = { ...selected };
  if (next[key]) delete next[key];
  else next[key] = true;
  return { selected: next, anchorName: key };
}

export function singleMediaPickerSelection(name: string) {
  const key = normalizeMediaName(name);
  if (!key) return null;
  return { selected: { [key]: true }, anchorName: key };
}

export function addMediaPickerSelectionState(selected: Record<string, boolean>, name: string) {
  const key = normalizeMediaName(name);
  if (!key) return null;
  if (selected[key]) {
    return { selected, anchorName: key };
  }
  return { selected: { ...selected, [key]: true }, anchorName: key };
}

export function rangeSelectMediaPickerState({
  name,
  mode,
  anchorName,
  assets,
  selected
}: {
  name: string;
  mode: 'replace' | 'add';
  anchorName: string | null;
  assets: MediaAsset[];
  selected: Record<string, boolean>;
}) {
  const key = normalizeMediaName(name);
  if (!key) return null;
  const toIndex = assets.findIndex((asset) => normalizeMediaName(asset.name) === key);
  const anchorIndex = anchorName ? assets.findIndex((asset) => normalizeMediaName(asset.name) === anchorName) : -1;
  if (toIndex < 0 || anchorIndex < 0) {
    return mode === 'replace' ? singleMediaPickerSelection(key) : addMediaPickerSelectionState(selected, key);
  }
  const start = Math.min(toIndex, anchorIndex);
  const end = Math.max(toIndex, anchorIndex);
  const next = mode === 'replace' ? {} : { ...selected };
  for (let i = start; i <= end; i += 1) {
    const id = normalizeMediaName(assets[i]?.name ?? '');
    if (id) next[id] = true;
  }
  return { selected: next, anchorName: key };
}

export function applyMediaPickerSelectionText(selected: Record<string, boolean>, mediaRoot: string): string {
  return Object.keys(selected)
    .filter((key) => selected[key])
    .map((name) => toMediaPath(mediaRoot, name))
    .join('\n');
}

export function selectAllVisibleMedia(selected: Record<string, boolean>, assets: MediaAsset[]): Record<string, boolean> {
  const next = { ...selected };
  for (const asset of assets) {
    const key = normalizeMediaName(asset.name);
    if (key) next[key] = true;
  }
  return next;
}

export function normalizeMediaPickerKind(option: string): 'all' | MediaAsset['kind'] | null {
  if (option === 'all') return option;
  return MEDIA_KIND_OPTIONS.includes(option as MediaAsset['kind']) ? (option as MediaAsset['kind']) : null;
}

export function normalizeMediaPickerSort(raw: string): 'name' | 'recent' | null {
  return raw === 'name' || raw === 'recent' ? raw : null;
}
