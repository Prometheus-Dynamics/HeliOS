import type { PipelineDataType } from '$lib/types/pipeline';
import { apiUrl } from '$lib/api/httpClient';
import { extractGraphOutputPorts } from '$lib/features/pipelines/graphOutputPorts';
import { extractGraphOutputPortTypes } from '$lib/features/pipelines/outputFilters';
import { resolveStreamLabel } from '$lib/utils/streamLabels';

export type LocalizationPipelineSource = {
  id: string;
  streamId: string;
  streamLabel: string;
  cameraUid: string;
  cameraPath: string;
  cameraKeys?: string[];
  pipelineId: string;
  pipelineLabel: string;
  outputKey: string;
  dataType: PipelineDataType | null;
};

export type PipelineOutputSample = {
  dataType: PipelineDataType | null;
  value: unknown;
};

const toLower = (value: string | null | undefined): string => clean(value).toLowerCase();

const outputKeyLooksImage = (outputKey: string): boolean => {
  const key = outputKey.trim().toLowerCase();
  return key === 'frame' || key === 'raw' || key === 'undistorted' || key.includes('image') || key.includes('frame');
};

export const isLocalizationDetectionSource = (source: LocalizationPipelineSource): boolean => {
  const key = toLower(source.outputKey);
  if (!key) return false;
  const detectionLike =
    key.includes('aruco') ||
    key.includes('detect') ||
    key.includes('detection') ||
    key.includes('tag_pose') ||
    key.includes('tag_poses') ||
    key.startsWith('tag_in_') ||
    key.startsWith('camera_in_tag') ||
    key.startsWith('robot_in_tag');
  if (!detectionLike) return false;
  return !outputKeyLooksImage(key);
};

export const isLocalizationImuSource = (source: LocalizationPipelineSource): boolean => {
  const streamId = toLower(source.streamId);
  const outputKey = toLower(source.outputKey);
  const cameraUid = toLower(source.cameraUid);
  return (
    streamId.startsWith('external:imu') ||
    outputKey.includes('imu_pose') ||
    outputKey.startsWith('imu_') ||
    cameraUid === 'imu'
  );
};

export const isLocalizationPoseSource = (source: LocalizationPipelineSource): boolean => {
  const key = toLower(source.outputKey);
  if (!key) return false;
  const poseLike =
    key.startsWith('solver:') ||
    key.startsWith('tag_in_') ||
    key.startsWith('camera_in_') ||
    key.startsWith('robot_in_') ||
    key.includes('pose');
  if (!poseLike) return false;
  if (isLocalizationDetectionSource(source)) return true;
  if (isLocalizationImuSource(source)) return true;
  return !outputKeyLooksImage(key);
};

const stringifyDataType = (value: PipelineDataType | null | undefined): string => {
  if (value == null) return '';
  if (typeof value === 'string') return value.toLowerCase();
  try {
    return JSON.stringify(value).toLowerCase();
  } catch {
    return '';
  }
};

const dataTypeLooksLocalization = (value: PipelineDataType | null | undefined): boolean => {
  const text = stringifyDataType(value);
  if (!text) return false;
  return (
    text.includes('localization') ||
    text.includes('detection') ||
    text.includes('aruco') ||
    text.includes('tag_pose') ||
    text.includes('tag_poses') ||
    text.includes('tag_in_') ||
    text.includes('camera_in_') ||
    text.includes('robot_in_') ||
    text.includes('imu') ||
    text.includes('pose')
  );
};

const dataTypeLooksImage = (value: PipelineDataType | null | undefined): boolean => {
  const text = stringifyDataType(value);
  if (!text) return false;
  return (
    text.includes('image') ||
    text.includes('frame') ||
    text.includes('rgb') ||
    text.includes('bgr') ||
    text.includes('nv12') ||
    text.includes('yuv') ||
    text.includes('jpeg') ||
    text.includes('png')
  );
};

