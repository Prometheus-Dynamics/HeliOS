import type { LightingSettings } from '../../../../routes/settings/types';

const DEFAULT_TIMELINE_SAMPLE_MS = 50;
const MIN_TIMELINE_SAMPLE_MS = 20;

export type LightingAnimationKind = 'frame' | 'chase' | 'pulse' | 'rainbow' | 'breathing_rainbow';
export type LightingAnimationMode = LightingAnimationKind | 'timeline';
export type LightingAnimationPayloadKind = Exclude<LightingAnimationKind, 'frame'>;

export type LightingAnimationPayload =
  | { kind: 'off' }
  | { kind: 'chase'; color: LightingColorPayload; speed_hz: number }
  | { kind: 'pulse'; color: LightingColorPayload; low: number; high: number; period_ms: number }
  | { kind: 'rainbow'; speed_hz: number }
  | { kind: 'breathing_rainbow'; speed_hz: number; low: number; high: number; period_ms: number };

export type LightingFramePayload = {
  frame: LightingColorPayload[];
  duration_ms: number;
};

export type LightingColorPayload = { r: number; g: number; b: number; w?: number };
export type LightingTimelineEasing = 'step' | 'linear' | 'ease_in' | 'ease_out' | 'ease_in_out';

export type LightingTimelineKeyframePayload = {
  time_ms: number;
  frame: LightingColorPayload[];
  easing?: LightingTimelineEasing;
};

export type LightingTimelinePayload = {
  duration_ms?: number | null;
  sample_ms?: number | null;
  keyframes: LightingTimelineKeyframePayload[];
};

export type SavedLightingAnimation = {
  name: string;
  frame?: LightingColorPayload[] | null;
  frames?: LightingFramePayload[] | null;
  timeline?: LightingTimelinePayload | null;
  brightness?: number | null;
  animation?: LightingAnimationPayload | null;
  duration_ms?: number | null;
};

export type LightingRuntimeState = {
  frame?: LightingColorPayload[] | null;
  brightness?: number | null;
  animation?: LightingAnimationPayload | null;
  updated_at_ms?: number | null;
  animation_running?: boolean | null;
};

export type LightingAnimationListResponse = {
  animations: SavedLightingAnimation[];
};

export type LightingAnimationTemplateSummary = {
  template_id: string;
  name: string;
  summary?: string | null;
  tags?: string[] | null;
};

export type LightingAnimationTemplateDocument = {
  id: string;
  name: string;
  summary?: string | null;
  tags?: string[] | null;
  frame?: LightingColorPayload[] | null;
  frames?: LightingFramePayload[] | null;
  timeline?: LightingTimelinePayload | null;
  brightness?: number | null;
  animation?: LightingAnimationPayload | null;
  duration_ms?: number | null;
};

export type TimelineKeyframe = {
  id: string;
  name: string;
  time_ms: number;
  colors: string[];
  whites: number[];
  brightnesses?: number[];
  easing: LightingTimelineEasing;
};

export type PreviewFrame = {
  colors: string[];
  whites: number[];
  brightness: number;
};

export function clampNumber(value: number, min = 0, max = 255): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, Math.trunc(value)));
}

export function normalizeLighting(raw: LightingSettings | undefined, defaults: LightingSettings): LightingSettings {
  if (!raw) return { ...defaults };
  const brightness =
    typeof raw.brightness === 'number' && Number.isFinite(raw.brightness) ? raw.brightness : defaults.brightness ?? null;
  return {
    ...defaults,
    ...raw,
    count: Number.isFinite(raw.count) ? Math.trunc(raw.count) : defaults.count,
    gpio: Number.isFinite(raw.gpio) ? Math.trunc(raw.gpio) : defaults.gpio,
    frequency_hz: Number.isFinite(raw.frequency_hz) ? Math.trunc(raw.frequency_hz) : defaults.frequency_hz,
    brightness,
    label: raw.label ?? defaults.label ?? null,
    color_order: (raw.color_order ?? defaults.color_order).trim(),
    protocol: (raw.protocol ?? defaults.protocol).trim()
  };
}

