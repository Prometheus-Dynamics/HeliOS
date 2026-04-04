import type {
  CaptureConfig,
  CodecInfo,
  Interval,
  Mode,
  ModeId,
  PeerInfo,
  PipelineSummary,
  PipelineTemplateSummary,
  ProbedBackend,
  ProbedDevice,
  StreamCapabilitiesResponse,
  StreamManifest
} from '$lib/api/client';
import { ApiError } from '$lib/api/client';
import { PipelinesApi } from '$lib/api/pipelinesApi';
import { resolveStreamCreationDefaults } from '$lib/api/streamDefaults';
import {
  buildEncoderSettingsForSelection,
  type EncoderSettingsDraft
} from '$lib/api/streamEncoderSettings';
import { normalizeRecordingMode } from '$lib/api/streamRecordingMode';
import { withCurrentStreamManifestSchema } from '$lib/api/streamSchema';
import { SvelteSet, SvelteURL } from 'svelte/reactivity';

export type RegisterExperience = 'simple' | 'advanced';
export type SimpleStreamKind = 'bw' | 'color';
export type SimplePipelineSource = 'none' | 'existing' | 'template';

const SIMPLE_KIND_FORMATS: Record<SimpleStreamKind, string[]> = {
  bw: ['N12', 'NV12'],
  color: ['YUYV']
};

export function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' ? (value as Record<string, unknown>) : null;
}

export function pipelineDisplayName(entry: PipelineSummary | null | undefined): string {
  const name = String(entry?.name ?? '').trim();
  return name.length ? name : entry?.id ?? 'Pipeline';
}

export function encodeSimpleAttachSelection(
  source: SimplePipelineSource,
  pipelineId: string | null,
  templateId: string | null
): string {
  if (source === 'existing' && pipelineId) return `pipeline:${pipelineId}`;
  if (source === 'template' && templateId) return `template:${templateId}`;
  return 'none';
}

function normalizeFormatCode(value: string | null | undefined): string {
  return String(value ?? '')
    .trim()
    .toUpperCase()
    .replace(/[^A-Z0-9]/g, '');
}

export function modeMatchesSimpleKind(mode: Mode | null | undefined, kind: SimpleStreamKind): boolean {
  const code = normalizeFormatCode(mode?.format?.code ?? null);
  if (!code) return false;
  return SIMPLE_KIND_FORMATS[kind].some((candidate) => normalizeFormatCode(candidate) === code);
}

function backendSupportsSimpleKind(backend: ProbedBackend | null, kind: SimpleStreamKind): boolean {
  return (backend?.descriptor?.modes ?? []).some((mode) => modeMatchesSimpleKind(mode, kind));
}

export function findBackendIndexForSimpleKind(device: ProbedDevice | null, kind: SimpleStreamKind): number {
  const backends = device?.backends ?? [];
  const index = backends.findIndex((backend) => backendSupportsSimpleKind(backend, kind));
  return index >= 0 ? index : 0;
}

export function dedupeModesByResolution(
  modes: Mode[],
  resolutionKey: (mode: Mode | undefined) => string | null
): Mode[] {
  const seen = new SvelteSet<string>();
  const out: Mode[] = [];
  for (const mode of modes) {
    const key = resolutionKey(mode);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push(mode);
  }
  return out;
}

function safeParseUrl(raw: string): { port?: string; endpoint?: string } | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  try {
    const url = new SvelteURL(trimmed);
    const port =
      url.port ||
      (url.protocol === 'http:' ? '80' : url.protocol === 'https:' ? '443' : url.protocol === 'rtsp:' ? '554' : '');
    const endpoint = (() => {
      const path = url.pathname?.trim() ?? '';
      if (path && path !== '/') {
        const last = path.split('/').filter(Boolean).pop();
        if (last) return last;
      }
      if (url.search) {
        const q = url.search.replace(/^\?/, '').trim();
        if (q) return q;
      }
      return null;
    })();
    return { port: port || undefined, endpoint: endpoint ?? undefined };
  } catch {
    return null;
  }
}