export const isLocalizationCompatibleSource = (source: LocalizationPipelineSource): boolean => {
  const outputKey = clean(source.outputKey);
  if (!outputKey) return false;
  if (outputKey.toLowerCase() === 'frame') return false;
  if (isLocalizationDetectionSource(source) || isLocalizationImuSource(source) || isLocalizationPoseSource(source)) {
    return true;
  }
  return dataTypeLooksLocalization(source.dataType) && !dataTypeLooksImage(source.dataType);
};

const UUID_LIKE_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

const hasDataType = (value: PipelineDataType | null | undefined): boolean => {
  if (value == null) return false;
  if (typeof value === 'string') return value.trim().length > 0;
  if (typeof value !== 'object') return false;
  const kind = String(value.kind ?? '').trim();
  const label = String(value.label ?? '').trim();
  const format = String(value.format ?? '').trim();
  return kind.length > 0 || label.length > 0 || format.length > 0;
};

const clean = (value: string | null | undefined): string => String(value ?? '').trim();

const scoreLabel = (label: string, streamId: string): number => {
  const value = clean(label);
  if (!value) return 0;
  let score = 1;
  if (!UUID_LIKE_RE.test(value)) score += 1;
  if (value !== streamId) score += 1;
  return score;
};

const scoreCameraPath = (cameraPath: string): number => {
  const value = clean(cameraPath);
  if (!value) return 0;
  if (value.startsWith('device:') || value.startsWith('/')) return 3;
  if (value.startsWith('stream:')) return 1;
  return 2;
};

const scorePipelineId = (pipelineId: string): number => {
  const value = clean(pipelineId).toLowerCase();
  if (!value || value === 'none' || value === 'pipeline') return 0;
  return 1;
};

const sourceMergeKey = (source: LocalizationPipelineSource): string =>
  `${clean(source.streamId)}::${clean(source.outputKey)}`;

const defaultCanonicalId = (source: LocalizationPipelineSource): string =>
  `${clean(source.streamId)}:${clean(source.outputKey)}`;

const mergedSourceId = (
  current: LocalizationPipelineSource,
  candidate: LocalizationPipelineSource
): string => {
  const canonical = defaultCanonicalId(current);
  if (current.id === canonical) return current.id;
  if (candidate.id === canonical) return candidate.id;

  const currentId = clean(current.id);
  const candidateId = clean(candidate.id);
  if (!currentId) return candidateId || canonical;
  if (!candidateId) return currentId;
  if (candidateId.startsWith('stream:') && !currentId.startsWith('stream:')) return currentId;
  if (currentId.startsWith('stream:') && !candidateId.startsWith('stream:')) return candidateId;
  return currentId;
};

const mergeSourceEntries = (
  current: LocalizationPipelineSource,
  candidate: LocalizationPipelineSource
): LocalizationPipelineSource => {
  const streamId = clean(current.streamId) || clean(candidate.streamId);
  const outputKey = clean(current.outputKey) || clean(candidate.outputKey);
  const currentLabelScore = scoreLabel(current.streamLabel, streamId);
  const candidateLabelScore = scoreLabel(candidate.streamLabel, streamId);

  const streamLabel =
    candidateLabelScore > currentLabelScore
      ? clean(candidate.streamLabel)
      : clean(current.streamLabel) || clean(candidate.streamLabel) || streamId;

  const currentPathScore = scoreCameraPath(current.cameraPath);
  const candidatePathScore = scoreCameraPath(candidate.cameraPath);
  const cameraPath =
    candidatePathScore > currentPathScore
      ? clean(candidate.cameraPath)
      : clean(current.cameraPath) || clean(candidate.cameraPath);

  const currentPipelineScore = scorePipelineId(current.pipelineId);
  const candidatePipelineScore = scorePipelineId(candidate.pipelineId);
  const pipelineId =
    candidatePipelineScore > currentPipelineScore
      ? clean(candidate.pipelineId)
      : clean(current.pipelineId) || clean(candidate.pipelineId) || 'none';
  const pipelineLabel =
    candidatePipelineScore > currentPipelineScore
      ? clean(candidate.pipelineLabel)
      : clean(current.pipelineLabel) || clean(candidate.pipelineLabel) || 'Pipeline';

  const cameraUid =
    clean(current.cameraUid) || clean(candidate.cameraUid) || streamId || outputKey;

  const cameraKeys = Array.from(
    new Set([
      ...(current.cameraKeys ?? []).map((value) => clean(value)),
      ...(candidate.cameraKeys ?? []).map((value) => clean(value)),
      cameraUid
    ].filter((value) => value.length > 0))
  );

  return {
    id: mergedSourceId(current, candidate),
    streamId,
    streamLabel,
    cameraUid,
    cameraPath,
    cameraKeys,
    pipelineId,
    pipelineLabel,
    outputKey,
    dataType: hasDataType(current.dataType)
      ? current.dataType
      : hasDataType(candidate.dataType)
        ? candidate.dataType
        : null
  };
};