export function hexToRgb(hex: string): { r: number; g: number; b: number } {
  const sanitized = hex?.startsWith('#') ? hex.slice(1) : hex;
  if (!sanitized || sanitized.length < 6) {
    return { r: 0, g: 0, b: 0 };
  }
  return {
    r: parseInt(sanitized.slice(0, 2), 16),
    g: parseInt(sanitized.slice(2, 4), 16),
    b: parseInt(sanitized.slice(4, 6), 16)
  };
}

export function rgbToHex(rgb: { r: number; g: number; b: number }): string {
  const toHex = (value: number) => clampNumber(Math.round(value), 0, 255).toString(16).padStart(2, '0');
  return `#${toHex(rgb.r)}${toHex(rgb.g)}${toHex(rgb.b)}`;
}

export function hslToHex(hue: number, saturation: number, lightness: number): string {
  const c = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1));
  const m = lightness - c / 2;
  let r = 0;
  let g = 0;
  let b = 0;
  if (hue < 60) {
    r = c;
    g = x;
  } else if (hue < 120) {
    r = x;
    g = c;
  } else if (hue < 180) {
    g = c;
    b = x;
  } else if (hue < 240) {
    g = x;
    b = c;
  } else if (hue < 300) {
    r = x;
    b = c;
  } else {
    r = c;
    b = x;
  }
  return rgbToHex({
    r: (r + m) * 255,
    g: (g + m) * 255,
    b: (b + m) * 255
  });
}

export function scaleHex(hex: string, factor: number): string {
  const rgb = hexToRgb(hex);
  const scale = Math.min(1, Math.max(0, factor));
  return rgbToHex({
    r: rgb.r * scale,
    g: rgb.g * scale,
    b: rgb.b * scale
  });
}

export function buildFramePayload(ledColors: string[], ledWhites: number[]): LightingColorPayload[] {
  return ledColors.map((hex, idx) => {
    const rgb = hexToRgb(hex);
    return { ...rgb, w: clampNumber(ledWhites[idx] ?? 0) };
  });
}

export function buildAnimationPayload(
  kind: LightingAnimationPayloadKind,
  params: {
    animationColor: string;
    animationWhite: number;
    animationSpeedHz: number;
    animationLow: number;
    animationHigh: number;
    animationPeriodMs: number;
  }
): LightingAnimationPayload {
  const baseRgb = hexToRgb(params.animationColor);
  const baseColor = { r: baseRgb.r, g: baseRgb.g, b: baseRgb.b, w: clampNumber(params.animationWhite) };
  const speedValue = Number(params.animationSpeedHz);
  const lowValue = Number(params.animationLow);
  const highValue = Number(params.animationHigh);
  const periodValue = Number(params.animationPeriodMs);
  const speed = Number.isFinite(speedValue) ? speedValue : 8;
  const low = clampNumber(Number.isFinite(lowValue) ? lowValue : 0);
  const high = clampNumber(Number.isFinite(highValue) ? highValue : 255);
  const periodMs = Number.isFinite(periodValue) ? periodValue : 900;
  switch (kind) {
    case 'chase':
      return { kind: 'chase', color: baseColor, speed_hz: speed };
    case 'pulse':
      return { kind: 'pulse', color: baseColor, low, high, period_ms: Math.max(50, Math.trunc(periodMs)) };
    case 'rainbow':
      return { kind: 'rainbow', speed_hz: speed };
    case 'breathing_rainbow':
      return {
        kind: 'breathing_rainbow',
        speed_hz: speed,
        low,
        high,
        period_ms: Math.max(100, Math.trunc(periodMs))
      };
    default:
      return { kind: 'off' };
  }
}

export function buildFramePayloadFrom(colors: string[], whites: number[]): LightingColorPayload[] {
  return colors.map((hex, idx) => {
    const rgb = hexToRgb(hex);
    return { ...rgb, w: clampNumber(whites[idx] ?? 0) };
  });
}

