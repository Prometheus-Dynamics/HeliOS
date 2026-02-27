import type {
  PipelineConnectionEmphasis,
  PipelineConnectionRoute,
  PipelineConnectionStyle
} from '$lib/types/pipeline';

export type NormalizedConnectionStyle = {
  route: PipelineConnectionRoute;
  curvature: number;
  emphasis: PipelineConnectionEmphasis;
  dashed: boolean;
};

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);

export const DEFAULT_CONNECTION_STYLE: NormalizedConnectionStyle = {
  route: 'bezier',
  curvature: 0.32,
  emphasis: 'normal',
  dashed: false
};

export const normalizeConnectionStyle = (
  style: PipelineConnectionStyle | null | undefined
): NormalizedConnectionStyle => {
  if (!style) {
    return { ...DEFAULT_CONNECTION_STYLE };
  }
  const curvatureRaw = typeof style.curvature === 'number' ? style.curvature : DEFAULT_CONNECTION_STYLE.curvature;
  const curvature = clamp(curvatureRaw, 0, 1);
  return {
    route: style.route ?? DEFAULT_CONNECTION_STYLE.route,
    curvature,
    emphasis: style.emphasis ?? DEFAULT_CONNECTION_STYLE.emphasis,
    dashed: Boolean(style.dashed)
  };
};

export const connectionStrokeWidth = (style: NormalizedConnectionStyle): number => {
  if (style.emphasis === 'bold') return 2.4;
  if (style.emphasis === 'soft') return 1.05;
  return 1.6;
};

export const isTeleportStyle = (style: NormalizedConnectionStyle): boolean =>
  style.route === 'teleport';