export function buildPeerStreamDevices(peers: PeerInfo[]): ProbedDevice[] {
  const out: ProbedDevice[] = [];
  for (const peer of peers) {
    if (!peer || typeof peer !== 'object') continue;
    const labelBase = String(peer.alias ?? peer.id ?? 'Peer').trim() || 'Peer';
    const integration = asRecord(peer.integration);
    const integrationKind = String(peer.integration?.kind ?? '').trim().toLowerCase();
    if (integrationKind === 'helios') continue;

    const urls = [
      ...(Array.isArray(integration?.streamUrls) ? integration.streamUrls : []),
      ...(Array.isArray(peer.integration?.stream_urls) ? peer.integration.stream_urls : []),
      integration?.streamUrl,
      peer.integration?.stream_url
    ]
      .map((value) => (typeof value === 'string' ? value.trim() : ''))
      .filter((value: string) => value.length > 0);

    const deduped = Array.from(new SvelteSet(urls));
    for (const url of deduped) {
      const parsed = safeParseUrl(url);
      const portLabel = parsed?.port ? parsed.port : '';
      const endpoint = parsed?.endpoint ?? 'stream';
      const display = `${labelBase} - ${portLabel}${portLabel ? ' ' : ''}${endpoint}`;
      const peerKey = (() => {
        const port = portLabel.trim();
        if (!port) return null;
        const alias = labelBase
          .trim()
          .toLowerCase()
          .replace(/\s+/g, '-')
          .replace(/[^a-z0-9_-]/g, '');
        const stable = alias || 'peer';
        return `peer:${stable}:${port}`;
      })();

      out.push({
        identity: { display, keys: peerKey ? [peerKey, url] : [url] },
        backends: [
          {
            kind: 'Netcam',
            handle: { type: 'netcam', url, width: 0, height: 0, fps: 30 } as unknown as ProbedBackend['handle'],
            properties: [],
            descriptor: {
              controls: [],
              modes: [
                {
                  format: { code: 'MJPG', color: 'Srgb', resolution: { width: 1, height: 1 } },
                  id: {
                    format: { code: 'MJPG', color: 'Srgb', resolution: { width: 1, height: 1 } },
                    interval: { numerator: 1, denominator: 30 }
                  },
                  intervals: [{ numerator: 1, denominator: 30 }]
                }
              ]
            }
          }
        ]
      });
    }
  }
  return out;
}

function fpsOf(interval: Interval | null | undefined): number | null {
  if (!interval) return null;
  const num = Number(interval.numerator);
  const den = Number(interval.denominator);
  if (!Number.isFinite(num) || !Number.isFinite(den) || num <= 0 || den <= 0) return null;
  return den / num;
}

export function bestIntervalIndex(target: Interval | null | undefined, candidates: Interval[]): number | null {
  const want = fpsOf(target);
  if (want == null || !candidates.length) return null;
  let bestIdx = 0;
  let bestDiff = Infinity;
  for (let i = 0; i < candidates.length; i += 1) {
    const fps = fpsOf(candidates[i]);
    if (fps == null) continue;
    const diff = Math.abs(fps - want);
    if (diff < bestDiff) {
      bestDiff = diff;
      bestIdx = i;
    }
  }
  return Number.isFinite(bestDiff) ? bestIdx : null;
}

function parseResolutionKey(key: string | null | undefined): { w: number; h: number } | null {
  if (!key) return null;
  const match = String(key).match(/^(\d+)x(\d+)$/);
  if (!match) return null;
  const w = Number(match[1]);
  const h = Number(match[2]);
  if (!Number.isFinite(w) || !Number.isFinite(h) || w <= 0 || h <= 0) return null;
  return { w, h };
}

export function bestResolutionModeForFormat(
  modes: Mode[],
  formatCodecLabel: (fmt: string | null | undefined) => string,
  resolutionKey: (mode: Mode | undefined) => string | null,
  format: string | null,
  targetKey: string | null
): Mode | null {
  const matchingModes = modes.filter((mode) => formatCodecLabel(mode.format?.code) === format);
  if (!matchingModes.length) return modes[0] ?? null;
  const wanted = parseResolutionKey(targetKey);
  if (!wanted) return matchingModes[0] ?? null;

  let best: Mode | null = null;
  let bestScore = Infinity;
  for (const mode of matchingModes) {
    const key = resolutionKey(mode);
    const resolution = parseResolutionKey(key);
    if (!resolution) continue;
    const dx = resolution.w - wanted.w;
    const dy = resolution.h - wanted.h;
    const score = dx * dx + dy * dy;
    if (score < bestScore) {
      bestScore = score;
      best = mode;
    }
  }
  return best ?? matchingModes[0] ?? null;
}

export function resolveSimpleMode(
  backend: ProbedBackend | null,
  kind: SimpleStreamKind,
  selectedResolutionKey: string | null,
  resolutionKey: (mode: Mode | undefined) => string | null
): Mode | null {
  const matches = (backend?.descriptor?.modes ?? []).filter((mode) => modeMatchesSimpleKind(mode, kind));
  if (!matches.length) return null;
  return matches.find((mode) => resolutionKey(mode) === selectedResolutionKey) ?? matches[0] ?? null;
}