export function normalizeTimelineKeyframes(keyframes: TimelineKeyframe[]): TimelineKeyframe[] {
  return [...keyframes]
    .map((keyframe) => ({
      ...keyframe,
      time_ms: Math.max(0, Math.trunc(keyframe.time_ms || 0)),
      colors: [...keyframe.colors],
      whites: Array.from({ length: keyframe.colors.length }, (_, idx) => clampNumber(keyframe.whites[idx] ?? 0, 0, 255)),
      brightnesses: Array.from({ length: keyframe.colors.length }, (_, idx) => clampNumber(keyframe.brightnesses?.[idx] ?? 255, 0, 255))
    }))
    .sort((a, b) => a.time_ms - b.time_ms);
}

export function buildTimelinePayloadFromKeyframes(
  keyframes: TimelineKeyframe[],
  options?: { durationMs?: number | null; sampleMs?: number | null }
): LightingTimelinePayload {
  const sorted = normalizeTimelineKeyframes(keyframes);
  const durationMs = Number(options?.durationMs);
  const sampleMs = Number(options?.sampleMs);
  return {
    duration_ms: Number.isFinite(durationMs) ? Math.max(0, Math.trunc(durationMs)) : null,
    sample_ms: Number.isFinite(sampleMs) ? Math.max(MIN_TIMELINE_SAMPLE_MS, Math.trunc(sampleMs)) : null,
    keyframes: sorted.map((keyframe) => ({
      time_ms: keyframe.time_ms,
      frame: buildFramePayloadFrom(
        keyframe.colors.map((hex, idx) => scaleHex(hex, clampNumber(keyframe.brightnesses?.[idx] ?? 255, 0, 255) / 255)),
        keyframe.whites
      ),
      easing: keyframe.easing
    }))
  };
}

export function compileTimelineToFrameSequence(timeline: LightingTimelinePayload): LightingFramePayload[] {
  const keyframes = [...(timeline.keyframes ?? [])]
    .map((keyframe) => ({
      time_ms: Math.max(0, Math.trunc(keyframe.time_ms || 0)),
      frame: keyframe.frame ?? [],
      easing: keyframe.easing ?? 'linear'
    }))
    .sort((a, b) => a.time_ms - b.time_ms);

  if (keyframes.length === 0) {
    return [];
  }

  if (keyframes.length === 1) {
    return [
      {
        frame: keyframes[0].frame,
        duration_ms: Math.max(MIN_TIMELINE_SAMPLE_MS, Math.trunc(Number(timeline.duration_ms) || DEFAULT_TIMELINE_SAMPLE_MS))
      }
    ];
  }

  const sampleMs = Math.max(MIN_TIMELINE_SAMPLE_MS, Math.trunc(Number(timeline.sample_ms) || DEFAULT_TIMELINE_SAMPLE_MS));
  const timelineEndMs = Math.max(
    Math.trunc(Number(timeline.duration_ms) || 0),
    keyframes[keyframes.length - 1]?.time_ms ?? 0
  );

  const sequence: LightingFramePayload[] = [];

  for (let idx = 0; idx < keyframes.length - 1; idx += 1) {
    const start = keyframes[idx];
    const end = keyframes[idx + 1];
    const segmentDuration = Math.max(sampleMs, end.time_ms - start.time_ms);
    let elapsed = 0;
    while (elapsed < segmentDuration) {
      const rawT = Math.max(0, Math.min(1, elapsed / Math.max(1, segmentDuration)));
      const easedT = applyEasing(start.easing, rawT);
      const frame = lerpFrame(start.frame, end.frame, easedT);
      const remaining = segmentDuration - elapsed;
      const duration = Math.max(MIN_TIMELINE_SAMPLE_MS, Math.min(sampleMs, remaining));
      sequence.push({ frame, duration_ms: duration });
      elapsed += duration;
    }
  }

  const last = keyframes[keyframes.length - 1];
  const holdMs = Math.max(MIN_TIMELINE_SAMPLE_MS, timelineEndMs > last.time_ms ? timelineEndMs - last.time_ms : sampleMs);
  sequence.push({ frame: last.frame, duration_ms: holdMs });

  return sequence;
}

