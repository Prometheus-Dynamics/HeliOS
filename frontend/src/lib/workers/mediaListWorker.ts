import { classifyMediaKind, type MediaAssetType } from '$lib/features/media/mediaKind';
import { compareMediaName, compareMediaRecent, mediaTimestampIso } from '$lib/features/media/sort';

type MediaItem = {
  name: string;
  size_bytes: number;
  content_type: string;
  description?: string | null;
  tags?: string[] | null;
  stream_id?: string | null;
  kind?: string | null;
  captured_at_ms?: number | null;
  width?: number | null;
  height?: number | null;
  fps?: number | null;
  video_codec?: string | null;
  label_attached?: boolean | null;
  label_file_name?: string | null;
  model_id?: string | null;
  model_input_resolution?: string | null;
  model_tensor_spec?: string | null;
  imu_data_file_name?: string | null;
  imu_data_samples?: number | null;
};

type MediaAsset = {
  id: string;
  kind: MediaAssetType;
  name: string;
  description?: string;
  tags: string[];
  createdAt: string;
  updatedAt: string;
  fileCount: number;
  sizeBytes: number;
  previewUrl?: string;
  playbackUrl?: string;
  mp4DownloadUrl?: string;
  downloadUrl: string;
  labelAttached: boolean;
  videoClip?: { start_ms: number; end_ms?: number | null } | null;
  imageCrop?: { x: number; y: number; width: number; height: number } | null;
  rotation: number;
  loopMode: boolean;
  width: number;
  height: number;
  fps: number;
  videoCodec?: string;
  primaryMime?: string;
  primaryExtension?: string;
  labelFileName?: string;
  modelInputResolution?: string;
  modelTensorSpec?: string;
  cameraSource?: string;
  imuDataFileName?: string;
  imuDataSamples?: number;
  imuDataUrl?: string;
  frameTimestampsUrl?: string;
};

type MediaListWorkerPayload = {
  requestId: number;
  allItems: MediaItem[];
  visibleItems: MediaItem[];
  options: {
    kind?: MediaAssetType;
    search?: string;
    sort?: 'recent' | 'name';
    includeCounts: boolean;
    wantsServerFilter: boolean;
    baseUrl: string;
  };
};

type MediaListWorkerResponse = {
  requestId: number;
  assets: MediaAsset[];
  counts?: { byKind: Record<string, number>; byCameraSource: Record<string, number> };
};

function normalizeBaseUrl(raw: string): string {
  const base = raw || '';
  return base.endsWith('/') ? base.slice(0, -1) : base;
}

function mapAssetFromItem(item: MediaItem, baseUrl: string): MediaAsset {
  const resolvedCreatedAt = mediaTimestampIso({
    name: item.name,
    captured_at_ms: item.captured_at_ms
  });
  const kind = classifyMediaKind({
    name: item.name,
    contentType: item.content_type,
    kindHint: item.kind,
    hasModelMetadata: Boolean(item.model_input_resolution || item.model_tensor_spec || item.label_attached || item.model_id)
  });
  const encoded = encodeURIComponent(item.name);
  const downloadUrl = `${baseUrl}/media/${encoded}`;
  const normalizedExtension = item.name.split('.').pop()?.trim().toLowerCase() ?? '';
  const normalizedCodec = (item.video_codec ?? '').trim().toLowerCase();
  const isRawBitstream = normalizedExtension === 'h264' || normalizedExtension === 'avc' || normalizedExtension === 'h265' || normalizedExtension === 'hevc';
  const needsTranscodedPlayback =
    kind === 'video' &&
    (isRawBitstream || normalizedCodec === 'h265' || normalizedCodec === 'hevc' || normalizedCodec === 'hvc1' || normalizedCodec === 'hev1');
  const previewUrl = kind === 'video' ? `${downloadUrl}/thumbnail` : downloadUrl;
  const transcodedUrl = `${downloadUrl}/preview`;
  const playbackUrl = kind === 'video' && needsTranscodedPlayback ? transcodedUrl : downloadUrl;
  return {
    id: item.name,
    kind,
    name: item.name,
    description: item.description ?? undefined,
    tags: Array.isArray(item.tags) ? item.tags : [],
    createdAt: resolvedCreatedAt,
    updatedAt: resolvedCreatedAt,
    fileCount: 1,
    sizeBytes: item.size_bytes,
    previewUrl: kind === 'image' || kind === 'video' ? previewUrl : undefined,
    playbackUrl: kind === 'video' ? playbackUrl : undefined,
    mp4DownloadUrl: kind === 'video' && isRawBitstream ? transcodedUrl : undefined,
    downloadUrl,
    labelAttached: Boolean(item.label_attached),
    videoClip: null,
    imageCrop: null,
    rotation: 0,
    loopMode: false,
    width: typeof item.width === 'number' ? item.width : 0,
    height: typeof item.height === 'number' ? item.height : 0,
    fps: typeof item.fps === 'number' ? item.fps : 0,
    videoCodec: item.video_codec ?? undefined,
    primaryMime: item.content_type,
    primaryExtension: item.name.split('.').pop(),
    labelFileName: item.label_file_name ?? undefined,
    modelInputResolution: item.model_input_resolution ?? undefined,
    modelTensorSpec: item.model_tensor_spec ?? undefined,
    cameraSource: item.stream_id ?? undefined,
    imuDataFileName: item.imu_data_file_name ?? undefined,
    imuDataSamples: typeof item.imu_data_samples === 'number' ? item.imu_data_samples : undefined,
    imuDataUrl: item.imu_data_file_name ? `${downloadUrl}/imu` : undefined,
    frameTimestampsUrl: kind === 'video' ? `${downloadUrl}.frame_ts.txt` : undefined
  };
}

self.onmessage = (event: MessageEvent<MediaListWorkerPayload>) => {
  const { requestId, allItems, visibleItems, options } = event.data;
  const baseUrl = normalizeBaseUrl(options?.baseUrl ?? '');
  const kind = options?.kind;
  const search = options?.search?.trim().toLowerCase();
  const wantsServerFilter = Boolean(options?.wantsServerFilter);
  const includeCounts = Boolean(options?.includeCounts);
  const sourceItems = Array.isArray(wantsServerFilter ? visibleItems : allItems) ? (wantsServerFilter ? visibleItems : allItems) : [];
  const allSourceItems = Array.isArray(allItems) ? allItems : [];

  const applyClientFilters = (items: MediaItem[]): MediaAsset[] => {
    let assets = items.map((item) => mapAssetFromItem(item, baseUrl));
    if (kind) assets = assets.filter((asset) => asset.kind === kind);
    if (search) assets = assets.filter((asset) => asset.name.toLowerCase().includes(search));
    return assets;
  };

  const assetsForCounts = includeCounts
    ? applyClientFilters(allSourceItems)
    : applyClientFilters(sourceItems);
  const assets = applyClientFilters(sourceItems);

  if (options?.sort === 'name') assets.sort(compareMediaName);
  else assets.sort(compareMediaRecent);

  const counts = includeCounts
    ? {
        byKind: assetsForCounts.reduce<Record<string, number>>((acc, asset) => {
          acc[asset.kind] = (acc[asset.kind] ?? 0) + 1;
          return acc;
        }, {}),
        byCameraSource: assetsForCounts.reduce<Record<string, number>>((acc, asset) => {
          const source = asset.cameraSource?.trim();
          if (!source) return acc;
          acc[source] = (acc[source] ?? 0) + 1;
          return acc;
        }, {})
      }
    : undefined;

  const response: MediaListWorkerResponse = {
    requestId,
    assets,
    counts
  };
  self.postMessage(response);
};
