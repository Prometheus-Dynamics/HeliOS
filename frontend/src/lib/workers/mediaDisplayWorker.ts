type MediaAssetPayload = {
  id: string;
  sizeBytes: number;
  updatedAt: string;
  width?: number | null;
  height?: number | null;
  fps?: number | null;
};

type MediaDisplayEntry = {
  sizeLabel: string;
  updatedLabel: string;
  dimensionLabel?: string;
  fpsLabel?: string;
};

const formatBytes = (value: number): string => {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const exponent = Math.min(units.length - 1, Math.floor(Math.log(value) / Math.log(1024)));
  const scaled = value / 1024 ** exponent;
  return `${scaled.toFixed(scaled >= 10 || exponent === 0 ? 0 : 1)} ${units[exponent]}`;
};

const formatUpdatedAt = (value: string): string => {
  const timestamp = Date.parse(value);
  if (Number.isNaN(timestamp)) return 'Unknown';
  return new Date(timestamp).toLocaleString();
};

const formatDimensions = (width?: number | null, height?: number | null): string | undefined => {
  if (!Number.isFinite(width) || !Number.isFinite(height) || !width || !height) return undefined;
  return `${width}×${height}`;
};

const formatFps = (fps?: number | null): string | undefined => {
  if (!Number.isFinite(fps) || !fps) return undefined;
  return `${fps.toFixed(2)} fps`;
};

self.onmessage = (event: MessageEvent<{ assets?: MediaAssetPayload[] }>) => {
  const assets = event.data?.assets ?? [];
  const display: Record<string, MediaDisplayEntry> = {};
  for (const asset of assets) {
    if (!asset || !asset.id) continue;
    display[asset.id] = {
      sizeLabel: formatBytes(asset.sizeBytes ?? 0),
      updatedLabel: formatUpdatedAt(asset.updatedAt ?? ''),
      dimensionLabel: formatDimensions(asset.width, asset.height),
      fpsLabel: formatFps(asset.fps)
    };
  }
  self.postMessage({ display });
};