function applyEasing(easing: LightingTimelineEasing | undefined, t: number): number {
  const value = Math.max(0, Math.min(1, t));
  switch (easing ?? 'linear') {
    case 'step':
      return value >= 1 ? 1 : 0;
    case 'ease_in':
      return value * value;
    case 'ease_out':
      return 1 - (1 - value) * (1 - value);
    case 'ease_in_out':
      return value < 0.5 ? 2 * value * value : 1 - ((-2 * value + 2) ** 2) / 2;
    default:
      return value;
  }
}

function lerpFrame(start: LightingColorPayload[], end: LightingColorPayload[], t: number): LightingColorPayload[] {
  const count = Math.max(start.length, end.length);
  const out: LightingColorPayload[] = [];
  for (let idx = 0; idx < count; idx += 1) {
    const a = start[idx] ?? { r: 0, g: 0, b: 0, w: 0 };
    const b = end[idx] ?? { r: 0, g: 0, b: 0, w: 0 };
    out.push({
      r: lerpChannel(a.r, b.r, t),
      g: lerpChannel(a.g, b.g, t),
      b: lerpChannel(a.b, b.b, t),
      w: lerpChannel(a.w ?? 0, b.w ?? 0, t)
    });
  }
  return out;
}

function lerpChannel(start: number, end: number, t: number): number {
  return clampNumber(Math.round(start + (end - start) * t), 0, 255);
}

function framePayloadToPreview(frame: LightingColorPayload[], count: number): { colors: string[]; whites: number[] } {
  const colors = Array.from({ length: count }, (_, idx) => {
    const color = frame[idx] ?? { r: 0, g: 0, b: 0, w: 0 };
    return rgbToHex({ r: color.r, g: color.g, b: color.b });
  });
  const whites = Array.from({ length: count }, (_, idx) => clampNumber(frame[idx]?.w ?? 0, 0, 255));
  return { colors, whites };
}

function resolveTimelineDurationMs(timeline: LightingTimelinePayload): number {
  const lastKeyframeTime = timeline.keyframes.reduce((max, keyframe) => Math.max(max, Math.trunc(Number(keyframe.time_ms) || 0)), 0);
  const explicitDuration = Math.trunc(Number(timeline.duration_ms) || 0);
  return Math.max(lastKeyframeTime, explicitDuration);
}

function previewTimelineFrame(
  timeline: LightingTimelinePayload,
  elapsedMs: number,
  count: number,
  fallback: PreviewFrame
): PreviewFrame {
  const keyframes = [...(timeline.keyframes ?? [])]
    .map((keyframe) => ({
      ...keyframe,
      time_ms: Math.max(0, Math.trunc(Number(keyframe.time_ms) || 0)),
      easing: keyframe.easing ?? 'linear'
    }))
    .sort((a, b) => a.time_ms - b.time_ms);

  if (keyframes.length === 0) {
    return fallback;
  }

  if (keyframes.length === 1) {
    const preview = framePayloadToPreview(keyframes[0].frame ?? [], count);
    return { ...preview, brightness: fallback.brightness };
  }

  const durationMs = resolveTimelineDurationMs(timeline);
  const cursor = durationMs > 0 ? elapsedMs % durationMs : elapsedMs;

  if (cursor <= keyframes[0].time_ms) {
    const preview = framePayloadToPreview(keyframes[0].frame ?? [], count);
    return { ...preview, brightness: fallback.brightness };
  }

  for (let idx = 0; idx < keyframes.length - 1; idx += 1) {
    const start = keyframes[idx];
    const end = keyframes[idx + 1];
    if (cursor > end.time_ms) {
      continue;
    }
    const segmentDuration = Math.max(1, end.time_ms - start.time_ms);
    const rawT = Math.max(0, Math.min(1, (cursor - start.time_ms) / segmentDuration));
    const easedT = applyEasing(start.easing, rawT);
    const frame = lerpFrame(start.frame ?? [], end.frame ?? [], easedT);
    const preview = framePayloadToPreview(frame, count);
    return { ...preview, brightness: fallback.brightness };
  }

  const finalPreview = framePayloadToPreview(keyframes[keyframes.length - 1].frame ?? [], count);
  return { ...finalPreview, brightness: fallback.brightness };
}