const dedupeLocalizationSources = (
  sources: LocalizationPipelineSource[]
): LocalizationPipelineSource[] => {
  const merged = new Map<string, LocalizationPipelineSource>();
  for (const source of sources) {
    const key = sourceMergeKey(source);
    const current = merged.get(key);
    if (!current) {
      merged.set(key, {
        ...source,
        id: clean(source.id) || defaultCanonicalId(source),
        streamId: clean(source.streamId),
        streamLabel: clean(source.streamLabel),
        cameraUid: clean(source.cameraUid),
        cameraPath: clean(source.cameraPath),
        pipelineId: clean(source.pipelineId),
        pipelineLabel: clean(source.pipelineLabel),
        outputKey: clean(source.outputKey),
        cameraKeys: (source.cameraKeys ?? [])
          .map((value) => clean(value))
          .filter((value) => value.length > 0)
      });
      continue;
    }
    merged.set(key, mergeSourceEntries(current, source));
  }

  return Array.from(merged.values()).sort((left, right) => {
    const streamOrder = left.streamLabel.localeCompare(right.streamLabel);
    if (streamOrder !== 0) return streamOrder;
    const outputOrder = left.outputKey.localeCompare(right.outputKey);
    if (outputOrder !== 0) return outputOrder;
    return left.id.localeCompare(right.id);
  });
};

const fetchJson = async <T>(url: string): Promise<T> => {
  const response = await fetch(url, { method: 'GET', headers: { Accept: 'application/json' } });
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(text || `Request failed (${response.status})`);
  }
  return (await response.json()) as T;
};

const normalizeStreamLabel = (stream: any): string => {
  return resolveStreamLabel(stream, 'Stream');
};

const normalizeCameraUid = (stream: any): string => {
  const manifest = stream?.manifest ?? {};
  const identity = manifest?.identity ?? {};
  const keys = Array.isArray(identity?.keys)
    ? identity.keys.map((value: unknown) => String(value ?? '').trim()).filter((value: string) => value.length > 0)
    : [];
  if (keys.length > 0) {
    return keys[0] ?? '';
  }
  const uid = identity?.hardware_id ?? identity?.id ?? stream?.id ?? '';
  return String(uid).trim() || String(stream?.id ?? '');
};

const normalizeCameraPath = (stream: any, cameraUid: string): string => {
  if (cameraUid) return `device:${cameraUid}`;
  return `stream:${String(stream?.id ?? '').trim()}`;
};

const collectPipelineIdsForStream = (stream: any): string[] => {
  const manifest = stream?.manifest ?? {};
  const ids = new Set<string>();
  const add = (value: unknown) => {
    const raw = typeof value === 'string' ? value.trim() : '';
    if (raw) ids.add(raw);
  };
  add(manifest?.pipeline_id);
  add(manifest?.active_pipeline_id);
  if (Array.isArray(manifest?.pipelines)) {
    manifest.pipelines.forEach((entry: any) => {
      add(entry?.pipeline_id ?? entry?.pipelineId ?? entry?.id);
    });
  }
  if (manifest?.pipeline_layout && Array.isArray(manifest.pipeline_layout.slots)) {
    manifest.pipeline_layout.slots.forEach((slot: any) => add(slot?.pipeline_id));
  }
  return Array.from(ids);
};

