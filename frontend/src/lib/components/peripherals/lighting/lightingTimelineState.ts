import {
  buildPreviewFrame,
  clampNumber,
  normalizeTimelineKeyframes,
  rgbToHex,
  scaleHex,
  type LightingColorPayload,
  type LightingFramePayload,
  type LightingTimelineEasing,
  type LightingTimelinePayload,
  type SavedLightingAnimation,
  type TimelineKeyframe
} from './lightingModalUtils';

export function makeId(): string {
  return `keyframe-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
}

export function colorPayloadToHex(
  color: LightingColorPayload | null | undefined
): string {
  if (!color) return '#000000';
  return rgbToHex({
    r: clampNumber(color.r ?? 0, 0, 255),
    g: clampNumber(color.g ?? 0, 0, 255),
    b: clampNumber(color.b ?? 0, 0, 255)
  });
}

export function timelinePayloadToKeyframes(
  timeline: LightingTimelinePayload,
  fallbackCount: number
): TimelineKeyframe[] {
  const keyframes = timeline.keyframes ?? [];
  const inferredCount = Math.max(
    fallbackCount,
    ...keyframes.map((frame) => frame.frame?.length ?? 0),
    1
  );
  return normalizeTimelineKeyframes(
    keyframes.map((frame, idx) => ({
      id: makeId(),
      name: `Keyframe ${idx + 1}`,
      time_ms: Math.max(0, Math.trunc(Number(frame.time_ms) || 0)),
      colors: Array.from({ length: inferredCount }, (_, ledIdx) =>
        colorPayloadToHex(frame.frame?.[ledIdx])
      ),
      whites: Array.from({ length: inferredCount }, (_, ledIdx) =>
        clampNumber(frame.frame?.[ledIdx]?.w ?? 0, 0, 255)
      ),
      brightnesses: Array.from({ length: inferredCount }, () => 255),
      easing: (frame.easing ?? 'linear') as LightingTimelineEasing
    }))
  );
}

export function framesPayloadToKeyframes(
  frames: LightingFramePayload[],
  fallbackCount: number,
  timelineSampleMs: number
): TimelineKeyframe[] {
  const inferredCount = Math.max(
    fallbackCount,
    ...frames.map((frame) => frame.frame?.length ?? 0),
    1
  );
  let cursor = 0;
  const keyframes = frames.map((frame, idx) => {
    const keyframe: TimelineKeyframe = {
      id: makeId(),
      name: `Keyframe ${idx + 1}`,
      time_ms: cursor,
      colors: Array.from({ length: inferredCount }, (_, ledIdx) =>
        colorPayloadToHex(frame.frame?.[ledIdx])
      ),
      whites: Array.from({ length: inferredCount }, (_, ledIdx) =>
        clampNumber(frame.frame?.[ledIdx]?.w ?? 0, 0, 255)
      ),
      brightnesses: Array.from({ length: inferredCount }, () => 255),
      easing: 'linear'
    };
    cursor += Math.max(20, Math.trunc(Number(frame.duration_ms) || timelineSampleMs || 50));
    return keyframe;
  });
  return normalizeTimelineKeyframes(keyframes);
}

export function isLitColor(color: LightingColorPayload | null | undefined): boolean {
  if (!color) return false;
  return (
    (Number(color.r) || 0) +
      (Number(color.g) || 0) +
      (Number(color.b) || 0) +
      (Number(color.w) || 0) >
    0
  );
}

export function normalizeFramesForEditor(
  frames: LightingFramePayload[],
  fallbackCount: number
): LightingFramePayload[] {
  const inferredCount = Math.max(
    fallbackCount,
    ...frames.map((frame) => frame.frame?.length ?? 0),
    1
  );
  const padded = frames.map((frame) => ({
    duration_ms: Math.max(20, Math.trunc(Number(frame.duration_ms) || 120)),
    frame: Array.from(
      { length: inferredCount },
      (_, idx) => frame.frame?.[idx] ?? { r: 0, g: 0, b: 0, w: 0 }
    )
  }));
  if (padded.length < 2) return padded;

  const litCounts = padded.map((item) =>
    item.frame.reduce((sum, color) => sum + (isLitColor(color) ? 1 : 0), 0)
  );
  const monotonicProgress =
    litCounts.every((value, idx) => idx === 0 || value >= litCounts[idx - 1]) &&
    litCounts.some((value, idx) => idx > 0 && value > litCounts[idx - 1]);
  const finalLit = litCounts[litCounts.length - 1] ?? 0;
  if (!monotonicProgress || finalLit >= inferredCount) {
    return padded;
  }

  const lastFrame = padded[padded.length - 1];
  const highlight =
    lastFrame.frame.find((color) => isLitColor(color)) ??
    ({ r: 255, g: 255, b: 255, w: 0 } as LightingColorPayload);
  const out = [...padded];
  let workingFrame = [...lastFrame.frame];
  for (let idx = finalLit; idx < inferredCount; idx += 1) {
    workingFrame = [...workingFrame];
    workingFrame[idx] = { ...highlight };
    out.push({
      duration_ms: lastFrame.duration_ms,
      frame: [...workingFrame]
    });
  }
  return out;
}

export function applyTimelineEasing(
  easing: LightingTimelineEasing | undefined,
  t: number
): number {
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

export function lerpChannel(a: number, b: number, t: number): number {
  return clampNumber(Math.round(a + (b - a) * t), 0, 255);
}

export function frameFromKeyframe(
  keyframe: TimelineKeyframe
): LightingColorPayload[] {
  return keyframe.colors.map((hex, idx) => {
    const scaled = scaleHex(
      hex,
      clampNumber(keyframe.brightnesses?.[idx] ?? 255, 0, 255) / 255
    );
    const match = /^#?([0-9a-f]{6})$/i.exec(scaled);
    const raw = match?.[1] ?? '000000';
    return {
      r: Number.parseInt(raw.slice(0, 2), 16),
      g: Number.parseInt(raw.slice(2, 4), 16),
      b: Number.parseInt(raw.slice(4, 6), 16),
      w: clampNumber(keyframe.whites?.[idx] ?? 0, 0, 255)
    };
  });
}

export function interpolateFrames(
  a: LightingColorPayload[],
  b: LightingColorPayload[],
  t: number
): LightingColorPayload[] {
  const count = Math.max(a.length, b.length);
  return Array.from({ length: count }, (_, idx) => {
    const left = a[idx] ?? { r: 0, g: 0, b: 0, w: 0 };
    const right = b[idx] ?? { r: 0, g: 0, b: 0, w: 0 };
    return {
      r: lerpChannel(left.r ?? 0, right.r ?? 0, t),
      g: lerpChannel(left.g ?? 0, right.g ?? 0, t),
      b: lerpChannel(left.b ?? 0, right.b ?? 0, t),
      w: lerpChannel(left.w ?? 0, right.w ?? 0, t)
    };
  });
}

export function sampleTimelineFrameAt(
  timelineKeyframes: TimelineKeyframe[],
  timeMs: number
): LightingColorPayload[] | null {
  if (timelineKeyframes.length === 0) return null;
  const sorted = normalizeTimelineKeyframes(timelineKeyframes);
  if (sorted.length === 1) {
    return frameFromKeyframe(sorted[0]);
  }
  const cursor = Math.max(0, Math.trunc(timeMs || 0));
  if (cursor <= sorted[0].time_ms) {
    return frameFromKeyframe(sorted[0]);
  }
  for (let idx = 0; idx < sorted.length - 1; idx += 1) {
    const start = sorted[idx];
    const end = sorted[idx + 1];
    if (cursor > end.time_ms) continue;
    const span = Math.max(1, end.time_ms - start.time_ms);
    const rawT = Math.max(0, Math.min(1, (cursor - start.time_ms) / span));
    const easedT = applyTimelineEasing(start.easing, rawT);
    return interpolateFrames(frameFromKeyframe(start), frameFromKeyframe(end), easedT);
  }
  return frameFromKeyframe(sorted[sorted.length - 1]);
}

export function animationPayloadToKeyframes(
  animation: NonNullable<SavedLightingAnimation['animation']>,
  ledCount: number,
  baseBrightness: number
): { keyframes: TimelineKeyframe[]; sampleMs: number; durationMs: number } {
  const count = Math.max(1, Math.trunc(ledCount || 1));
  const offColors = Array.from({ length: count }, () => '#000000');
  const offWhites = Array.from({ length: count }, () => 0);
  if (animation.kind === 'off') {
    return {
      keyframes: [
        {
          id: makeId(),
          name: 'Keyframe 1',
          time_ms: 0,
          colors: offColors,
          whites: offWhites,
          brightnesses: Array.from({ length: count }, () => 255),
          easing: 'step'
        }
      ],
      sampleMs: 120,
      durationMs: 600
    };
  }

  const animationColor = colorPayloadToHex(
    (animation as { color?: LightingColorPayload }).color
  );
  const animationWhite = clampNumber(
    (animation as { color?: LightingColorPayload }).color?.w ?? 0,
    0,
    255
  );
  const speedHz = Number.isFinite((animation as { speed_hz?: number }).speed_hz)
    ? Math.max(0.1, Number((animation as { speed_hz?: number }).speed_hz))
    : 8;
  const low = clampNumber((animation as { low?: number }).low ?? 8, 0, 255);
  const high = clampNumber((animation as { high?: number }).high ?? 255, 0, 255);
  const periodMs = Math.max(
    120,
    Math.trunc((animation as { period_ms?: number }).period_ms ?? 900)
  );

  if (animation.kind === 'pulse') {
    const colors = Array.from({ length: count }, () => animationColor);
    const whites = Array.from({ length: count }, () => animationWhite);
    return {
      keyframes: normalizeTimelineKeyframes([
        {
          id: makeId(),
          name: 'Keyframe 1',
          time_ms: 0,
          colors,
          whites,
          brightnesses: Array.from({ length: count }, () => low),
          easing: 'ease_in_out'
        },
        {
          id: makeId(),
          name: 'Keyframe 2',
          time_ms: Math.trunc(periodMs / 2),
          colors,
          whites,
          brightnesses: Array.from({ length: count }, () => high),
          easing: 'ease_in_out'
        },
        {
          id: makeId(),
          name: 'Keyframe 3',
          time_ms: periodMs,
          colors,
          whites,
          brightnesses: Array.from({ length: count }, () => low),
          easing: 'ease_in_out'
        }
      ]),
      sampleMs: Math.max(80, Math.trunc(periodMs / 8)),
      durationMs: periodMs
    };
  }

  const spanSteps = Math.max(
    1,
    Math.ceil((Math.max(low, high) - Math.min(low, high)) / 4)
  );
  const pulseCycleSteps = Math.max(2, spanSteps * 2);
  const stepDelayMs = Math.max(1, Math.floor(periodMs / 32));
  const pulseCycleMs = pulseCycleSteps * stepDelayMs;
  const rainbowCycleMs = Math.max(600, Math.round(90_000 / speedHz));
  const chaseCycleMs = Math.max(300, Math.round((count * 1000) / speedHz));
  const hueStep = Math.max(1, Math.round(speedHz * 4 * (stepDelayMs / 1000)));
  const hueCycleMs = Math.max(stepDelayMs, Math.ceil(360 / hueStep) * stepDelayMs);

  const durationMs = (() => {
    switch (animation.kind) {
      case 'chase':
        return Math.min(20_000, chaseCycleMs);
      case 'rainbow':
        return Math.min(20_000, rainbowCycleMs);
      case 'breathing_rainbow':
        return Math.min(20_000, Math.max(periodMs, pulseCycleMs, hueCycleMs));
      default:
        return 1_200;
    }
  })();

  const sampleMs = (() => {
    switch (animation.kind) {
      case 'chase':
        return clampNumber(Math.round((1000 / speedHz) / 2), 20, 240);
      case 'breathing_rainbow':
        return clampNumber(Math.round(stepDelayMs), 20, 240);
      case 'rainbow':
        return clampNumber(Math.round((1000 / speedHz) / 2), 20, 240);
      default:
        return 80;
    }
  })();

  const times = Array.from(
    { length: Math.max(2, Math.floor(durationMs / sampleMs) + 1) },
    (_, idx) => idx * sampleMs
  );
  if (times[times.length - 1] !== durationMs) {
    times.push(durationMs);
  }

  const easing: LightingTimelineEasing =
    animation.kind === 'chase'
      ? 'step'
      : animation.kind === 'breathing_rainbow'
        ? 'ease_in_out'
        : 'linear';
  const keyframes: TimelineKeyframe[] = [];
  let previousFingerprint: string | null = null;
  for (const timeMs of times) {
    const preview = buildPreviewFrame({
      ledColors: offColors,
      ledWhites: offWhites,
      liveBrightness: clampNumber(baseBrightness, 0, 255),
      showUiPreview: true,
      animationKind: animation.kind,
      animationSpeedHz: speedHz,
      animationLow: low,
      animationHigh: high,
      animationPeriodMs: periodMs,
      timeline: null,
      showTimelinePreview: false,
      previewTickMs: timeMs,
      previewStartMs: 0,
      animationColor,
      animationWhite
    });
    const colors = Array.from(
      { length: count },
      (_, idx) => preview.colors[idx] ?? '#000000'
    );
    const whites = Array.from(
      { length: count },
      (_, idx) => clampNumber(preview.whites[idx] ?? 0, 0, 255)
    );
    const brightnesses = Array.from(
      { length: count },
      () => clampNumber(preview.brightness ?? baseBrightness, 0, 255)
    );
    const fingerprint = JSON.stringify([colors, whites, brightnesses]);
    const isBoundary = timeMs === 0 || timeMs === durationMs;
    if (!isBoundary && previousFingerprint === fingerprint) {
      continue;
    }
    previousFingerprint = fingerprint;
    keyframes.push({
      id: makeId(),
      name: `Keyframe ${keyframes.length + 1}`,
      time_ms: timeMs,
      colors,
      whites,
      brightnesses,
      easing
    });
  }

  return {
    keyframes: normalizeTimelineKeyframes(keyframes),
    sampleMs,
    durationMs
  };
}

export function buildScaledColorsForOutput(
  colors: string[],
  brightnesses: number[]
): string[] {
  return colors.map((hex, idx) =>
    scaleHex(hex, clampNumber(brightnesses[idx] ?? 255, 0, 255) / 255)
  );
}
