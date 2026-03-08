import { apiUrl } from '$lib';
import { normalizeUploadError, uploadSizeHeaders, verifyUploadedBytes } from '$lib/api/uploadIntegrity';
import { invalidateSWRPrefix } from '$lib/utils/swrCache';
import { classifyMediaKind, type MediaAssetType } from './mediaKind';
import { emitMediaMutation } from './mutations';
import { compareMediaName, compareMediaRecent, mediaTimestampIso } from './sort';

export type { MediaAssetType } from './mediaKind';

export interface MediaAsset {
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
}

function mediaUrl(path: string): string {
  const normalized = path.startsWith('/') ? path : `/${path}`;
  return apiUrl(normalized);
}

export function buildMediaArchiveDownloadUrl(assetIds: string[]): string {
  const params = new URLSearchParams();
  for (const rawId of assetIds) {
    const id = String(rawId ?? '').trim();
    if (!id) continue;
    params.append('name', id);
  }
  const query = params.toString();
  return mediaUrl(query ? `/media/download.zip?${query}` : '/media/download.zip');
}

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

function mapAssetFromItem(item: MediaItem): MediaAsset {
  const createdAt = mediaTimestampIso({
    name: item.name,
    captured_at_ms: item.captured_at_ms
  });
  const kind = classifyMediaKind({
    name: item.name,
    contentType: item.content_type,
    kindHint: item.kind,
    hasModelMetadata: Boolean(item.model_input_resolution || item.model_tensor_spec || item.label_attached || item.model_id)
  });
  const downloadPath = `/media/${encodeURIComponent(item.name)}`;
  const normalizedExtension = item.name.split('.').pop()?.trim().toLowerCase() ?? '';
  const normalizedCodec = (item.video_codec ?? '').trim().toLowerCase();
  const isRawBitstream = normalizedExtension === 'h264' || normalizedExtension === 'avc' || normalizedExtension === 'h265' || normalizedExtension === 'hevc';
  const needsTranscodedPlayback =
    kind === 'video' &&
    (isRawBitstream || normalizedCodec === 'h265' || normalizedCodec === 'hevc' || normalizedCodec === 'hvc1' || normalizedCodec === 'hev1');
  const previewPath = kind === 'video' ? `${downloadPath}/thumbnail` : downloadPath;
  const transcodedPath = `${downloadPath}/preview`;
  const playbackPath = kind === 'video' && needsTranscodedPlayback ? transcodedPath : downloadPath;
  return {
    id: item.name,
    kind,
    name: item.name,
    description: item.description ?? undefined,
    tags: Array.isArray(item.tags) ? item.tags : [],
    createdAt,
    updatedAt: createdAt,
    fileCount: 1,
    sizeBytes: item.size_bytes,
    previewUrl: kind === 'image' || kind === 'video' ? mediaUrl(previewPath) : undefined,
    playbackUrl: kind === 'video' ? mediaUrl(playbackPath) : undefined,
    mp4DownloadUrl: kind === 'video' && isRawBitstream ? mediaUrl(transcodedPath) : undefined,
    downloadUrl: mediaUrl(downloadPath),
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
    imuDataUrl: item.imu_data_file_name ? mediaUrl(`${downloadPath}/imu`) : undefined,
    frameTimestampsUrl: kind === 'video' ? mediaUrl(`${downloadPath}.frame_ts.txt`) : undefined
  };
}

export type MediaListSort = 'recent' | 'name';

export interface MediaListOptions {
  kind?: MediaAssetType;
  cameraSource?: string;
  page?: number;
  pageSize?: number;
  search?: string;
  includeCounts?: boolean;
  sort?: MediaListSort;
}

export interface MediaAssetListResult {
  assets: MediaAsset[];
  total: number;
  page: number;
  pageSize: number;
  counts?: {
    byKind: Record<string, number>;
    byCameraSource: Record<string, number>;
  };
}

type MediaListWorkerResult = {
  assets: MediaAsset[];
  counts?: {
    byKind: Record<string, number>;
    byCameraSource: Record<string, number>;
  };
};

let mediaListWorker: Worker | null = null;
let mediaListRequestId = 0;
const mediaListResolvers = new Map<number, { resolve: (result: MediaListWorkerResult) => void; reject: (error: unknown) => void }>();
const MEDIA_LIST_WORKER_TIMEOUT_MS = 2_000;