export async function resolveSimpleAttachPipelineId(args: {
  source: SimplePipelineSource;
  pipelineId: string | null;
  templateId: string | null;
  alias: string;
  device: ProbedDevice;
}): Promise<string | null> {
  if (args.source === 'none') return null;
  if (args.source === 'existing') {
    const id = String(args.pipelineId ?? '').trim();
    return id || null;
  }

  const templateId = String(args.templateId ?? '').trim();
  if (!templateId) return null;
  const templateDoc = await PipelinesApi.fetchTemplate({ id: templateId });
  const templateGraph = templateDoc?.graph;
  if (!templateGraph || typeof templateGraph !== 'object') {
    throw new Error('Selected template has no graph payload.');
  }

  const templateName = String(templateDoc?.name ?? '').trim() || 'Template Pipeline';
  const aliasSeed = String(args.alias).trim() || String(args.device.identity?.display ?? '').trim() || 'Camera';
  const baseUploadName = `${templateName} - ${aliasSeed}`;

  const isIdentityConflict = (error: unknown): boolean => {
    if (!(error instanceof ApiError) || error.status !== 409) return false;
    const message =
      typeof error.body === 'string'
        ? error.body
        : typeof asRecord(error.body)?.error === 'string'
          ? String(asRecord(error.body)?.error)
          : '';
    const normalized = message.toLowerCase();
    return normalized.includes('identity token already in use') || normalized.includes('collide within the requested pipeline');
  };

  let createdId: string | null = null;
  const maxAttempts = 8;
  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    const uploadName = attempt === 0 ? baseUploadName : `${baseUploadName} (${attempt + 1})`;
    try {
      const created = await PipelinesApi.uploadGraph({ requestBody: { graph: templateGraph, name: uploadName } });
      createdId = String(created?.id ?? '').trim() || null;
      if (createdId) break;
    } catch (error) {
      if (isIdentityConflict(error) && attempt < maxAttempts - 1) continue;
      throw error;
    }
  }

  if (!createdId) {
    throw new Error('Failed to create pipeline from template.');
  }
  return createdId;
}

export function selectStableHardwareId(keys: unknown): string | null {
  if (!Array.isArray(keys)) return null;
  const normalized = keys.map((value) => (typeof value === 'string' ? value.trim() : '')).filter(Boolean) as string[];
  const withSlash = normalized.find((value) => value.includes('/'));
  if (withSlash) return withSlash;
  const withColon = normalized.find((value) => value.includes(':'));
  if (withColon) return withColon;
  const sorted = [...normalized].sort();
  return sorted[0] ?? null;
}

