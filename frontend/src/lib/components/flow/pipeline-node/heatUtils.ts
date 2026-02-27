import { clamp01 } from '../nodePalette';

const HEAT_COLOR_STOPS = [
  { stop: 0, r: 64, g: 196, b: 255 },
  { stop: 0.35, r: 140, g: 114, b: 255 },
  { stop: 0.6, r: 255, g: 136, b: 97 },
  { stop: 0.82, r: 255, g: 94, b: 71 },
  { stop: 1, r: 255, g: 45, b: 32 }
];

export const heatColorForIntensity = (value: number): string => {
  const clamped = clamp01(value);
  for (let index = 0; index < HEAT_COLOR_STOPS.length - 1; index += 1) {
    const current = HEAT_COLOR_STOPS[index];
    const next = HEAT_COLOR_STOPS[index + 1];
    if (clamped > next.stop) {
      continue;
    }
    const range = next.stop - current.stop;
    const progress = range <= 0 ? 0 : clamp01((clamped - current.stop) / range);
    const r = Math.round(current.r + (next.r - current.r) * progress);
    const g = Math.round(current.g + (next.g - current.g) * progress);
    const b = Math.round(current.b + (next.b - current.b) * progress);
    return `rgb(${r}, ${g}, ${b})`;
  }
  const fallback = HEAT_COLOR_STOPS[HEAT_COLOR_STOPS.length - 1];
  return `rgb(${fallback.r}, ${fallback.g}, ${fallback.b})`;
};

export const formatHeatTime = (value: number | null | undefined): string => {
  if (!Number.isFinite(value) || !value || value <= 0) {
    return '—';
  }
  if (value >= 10) {
    return `${value.toFixed(0)} ms`;
  }
  if (value >= 1) {
    return `${value.toFixed(1)} ms`;
  }
  return `${value.toFixed(2)} ms`;
};

export const formatHeatDelta = (value: number | null | undefined): string | null => {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return null;
  }
  if (value === 0) {
    return '±0 ms';
  }
  const magnitude = Math.abs(value);
  let formatted: string;
  if (magnitude >= 10) {
    formatted = `${magnitude.toFixed(0)} ms`;
  } else if (magnitude >= 1) {
    formatted = `${magnitude.toFixed(1)} ms`;
  } else {
    formatted = `${magnitude.toFixed(2)} ms`;
  }
  const sign = value > 0 ? '+' : '-';
  return `${sign}${formatted}`;
};
