import type { MediaAsset, MediaAssetType } from '$lib/features/media/api';

type MediaClientSort =
  | 'recent_desc'
  | 'recent_asc'
  | 'size_desc'
  | 'size_asc'
  | 'name_asc'
  | 'name_desc';

type Payload = {
  requestId: number;
  assets: MediaAsset[];
  search: string;
  streamFilter: string;
  kindFilter: MediaAssetType | 'all';
  sortMode: MediaClientSort;
};

type Response = {
  requestId: number;
  filtered: MediaAsset[];
};

function normalize(value: string): string {
  return value.trim().toLowerCase();
}

self.onmessage = (event: MessageEvent<Payload>) => {
  const { requestId, assets, search, streamFilter, kindFilter, sortMode } = event.data;
  const list = Array.isArray(assets) ? assets : [];
  const normalizedSearch = normalize(search ?? '');
  const filterStream = normalize(streamFilter ?? '');
  const filterKind = kindFilter ?? 'all';
  const filtered = list.filter((asset) => {
    if (filterKind !== 'all' && asset.kind !== filterKind) return false;
    if (filterStream && filterStream !== 'all') {
      const source = (asset.cameraSource ?? '').trim();
      if (normalize(source) !== filterStream) return false;
    }
    if (normalizedSearch) {
      if (!asset.name.toLowerCase().includes(normalizedSearch)) return false;
    }
    return true;
  });

  const timestamp = (asset: MediaAsset): number => {
    const updated = Date.parse(asset.updatedAt ?? '');
    if (Number.isFinite(updated)) return updated;
    const created = Date.parse(asset.createdAt ?? '');
    return Number.isFinite(created) ? created : 0;
  };

  const sorted = [...filtered];
  switch (sortMode ?? 'recent_desc') {
    case 'recent_asc':
      sorted.sort((a, b) => timestamp(a) - timestamp(b) || a.name.localeCompare(b.name));
      break;
    case 'size_desc':
      sorted.sort((a, b) => (b.sizeBytes ?? 0) - (a.sizeBytes ?? 0) || a.name.localeCompare(b.name));
      break;
    case 'size_asc':
      sorted.sort((a, b) => (a.sizeBytes ?? 0) - (b.sizeBytes ?? 0) || a.name.localeCompare(b.name));
      break;
    case 'name_asc':
      sorted.sort((a, b) => a.name.localeCompare(b.name));
      break;
    case 'name_desc':
      sorted.sort((a, b) => b.name.localeCompare(a.name));
      break;
    case 'recent_desc':
    default:
      sorted.sort((a, b) => timestamp(b) - timestamp(a) || a.name.localeCompare(b.name));
      break;
  }

  const response: Response = { requestId, filtered: sorted };
  self.postMessage(response);
};