export async function fetchLocalizationPipelineSources(): Promise<LocalizationPipelineSource[]> {
  const baseSources = await fetchJson<LocalizationPipelineSource[]>(apiUrl('/localization/sources'));
  let streamSources: LocalizationPipelineSource[] = [];
  try {
    const streams = await fetchJson<any[]>(apiUrl('/streams'));
    const pipelineSummaries = await fetchJson<Array<{ id: string; name?: string | null }>>(apiUrl('/pipelines/graphs')).catch(() => []);
    const pipelineNameById = Object.fromEntries(
      (pipelineSummaries ?? []).map((entry) => [String(entry.id), String(entry.name ?? entry.id)])
    );
    const pipelineGraphCache = new Map<string, any>();
    for (const stream of streams) {
      const streamId = String(stream?.id ?? '').trim();
      if (!streamId) continue;
      const streamLabel = normalizeStreamLabel(stream);
      const cameraUid = normalizeCameraUid(stream);
      const cameraPath = normalizeCameraPath(stream, cameraUid);
      const identityKeys = Array.isArray(stream?.manifest?.identity?.keys)
        ? stream.manifest.identity.keys
            .map((value: unknown) => String(value ?? '').trim())
            .filter((value: string) => value.length > 0)
        : [];
      const pipelineIds = collectPipelineIdsForStream(stream);
      for (const pipelineId of pipelineIds) {
        if (!pipelineId) continue;
        let graph = pipelineGraphCache.get(pipelineId);
        if (!graph) {
          const binding = (stream?.manifest?.pipelines ?? []).find((entry: any) => String(entry?.pipeline_id ?? '').trim() === pipelineId);
          graph = binding?.pipeline_graph ?? stream?.manifest?.pipeline_graph ?? null;
        }
        if (!graph) {
          try {
            const doc = await fetchJson<{ graph: any }>(apiUrl(`/pipelines/graphs/${encodeURIComponent(pipelineId)}`));
            graph = doc?.graph ?? null;
          } catch {
            graph = null;
          }
        }
        if (!graph) continue;
        pipelineGraphCache.set(pipelineId, graph);
        const outputKeys = extractGraphOutputPorts(graph);
        const outputTypes = extractGraphOutputPortTypes(graph);
        const pipelineLabel = pipelineNameById[pipelineId] ?? pipelineId;
        outputKeys.forEach((outputKey) => {
          if (!outputKey) return;
          const id = `stream:${streamId}:${pipelineId}:${outputKey}`;
          streamSources.push({
            id,
            streamId,
            streamLabel,
            cameraUid,
            cameraPath,
            cameraKeys: identityKeys,
            pipelineId,
            pipelineLabel,
            outputKey,
            dataType: outputTypes[outputKey] ?? null
          });
        });
      }
    }
  } catch {
    streamSources = [];
  }
  return dedupeLocalizationSources([...baseSources, ...streamSources]);
}

export async function fetchPipelineOutputSample(
  streamId: string,
  _pipelineId: string,
  outputKey: string,
  signal?: AbortSignal
): Promise<PipelineOutputSample | null> {
  const url = streamId.startsWith('peer:')
    ? apiUrl(`/localization/peers/${encodeURIComponent(streamId.slice('peer:'.length))}/outputs/${encodeURIComponent(outputKey)}`)
    : streamId.startsWith('profile:')
      ? apiUrl(`/localization/profiles/${encodeURIComponent(streamId.slice('profile:'.length))}/outputs/${encodeURIComponent(outputKey)}`)
    : streamId.startsWith('external:')
      ? apiUrl(
          `/localization/external/${encodeURIComponent(streamId.slice('external:'.length))}/outputs/${encodeURIComponent(outputKey)}`
        )
      : apiUrl(`/localization/streams/${encodeURIComponent(streamId)}/outputs/${encodeURIComponent(outputKey)}`);
  const response = await fetch(url, {
    method: 'GET',
    headers: {
      Accept: 'application/json'
    },
    signal
  });

  if (response.status === 404) {
    return null;
  }

  if (!response.ok) {
    const text = await response.text().catch(() => '');
    const message = text || `Request failed (${response.status})`;
    throw new Error(message);
  }

  return (await response.json()) as PipelineOutputSample;
}
