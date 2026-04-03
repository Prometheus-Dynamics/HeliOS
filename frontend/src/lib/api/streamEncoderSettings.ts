import type { CodecInfo, EncoderSettings, FrameRate, ResolutionHint } from '$lib/api/client';
import {
  STREAM_ENCODER_FAMILIES,
  STREAM_ENCODER_FAMILY_IDS,
  type StreamEncoderFamily,
  type StreamRecordingCodecId
} from '$lib/contracts/codecFamilies';

type CodecLike = {
  implementation?: string | null;
  name?: string | null;
  fourcc?: string | null;
  input?: string | null;
  output?: string | null;
  kind?: string | null;
};

export type EncoderSettingsDraft = {
  quality: number | null;
  bitrate: number | null;
  gop: number | null;
  framerateNum: number | null;
  framerateDen: number | null;
  threadCount: number | null;
  outWidth: number | null;
  outHeight: number | null;
};

export type EncoderSettingsKind = EncoderSettings['kind'];

function normalizeCodecToken(value: string | null | undefined): string {
  return String(value ?? '').trim().toLowerCase();
}

function normalizeFourccToken(value: string | null | undefined): string {
  const trimmed = String(value ?? '').trim();
  return trimmed ? trimmed.split(/\s+/)[0]?.toUpperCase() ?? '' : '';
}

function encoderFamilyForKind(kind: EncoderSettingsKind | null | undefined): StreamEncoderFamily | null {
  if (!kind) return null;
  return STREAM_ENCODER_FAMILIES.find((family) => family.settingsKind === kind) ?? null;
}

function encoderFamilyForSelection(value: string | null | undefined): StreamEncoderFamily | null {
  const normalized = normalizeCodecToken(value);
  if (!normalized) return null;

  const implementationMatches = STREAM_ENCODER_FAMILIES.filter((family) =>
    family.runtimeImplementationAliases.some((alias) => normalizeCodecToken(alias) === normalized)
  );
  if (implementationMatches.length === 1 && normalized !== 'ffmpeg') {
    return implementationMatches[0] ?? null;
  }

  return (
    STREAM_ENCODER_FAMILIES.find(
      (family) =>
        normalizeCodecToken(family.selectorId) === normalized ||
        family.selectorAliases.some((alias) => normalizeCodecToken(alias) === normalized)
    ) ?? null
  );
}

function encoderFamilyForCodec(codec: CodecLike | null | undefined): StreamEncoderFamily | null {
  if (!codec) return null;

  const implementation = normalizeCodecToken(codec.implementation);
  if (implementation) {
    const exact = STREAM_ENCODER_FAMILIES.filter((family) =>
      family.runtimeImplementationAliases.some((alias) => normalizeCodecToken(alias) === implementation)
    );
    if (exact.length === 1 && implementation !== 'ffmpeg') {
      return exact[0] ?? null;
    }
    if (implementation === 'ffmpeg') {
      const outputTokens = [codec.output, codec.fourcc, codec.input].map((value) => normalizeFourccToken(value)).filter(Boolean);
      const nameToken = normalizeCodecToken(codec.name);
      const match =
        exact.find(
          (family) =>
            family.outputFourccAliases.some((alias) => outputTokens.includes(normalizeFourccToken(alias))) ||
            family.runtimeNameAliases.some((alias) => normalizeCodecToken(alias) === nameToken)
        ) ?? null;
      if (match) return match;
    }
  }

  for (const candidate of [codec.name, codec.fourcc, codec.output, codec.input]) {
    const match = encoderFamilyForSelection(candidate);
    if (match) return match;
  }
  return null;
}

export function createEncoderSettingsDraft(): EncoderSettingsDraft {
  return {
    quality: null,
    bitrate: null,
    gop: null,
    framerateNum: null,
    framerateDen: null,
    threadCount: null,
    outWidth: null,
    outHeight: null
  };
}

export function encoderSettingsKindForSelection(value: string | null | undefined): EncoderSettingsKind | null {
  return encoderFamilyForSelection(value)?.settingsKind ?? null;
}

export function encoderSelectorForKind(kind: EncoderSettingsKind | null | undefined): string | null {
  return encoderFamilyForKind(kind)?.selectorId ?? null;
}

export function encoderSettingsKindForCodec(codec: CodecLike | null | undefined): EncoderSettingsKind | null {
  return encoderFamilyForCodec(codec)?.settingsKind ?? null;
}

export function encoderSelectionId(codec: CodecLike | null | undefined): string | null {
  const implementation = String(codec?.implementation ?? '').trim();
  if (implementation) return implementation;
  const typed = encoderFamilyForCodec(codec)?.selectorId ?? null;
  if (typed) return typed;
  const name = String(codec?.name ?? '').trim();
  return name || null;
}

export function encoderSettingsSupportsQuality(kind: EncoderSettingsKind | null | undefined): boolean {
  const family = encoderFamilyForKind(kind);
  return family?.id === STREAM_ENCODER_FAMILY_IDS.TURBOJPEG || family?.id === STREAM_ENCODER_FAMILY_IDS.MOZJPEG;
}