export function buildPreviewFrame(params: {
  ledColors: string[];
  ledWhites: number[];
  liveBrightness: number;
  showUiPreview: boolean;
  animationKind: LightingAnimationMode;
  animationSpeedHz: number;
  animationLow: number;
  animationHigh: number;
  animationPeriodMs: number;
  timeline: LightingTimelinePayload | null;
  showTimelinePreview: boolean;
  previewTickMs: number;
  previewStartMs: number;
  animationColor: string;
  animationWhite: number;
}): PreviewFrame {
  const count = Math.max(1, params.ledColors.length);
  const baseBrightness = clampNumber(params.liveBrightness, 0, 255);
  const baseFrame: PreviewFrame = { colors: params.ledColors, whites: params.ledWhites, brightness: baseBrightness };
  const animationActive = params.showUiPreview && params.animationKind !== 'frame';
  if (!animationActive) {
    return baseFrame;
  }

  const elapsedMs = Math.max(0, params.previewTickMs - params.previewStartMs);
  const speed = Number.isFinite(Number(params.animationSpeedHz)) ? Math.max(0.1, Number(params.animationSpeedHz)) : 8;
  const low = clampNumber(Number(params.animationLow), 0, 255);
  const high = clampNumber(Number(params.animationHigh), 0, 255);
  const period = Math.max(120, Number(params.animationPeriodMs) || 900);
  const stepDelayMs = Math.max(1, Math.floor(period / 32));
  const stepIndex = Math.floor(elapsedMs / stepDelayMs);

  const pulseLevel = (() => {
    const step = 4;
    const min = Math.min(low, high);
    const max = Math.max(low, high);
    const spanSteps = Math.max(1, Math.ceil((max - min) / step));
    const cycle = spanSteps * 2;
    const cycleIndex = stepIndex % cycle;
    const forward = cycleIndex < spanSteps;
    const offset = forward ? cycleIndex : cycle - cycleIndex;
    return clampNumber(min + offset * step, 0, 255);
  })();

  if (params.animationKind === 'timeline') {
    if (!params.showTimelinePreview || !params.timeline || params.timeline.keyframes.length === 0) {
      return baseFrame;
    }
    return previewTimelineFrame(params.timeline, elapsedMs, count, baseFrame);
  }

  if (params.animationKind === 'chase') {
    const idx = Math.floor((elapsedMs / 1000) * speed) % count;
    const colors = Array.from({ length: count }, () => '#09040f');
    const whites = Array.from({ length: count }, () => 0);
    colors[idx] = params.animationColor;
    whites[idx] = clampNumber(params.animationWhite, 0, 255);
    return { colors, whites, brightness: baseBrightness };
  }

  if (params.animationKind === 'pulse') {
    const colors = Array.from({ length: count }, () => params.animationColor);
    const whites = Array.from({ length: count }, () => clampNumber(params.animationWhite, 0, 255));
    return { colors, whites, brightness: pulseLevel };
  }

  if (params.animationKind === 'breathing_rainbow') {
    const stepSecs = stepDelayMs / 1000;
    const hueStep = Math.max(1, Math.round(speed * 4 * stepSecs));
    const frameIndex = stepIndex * hueStep;
    const colors = Array.from({ length: count }, (_, index) => {
      const hue = (frameIndex + index * 6) % 360;
      return hslToHex(hue, 0.85, 0.55);
    });
    return { colors, whites: Array.from({ length: count }, () => 0), brightness: pulseLevel };
  }

  if (params.animationKind === 'rainbow') {
    const frameIndex = Math.floor((elapsedMs / 1000) * speed) * 4;
    const colors = Array.from({ length: count }, (_, index) => {
      const hue = (frameIndex + index * 6) % 360;
      return hslToHex(hue, 0.85, 0.55);
    });
    return { colors, whites: Array.from({ length: count }, () => 0), brightness: baseBrightness };
  }

  return baseFrame;
}