function ensureMediaListWorker(): Worker | null {
  if (mediaListWorker) return mediaListWorker;
  if (typeof Worker === 'undefined') return null;
  mediaListWorker = new Worker(new URL('$lib/workers/mediaListWorker.ts', import.meta.url), { type: 'module' });
  mediaListWorker.onmessage = (event) => {
    const payload = event.data as { requestId: number; assets?: MediaAsset[]; counts?: MediaAssetListResult['counts'] };
    const resolver = mediaListResolvers.get(payload.requestId);
    if (!resolver) return;
    mediaListResolvers.delete(payload.requestId);
    resolver.resolve({ assets: payload.assets ?? [], counts: payload.counts });
  };
  mediaListWorker.onerror = (event) => {
    console.warn('Media list worker error', event);
  };
  return mediaListWorker;
}

async function transformMediaList(options: {
  allItems: MediaItem[];
  visibleItems: MediaItem[];
  kind?: MediaAssetType;
  search?: string;
  sort?: MediaListSort;
  includeCounts: boolean;
  wantsServerFilter: boolean;
}): Promise<MediaListWorkerResult> {
  const worker = ensureMediaListWorker();
  if (worker) {
    const requestId = ++mediaListRequestId;
    const result = new Promise<MediaListWorkerResult>((resolve, reject) => {
      mediaListResolvers.set(requestId, { resolve, reject });
      setTimeout(() => {
        if (!mediaListResolvers.has(requestId)) return;
        mediaListResolvers.delete(requestId);
        reject(new Error('media_list_worker_timeout'));
      }, MEDIA_LIST_WORKER_TIMEOUT_MS);
    });
    worker.postMessage({
      requestId,
      allItems: options.allItems,
      visibleItems: options.visibleItems,
      options: {
        kind: options.kind,
        search: options.search,
        sort: options.sort,
        includeCounts: options.includeCounts,
        wantsServerFilter: options.wantsServerFilter,
        baseUrl: apiUrl('')
      }
    });
    try {
      return await result;
    } catch (error) {
      console.warn('Media list worker timed out, using fallback', error);
    }
  }

  const kind = options.kind;
  const search = options.search?.trim().toLowerCase();
  const sourceItems = options.wantsServerFilter ? options.visibleItems : options.allItems;

  const applyClientFilters = (items: MediaItem[]): MediaAsset[] => {
    let assets = items.map(mapAssetFromItem);
    if (kind) assets = assets.filter((asset) => asset.kind === kind);
    if (search) assets = assets.filter((asset) => asset.name.toLowerCase().includes(search));
    return assets;
  };

  const assetsForCounts = options.includeCounts
    ? applyClientFilters(options.allItems)
    : applyClientFilters(sourceItems);
  const assets = applyClientFilters(sourceItems);

  if (options.sort === 'name') assets.sort(compareMediaName);
  else assets.sort(compareMediaRecent);

  const counts = options.includeCounts
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

  return { assets, counts };
}