export function encoderSettingsSupportsVideoControls(kind: EncoderSettingsKind | null | undefined): boolean {
  const family = encoderFamilyForKind(kind);
  return (
    family?.id === STREAM_ENCODER_FAMILY_IDS.FFMPEG_MJPEG ||
    family?.id === STREAM_ENCODER_FAMILY_IDS.H264 ||
    family?.id === STREAM_ENCODER_FAMILY_IDS.H265
  );
}

export function encoderRecordingCodecForSelection(value: string | null | undefined): StreamRecordingCodecId | null {
  return encoderFamilyForSelection(value)?.recordingCodec ?? null;
}

export function encoderSettingsSummary(settings: EncoderSettings | null | undefined): string | null {
  if (!settings) return null;
  if (settings.kind === 'turbojpeg' || settings.kind === 'mozjpeg') {
    return `Defaults: quality ${positiveIntOrNull(settings.quality) ?? 'auto'}`;
  }
  return `Defaults: bitrate ${positiveIntOrNull(settings.bitrate) ?? 'auto'} · threads ${positiveIntOrNull(settings.thread_count) ?? 'auto'}`;
}

export function encoderSettingsDraftFromWire(settings: EncoderSettings | null | undefined): EncoderSettingsDraft {
  const draft = createEncoderSettingsDraft();
  if (!settings) return draft;

  switch (settings.kind) {
    case 'turbojpeg':
    case 'mozjpeg':
      draft.quality = boundedIntOrNull(settings.quality, 1, 100);
      return draft;
    case 'ffmpeg_mjpeg':
    case 'h264':
    case 'h265':
      draft.bitrate = positiveIntOrNull(settings.bitrate);
      draft.gop = positiveIntOrNull(settings.gop);
      draft.framerateNum = positiveIntOrNull(settings.framerate?.numerator);
      draft.framerateDen = positiveIntOrNull(settings.framerate?.denominator);
      draft.threadCount = positiveIntOrNull(settings.thread_count);
      draft.outWidth = positiveIntOrNull(settings.output_resolution?.width);
      draft.outHeight = positiveIntOrNull(settings.output_resolution?.height);
      return draft;
  }
}

export function encoderSettingsDraftFromUnknown(value: unknown): EncoderSettingsDraft {
  const record = asRecord(value);
  if (!record) return createEncoderSettingsDraft();
  const kind = encoderSettingsKindForSelection(record.kind as string | undefined);
  if (kind === 'turbojpeg' || kind === 'mozjpeg') {
    return {
      ...createEncoderSettingsDraft(),
      quality: boundedIntOrNull(record.quality, 1, 100)
    };
  }
  return {
    quality: null,
    bitrate: positiveIntOrNull(record.bitrate),
    gop: positiveIntOrNull(record.gop),
    framerateNum: positiveIntOrNull(asRecord(record.framerate)?.numerator),
    framerateDen: positiveIntOrNull(asRecord(record.framerate)?.denominator),
    threadCount: positiveIntOrNull(record.thread_count),
    outWidth: positiveIntOrNull(asRecord(record.output_resolution)?.width),
    outHeight: positiveIntOrNull(asRecord(record.output_resolution)?.height)
  };
}

export function buildEncoderSettingsForSelection(
  selection: string | null | undefined,
  draft: EncoderSettingsDraft,
  options: {
    frameRate?: FrameRate | null;
    defaultOutputResolution?: ResolutionHint | null;
  } = {}
): EncoderSettings | null {
  const kind = encoderSettingsKindForSelection(selection);
  if (!kind) return null;

  if (encoderSettingsSupportsQuality(kind)) {
    return {
      kind,
      quality: boundedIntOrNull(draft.quality, 1, 100)
    };
  }

  const framerate = options.frameRate ?? frameRateFromDraft(draft);
  const outputResolution = resolutionHintFromDraft(draft) ?? options.defaultOutputResolution ?? null;

  return {
    kind,
    bitrate: positiveIntOrNull(draft.bitrate),
    gop: positiveIntOrNull(draft.gop),
    framerate,
    thread_count: positiveIntOrNull(draft.threadCount),
    output_resolution: outputResolution
  };
}

function frameRateFromDraft(draft: EncoderSettingsDraft): FrameRate | null {
  const numerator = positiveIntOrNull(draft.framerateNum);
  const denominator = positiveIntOrNull(draft.framerateDen);
  return numerator && denominator ? { numerator, denominator } : null;
}

function resolutionHintFromDraft(draft: EncoderSettingsDraft): ResolutionHint | null {
  const width = positiveIntOrNull(draft.outWidth);
  const height = positiveIntOrNull(draft.outHeight);
  return width && height ? { width, height } : null;
}

function positiveIntOrNull(value: unknown): number | null {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return null;
  return Math.trunc(numeric);
}

function boundedIntOrNull(value: unknown, min: number, max: number): number | null {
  const numeric = positiveIntOrNull(value);
  if (numeric == null) return null;
  return numeric >= min && numeric <= max ? numeric : null;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" ? (value as Record<string, unknown>) : null;
}