export function buildRegisterCameraManifest(args: {
  device: ProbedDevice;
  backend: ProbedBackend;
  mode: Mode;
  selectedInterval: Interval | null;
  registerExperience: RegisterExperience;
  attachedPipelineId: string | null;
  alias: string;
  encoderImpl: string | null;
  decoderImpl: string | null;
  decoderRotationDegrees: number;
  decoderMirrorHorizontal: boolean;
  hostBuffer: number;
  fpsLimit: number | null;
  encoderSettings: EncoderSettingsDraft;
  streamCapabilities: StreamCapabilitiesResponse | null;
}): StreamManifest {
  const isSimpleRegistration = args.registerExperience === 'simple';
  const capture: CaptureConfig = {
    device_keys: args.device.identity?.keys ?? [],
    backend: args.backend.kind,
    handle: args.backend.handle,
    mode: args.mode.id as ModeId,
    interval:
      !isSimpleRegistration && args.fpsLimit && args.fpsLimit > 0
        ? { numerator: 1, denominator: Math.max(1, Math.round(args.fpsLimit)) }
        : args.selectedInterval ?? args.mode.intervals?.[0] ?? null,
    controls: []
  };

  const normalizedEncoderImpl = args.encoderImpl && args.encoderImpl.trim().length ? args.encoderImpl : null;
  const normalizedDecoderImpl = args.decoderImpl && args.decoderImpl.trim().length ? args.decoderImpl.trim() : null;
  const modeWidth = Number(args.mode.format?.resolution?.width ?? 0);
  const modeHeight = Number(args.mode.format?.resolution?.height ?? 0);
  const defaultEncoderOutputResolution =
    Number.isFinite(modeWidth) && Number.isFinite(modeHeight) && modeWidth > 0 && modeHeight > 0
      ? { width: Math.max(1, Math.trunc(modeWidth / 2)), height: Math.max(1, Math.trunc(modeHeight / 2)) }
      : null;

  const backendKind = String(args.backend.kind ?? '').trim().toLowerCase();
  const isMediaBackend = backendKind === 'file' || backendKind === 'netcam';
  const streamDefaults = resolveStreamCreationDefaults(args.streamCapabilities);
  if (!streamDefaults) {
    throw new Error('Unable to determine the backend stream defaults required to build the manifest.');
  }

  const rawPipelineId = String(args.streamCapabilities?.rawPipelineId ?? '').trim();
  const rawPipelineOutput = streamDefaults.rawOutput;
  if (isMediaBackend && (!rawPipelineId || !rawPipelineOutput)) {
    throw new Error('Unable to determine raw pipeline defaults from backend capabilities.');
  }

  const shouldAttachSelectedPipeline = Boolean(args.attachedPipelineId);
  const useRawMediaPipelineInSimpleMode = isSimpleRegistration && !shouldAttachSelectedPipeline && isMediaBackend;
  const encoderSettingsWire = buildEncoderSettingsForSelection(normalizedEncoderImpl, args.encoderSettings, {
    frameRate:
      args.encoderSettings.framerateNum && args.encoderSettings.framerateDen
        ? { numerator: args.encoderSettings.framerateNum, denominator: args.encoderSettings.framerateDen }
        : null,
    defaultOutputResolution: defaultEncoderOutputResolution
  });

  return withCurrentStreamManifestSchema({
    identity: {
      id: null,
      alias: args.alias.trim().length ? args.alias.trim() : null,
      hardware_id: selectStableHardwareId(args.device.identity?.keys) ?? args.device.identity?.display?.trim?.() ?? null
    } as unknown as StreamManifest['identity'],
    capture,
    internal: false,
    pipeline_enabled: isSimpleRegistration ? shouldAttachSelectedPipeline || useRawMediaPipelineInSimpleMode : isMediaBackend,
    active_pipeline_id: isSimpleRegistration
      ? args.attachedPipelineId ?? (useRawMediaPipelineInSimpleMode ? rawPipelineId : null)
      : isMediaBackend
        ? rawPipelineId
        : null,
    active_pipeline_output: isSimpleRegistration
      ? useRawMediaPipelineInSimpleMode
        ? rawPipelineOutput
        : null
      : isMediaBackend
        ? rawPipelineOutput
        : null,
    pipelines: isSimpleRegistration
      ? args.attachedPipelineId
        ? [{ pipeline_id: args.attachedPipelineId, pipeline_graph: null, pipeline_output: null }]
        : useRawMediaPipelineInSimpleMode
          ? [{ pipeline_id: rawPipelineId, pipeline_graph: null, pipeline_output: rawPipelineOutput }]
          : []
      : isMediaBackend
        ? [{ pipeline_id: rawPipelineId, pipeline_graph: null, pipeline_output: rawPipelineOutput }]
        : [],
    pipeline_layout: isSimpleRegistration
      ? args.attachedPipelineId
        ? {
            rows: 1,
            columns: 1,
            slots: [{ row: 0, column: 0, pipeline_id: args.attachedPipelineId, output_key: null }]
          }
        : useRawMediaPipelineInSimpleMode
          ? {
              rows: 1,
              columns: 1,
              slots: [{ row: 0, column: 0, pipeline_id: rawPipelineId, output_key: rawPipelineOutput }]
            }
          : null
      : isMediaBackend
        ? {
            rows: 1,
            columns: 1,
            slots: [{ row: 0, column: 0, pipeline_id: rawPipelineId, output_key: rawPipelineOutput }]
          }
        : null,
    pipeline_wires: [],
    pipeline_host_inputs: {},
    encoder: normalizedEncoderImpl
      ? {
          state: 'enabled',
          id: normalizedEncoderImpl,
          settings: encoderSettingsWire ?? undefined
        }
      : { state: 'disabled' },
    decoder: normalizedDecoderImpl
      ? {
          state: 'enabled',
          id: normalizedDecoderImpl,
          settings: {
            fps_limit: args.fpsLimit ?? null,
            rotation_degrees: args.decoderRotationDegrees,
            mirror_horizontal: args.decoderMirrorHorizontal
          }
        }
      : { state: 'disabled' },
    host_buffer: Number.isFinite(args.hostBuffer) && args.hostBuffer > 0 ? args.hostBuffer : streamDefaults.defaultHostBuffer,
    preview_jpeg_quality: streamDefaults.defaultPreviewJpegQuality,
    recording_mode: normalizeRecordingMode(streamDefaults.defaultRecordingMode),
    start_on_boot: streamDefaults.defaultStartOnBoot
  });
}

export function sortPipelineEntries(entries: PipelineSummary[]): PipelineSummary[] {
  return [...entries].sort((a, b) => pipelineDisplayName(a).localeCompare(pipelineDisplayName(b)));
}

export function sortTemplateEntries(entries: PipelineTemplateSummary[]): PipelineTemplateSummary[] {
  return [...entries].sort((a, b) => String(a.name ?? '').localeCompare(String(b.name ?? '')));
}

export function dedupeCodecs(items: CodecInfo[], keyFor: (codec: CodecInfo) => string): CodecInfo[] {
  const seen = new SvelteSet<string>();
  const result: CodecInfo[] = [];
  for (const codec of items) {
    const key = keyFor(codec);
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(codec);
  }
  return result;
}
