import { makeUiId } from '$lib/components/pipelines/overrides/pipelineUiLayoutUtils';

export type GradientStop = {
  id: string;
  color: string;
  position: number;
};

export const DEFAULT_GRADIENT = 'linear-gradient(90deg, #0f172a 0%, #e2e8f0 100%)';

export function parseGradientStops(value?: string | null): { angle: number; stops: GradientStop[] } {
  if (!value) {
    return {
      angle: 90,
      stops: [
        { id: makeUiId('stop'), color: '#0f172a', position: 0 },
        { id: makeUiId('stop'), color: '#e2e8f0', position: 100 }
      ]
    };
  }
  const match = value.match(/linear-gradient\((.+)\)/i);
  if (!match) {
    return {
      angle: 90,
      stops: [
        { id: makeUiId('stop'), color: '#0f172a', position: 0 },
        { id: makeUiId('stop'), color: '#e2e8f0', position: 100 }
      ]
    };
  }
  const rawParts = match[1].split(',').map((part) => part.trim());
  let angle = 90;
  if (rawParts[0]?.includes('deg')) {
    const parsed = Number.parseFloat(rawParts[0]);
    if (Number.isFinite(parsed)) {
      angle = parsed;
    }
    rawParts.shift();
  }
  const stopParts = rawParts.filter(Boolean);
  const stops: GradientStop[] = stopParts.map((part, index) => {
    const tokens = part.split(/\s+/);
    const color = tokens[0] && tokens[0].startsWith('#') ? tokens[0] : '#ffffff';
    const posToken = tokens.find((token) => token.endsWith('%'));
    const position = posToken ? Number.parseFloat(posToken) : Number.NaN;
    return {
      id: makeUiId(`stop_${index}`),
      color,
      position: Number.isFinite(position) ? position : Number.NaN
    };
  });
  const definedStops = stops.filter((stop) => Number.isFinite(stop.position));
  if (definedStops.length !== stops.length) {
    const fallbackCount = stops.length || 2;
    stops.forEach((stop, index) => {
      if (!Number.isFinite(stop.position)) {
        stop.position = fallbackCount === 1 ? 0 : (index / (fallbackCount - 1)) * 100;
      }
    });
  }
  return {
    angle,
    stops: stops.map((stop) => ({
      ...stop,
      position: Math.min(100, Math.max(0, stop.position))
    }))
  };
}

export function buildGradientString(angle: number, stops: GradientStop[]): string {
  const ordered = [...stops].sort((a, b) => a.position - b.position);
  const stopText = ordered.map((stop) => `${stop.color} ${stop.position.toFixed(1)}%`).join(', ');
  return `linear-gradient(${angle}deg, ${stopText})`;
}

export function resolveGradientPreview(value?: string | null): string {
  return value && value.trim().length ? value : DEFAULT_GRADIENT;
}

export function clampGradientPosition(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, value));
}
