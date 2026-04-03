import type { PipelineDataType } from '$lib/types/pipeline';
import { apiUrl } from '$lib/api/client';
import { apiFetch, apiFetchResponse } from '$lib/api/core/http';
import { resolveStreamLabel } from '$lib/utils/streamLabels';
import type { StreamInfo } from '$lib/api/client';

export type LocalizationSourceKind = 'detection' | 'pose' | 'imu';

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
  localizationKind: LocalizationSourceKind;
  dataType: PipelineDataType | null;
};

export type PipelineOutputSample = {
  dataType: PipelineDataType | null;
  value: unknown;
};

type UnknownRecord = Record<string, unknown>;

const asRecord = (value: unknown): UnknownRecord | null =>
  value && typeof value === 'object' ? (value as UnknownRecord) : null;

export const isLocalizationDetectionSource = (source: LocalizationPipelineSource): boolean => {
  return source.localizationKind === 'detection';
};

export const isLocalizationImuSource = (source: LocalizationPipelineSource): boolean => {
  return source.localizationKind === 'imu';
};

export const isLocalizationPoseSource = (source: LocalizationPipelineSource): boolean => {
  return source.localizationKind === 'pose';
};

export const isLocalizationCompatibleSource = (source: LocalizationPipelineSource): boolean => {
  return source.localizationKind === 'detection' || source.localizationKind === 'pose' || source.localizationKind === 'imu';
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
    localizationKind: current.localizationKind ?? candidate.localizationKind,
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
        localizationKind: source.localizationKind,
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
  return apiFetch<T>(url, { method: 'GET', headers: { Accept: 'application/json' } });
};

const normalizeStreamLabel = (stream: StreamInfo): string => {
  return resolveStreamLabel(stream, 'Stream');
};

const normalizeCameraUid = (stream: StreamInfo): string => {
  const manifest = asRecord(stream.manifest);
  const identity = asRecord(manifest?.identity);
  const keys = Array.isArray(identity?.keys)
    ? identity.keys.map((value: unknown) => String(value ?? '').trim()).filter((value: string) => value.length > 0)
    : [];
  if (keys.length > 0) {
    return keys[0] ?? '';
  }
  const uid = identity?.hardware_id ?? identity?.id ?? stream?.id ?? '';
  return String(uid).trim() || String(stream?.id ?? '');
};

const normalizeCameraPath = (stream: StreamInfo, cameraUid: string): string => {
  if (cameraUid) return `device:${cameraUid}`;
  return `stream:${String(stream?.id ?? '').trim()}`;
};

export async function fetchLocalizationPipelineSources(): Promise<LocalizationPipelineSource[]> {
  const baseSources = await fetchJson<LocalizationPipelineSource[]>(apiUrl('/localization/sources'));
  try {
    const streams = await fetchJson<StreamInfo[]>(apiUrl('/streams'));
    const streamIndex = new Map(
      streams.map((stream) => {
        const streamId = String(stream?.id ?? '').trim();
        const cameraUid = normalizeCameraUid(stream);
        const cameraKeys = Array.isArray(stream?.manifest?.identity?.keys)
          ? stream.manifest.identity.keys
              .map((value: unknown) => String(value ?? '').trim())
              .filter((value: string) => value.length > 0)
          : [];
        return [
          streamId,
          {
            streamLabel: normalizeStreamLabel(stream),
            cameraUid,
            cameraPath: normalizeCameraPath(stream, cameraUid),
            cameraKeys
          }
        ] as const;
      })
    );

    return dedupeLocalizationSources(
      baseSources.map((source) => {
        const streamMeta = streamIndex.get(clean(source.streamId));
        return {
          ...source,
          streamLabel: clean(source.streamLabel) || streamMeta?.streamLabel || clean(source.streamId),
          cameraUid: clean(source.cameraUid) || streamMeta?.cameraUid || clean(source.streamId),
          cameraPath: clean(source.cameraPath) || streamMeta?.cameraPath || '',
          cameraKeys:
            streamMeta?.cameraKeys?.length
              ? Array.from(
                  new Set([
                    ...(source.cameraKeys ?? []).map((value) => clean(value)),
                    ...streamMeta.cameraKeys
                  ].filter((value) => value.length > 0))
                )
              : source.cameraKeys
        };
      })
    );
  } catch {
    return dedupeLocalizationSources(baseSources);
  }
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
  const response = await apiFetchResponse(url, {
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