export async function listMediaAssets(_options: MediaListOptions = {}): Promise<MediaAssetListResult> {
  const includeCounts = _options.includeCounts !== false;
  const wantsServerFilter = Boolean(_options.cameraSource);

  const fetchItems = async (query: URLSearchParams | null): Promise<MediaItem[]> => {
    const url = query && query.toString().length ? mediaUrl(`/media?${query.toString()}`) : mediaUrl('/media');
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Failed to load assets (${response.status})`);
    return ((await response.json()) as MediaItem[]) ?? [];
  };

  const filteredQuery = wantsServerFilter ? new URLSearchParams({ stream_id: _options.cameraSource as string }) : null;
  const [allItems, visibleItems] = await (wantsServerFilter
    ? includeCounts
      ? Promise.all([fetchItems(null), fetchItems(filteredQuery)])
      : Promise.all([Promise.resolve([] as MediaItem[]), fetchItems(filteredQuery)])
    : Promise.all([fetchItems(null), Promise.resolve([] as MediaItem[])]));
  const resolvedVisibleItems = wantsServerFilter ? visibleItems : allItems;

  const kind = _options.kind;
  const search = _options.search?.trim().toLowerCase();

  const { assets, counts } = await transformMediaList({
    allItems,
    visibleItems: resolvedVisibleItems,
    kind,
    search,
    sort: _options.sort,
    includeCounts,
    wantsServerFilter
  });

  return {
    assets,
    total: assets.length,
    page: 1,
    pageSize: assets.length,
    counts
  };
}

export interface MediaUploadProgress {
  loaded: number;
  total?: number;
  percent: number | null;
}

export interface MediaUploadRequest {
  files: FileList | File[];
  kind: MediaAssetType;
  name?: string;
  description?: string;
  tags?: string[];
  cameraSource?: string;
  loopMode?: boolean;
  rotation?: number;
  fps?: number;
  labelFile?: File | null;
  modelInputResolution?: string;
  modelTensorSpec?: string;
  onProgress?: (progress: MediaUploadProgress) => void;
}

export async function uploadMediaAsset(request: MediaUploadRequest): Promise<MediaAsset> {
  const providedFiles = request.files ? (Array.isArray(request.files) ? request.files : Array.from(request.files)) : [];
  if (!providedFiles.length) {
    throw new Error('Select at least one file to upload.');
  }

  if (providedFiles.length > 1) {
    throw new Error('This backend only supports uploading one file per request.');
  }

  const file = providedFiles[0];
  const form = new FormData();
  const desiredName = request.name?.trim();
  const uploadFilename = desiredName && desiredName.length > 0 ? desiredName : file.name;
  form.append('file', file, uploadFilename);
  form.set('kind', request.kind);
  if (desiredName) form.set('name', desiredName);
  if (request.description) form.set('description', request.description);
  if (request.tags?.length) form.set('tags', request.tags.join(','));
  if (request.cameraSource) form.set('cameraSource', request.cameraSource);
  if (typeof request.loopMode === 'boolean') form.set('loopMode', request.loopMode ? 'true' : 'false');
  if (typeof request.rotation === 'number') form.set('rotation', request.rotation.toString());
  if (typeof request.fps === 'number') form.set('fps', request.fps.toString());
  if (request.labelFile) {
    form.set('label', request.labelFile, request.labelFile.name);
  }
  if (request.modelInputResolution) {
    form.set('modelInputResolution', request.modelInputResolution);
  }
  if (request.modelTensorSpec) {
    form.set('modelTensorSpec', request.modelTensorSpec);
  }

  const endpoint = mediaUrl('/media');
  if (typeof XMLHttpRequest === 'undefined') {
    let response: Response;
    try {
      response = await fetch(endpoint, { method: 'POST', body: form, headers: uploadSizeHeaders(file) });
    } catch (error) {
      throw normalizeUploadError(error, 'Media upload');
    }
    if (!response.ok) {
      const text = await response.text();
      throw new Error(text || `Upload failed (${response.status})`);
    }
    const payload = (await response.json()) as MediaItem;
    verifyUploadedBytes(file.size, payload.size_bytes, 'Media upload');
    return mapAssetFromItem(payload);
  }

  const payload = await new Promise<MediaItem>((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('POST', endpoint);
    xhr.responseType = 'json';
    xhr.onerror = () => {
      reject(normalizeUploadError(new Error('Network error during upload'), 'Media upload'));
    };
    const uploadHeaders = uploadSizeHeaders(file);
    for (const [headerName, headerValue] of Object.entries(uploadHeaders)) {
      xhr.setRequestHeader(headerName, headerValue);
    }
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        try {
          if (xhr.response) {
            const responsePayload = xhr.response as MediaItem;
            verifyUploadedBytes(file.size, responsePayload.size_bytes, 'Media upload');
            resolve(responsePayload);
            return;
          }
          const parsed = JSON.parse(xhr.responseText) as MediaItem;
          verifyUploadedBytes(file.size, parsed.size_bytes, 'Media upload');
          resolve(parsed);
        } catch (error) {
          reject(error instanceof Error ? error : new Error('Failed to parse upload response'));
        }
        return;
      }
      reject(new Error(xhr.responseText || `Upload failed (${xhr.status})`));
    };
    xhr.upload.onprogress = (event) => {
      request.onProgress?.({
        loaded: event.loaded,
        total: event.lengthComputable ? event.total : undefined,
        percent: event.lengthComputable ? (event.loaded / event.total) * 100 : null
      });
    };
    xhr.send(form);
  });

  const asset = mapAssetFromItem(payload);
  invalidateSWRPrefix('media:');
  emitMediaMutation({
    mutation: 'created',
    mediaKind: asset.kind,
    cameraSource: asset.cameraSource ?? null,
    mediaId: asset.id
  });
  return asset;
}

export async function updateMediaAssetMetadata(
  assetId: string,
  payload: { name?: string; description?: string; tags?: string[]; modelInputResolution?: string | null; modelTensorSpec?: string | null }
): Promise<MediaAsset> {
  const response = await fetch(mediaUrl(`/media/${encodeURIComponent(assetId)}/metadata`), {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      name: payload.name,
      description: payload.description,
      tags: payload.tags,
      model_input_resolution: payload.modelInputResolution,
      model_tensor_spec: payload.modelTensorSpec
    })
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `Failed to update metadata (${response.status})`);
  }
  const item = (await response.json()) as MediaItem;
  invalidateSWRPrefix('media:');
  const asset = mapAssetFromItem(item);
  emitMediaMutation({
    mutation: 'updated',
    mediaKind: asset.kind,
    cameraSource: asset.cameraSource ?? null,
    mediaId: asset.id
  });
  return asset;
}

export async function deleteMediaAsset(assetId: string): Promise<void> {
  const response = await fetch(mediaUrl(`/media/${encodeURIComponent(assetId)}`), { method: 'DELETE' });
  if (!response.ok) {
    throw new Error(`Failed to delete asset (${response.status})`);
  }
  invalidateSWRPrefix('media:');
  emitMediaMutation({
    mutation: 'deleted',
    mediaKind: 'unknown',
    mediaId: assetId
  });
}

export async function attachLabelFile(assetId: string, file: File): Promise<MediaAsset> {
  const form = new FormData();
  form.set('label', file, file.name);
  let response: Response;
  try {
    response = await fetch(mediaUrl(`/media/${encodeURIComponent(assetId)}/label`), {
      method: 'POST',
      body: form,
      headers: uploadSizeHeaders(file)
    });
  } catch (error) {
    throw normalizeUploadError(error, 'Label upload');
  }
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `Failed to attach label (${response.status})`);
  }
  const item = (await response.json()) as MediaItem;
  invalidateSWRPrefix('media:');
  const asset = mapAssetFromItem(item);
  emitMediaMutation({
    mutation: 'updated',
    mediaKind: asset.kind,
    cameraSource: asset.cameraSource ?? null,
    mediaId: asset.id
  });
  return asset;
}

export interface ImageEditPayload {
  rotateDegrees?: number;
  crop?: { x: number; y: number; width: number; height: number };
}

export async function applyImageEdits(assetId: string, payload: ImageEditPayload): Promise<MediaAsset> {
  const response = await fetch(mediaUrl(`/media/${encodeURIComponent(assetId)}/image/edits`), {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      rotate_degrees: payload.rotateDegrees,
      crop: payload.crop
    })
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `Failed to apply image edits (${response.status})`);
  }
  const item = (await response.json()) as MediaItem;
  invalidateSWRPrefix('media:');
  const asset = mapAssetFromItem(item);
  emitMediaMutation({
    mutation: 'updated',
    mediaKind: asset.kind,
    cameraSource: asset.cameraSource ?? null,
    mediaId: asset.id
  });
  return asset;
}

export interface VideoEditPayload {
  startMs: number;
  endMs?: number;
}

export async function applyVideoEdits(assetId: string, payload: VideoEditPayload): Promise<MediaAsset> {
  void payload;
  invalidateSWRPrefix('media:');
  const asset = mapAssetFromItem({ name: assetId, size_bytes: 0, content_type: 'application/octet-stream' });
  emitMediaMutation({
    mutation: 'updated',
    mediaKind: asset.kind,
    cameraSource: asset.cameraSource ?? null,
    mediaId: asset.id
  });
  return asset;
}

export async function fetchLabelText(assetId: string): Promise<string> {
  const response = await fetch(mediaUrl(`/media/${encodeURIComponent(assetId)}/label`));
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `Failed to fetch label (${response.status})`);
  }
  return await response.text();
}
