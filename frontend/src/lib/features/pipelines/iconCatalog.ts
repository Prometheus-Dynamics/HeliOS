import type { PipelineAppearance } from '$lib/types/pipeline';
import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
import {
  faAtom,
  faBolt,
  faBrain,
  faBullseye,
  faCamera,
  faCloud,
  faCodeBranch,
  faCogs,
  faCubes,
  faDiagramProject,
  faGlobe,
  faMicrochip,
  faShareNodes,
  faSatelliteDish,
  faSitemap,
  faWaveSquare
} from '@fortawesome/free-solid-svg-icons';

type PipelineIconOption = { id: string; label: string; icon: IconDefinition };

export const PIPELINE_ICON_OPTIONS: PipelineIconOption[] = [
  { id: 'diagram', label: 'Diagram', icon: faDiagramProject },
  { id: 'graph', label: 'Graph', icon: faSitemap },
  { id: 'capture', label: 'Capture', icon: faCamera },
  { id: 'compute', label: 'Compute', icon: faMicrochip },
  { id: 'signal', label: 'Signal', icon: faWaveSquare },
  { id: 'nodes', label: 'Nodes', icon: faCubes },
  { id: 'uplink', label: 'Uplink', icon: faSatelliteDish },
  { id: 'burst', label: 'Burst', icon: faBolt },
  { id: 'atom', label: 'Atom', icon: faAtom },
  { id: 'brain', label: 'Brain', icon: faBrain },
  { id: 'target', label: 'Target', icon: faBullseye },
  { id: 'branch', label: 'Branch', icon: faCodeBranch },
  { id: 'cogs', label: 'Cogs', icon: faCogs },
  { id: 'cloud', label: 'Cloud', icon: faCloud },
  { id: 'globe', label: 'Globe', icon: faGlobe },
  { id: 'share', label: 'Share', icon: faShareNodes }
];

const PIPELINE_ICON_OPTION_MAP = Object.fromEntries(
  PIPELINE_ICON_OPTIONS.map((option) => [option.id, option])
) as Record<string, PipelineIconOption>;

export const PIPELINE_ICON_COLORS = [
  '#0f172a',
  '#134e4a',
  '#065f46',
  '#1d4ed8',
  '#3730a3',
  '#6d28d9',
  '#7c2d12',
  '#92400e',
  '#991b1b',
  '#be185d'
];

export const DEFAULT_PIPELINE_ICON_ID = PIPELINE_ICON_OPTIONS[0].id;
export const DEFAULT_PIPELINE_COLOR = PIPELINE_ICON_COLORS[0];

const hashString = (value: string): number => {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(index);
    hash |= 0;
  }
  return Math.abs(hash);
};

export function defaultIconIdForPipeline(pipelineId: string, revision?: string | null): string {
  const versionSeed = revision?.trim() ? `${pipelineId}@${revision.trim()}` : pipelineId;
  if (!versionSeed) return DEFAULT_PIPELINE_ICON_ID;
  const hash = hashString(versionSeed);
  return PIPELINE_ICON_OPTIONS[hash % PIPELINE_ICON_OPTIONS.length]?.id ?? DEFAULT_PIPELINE_ICON_ID;
}

export function defaultColorForPipeline(pipelineId: string, revision?: string | null): string {
  const versionSeed = revision?.trim() ? `${pipelineId}@${revision.trim()}` : pipelineId;
  if (!versionSeed) return DEFAULT_PIPELINE_COLOR;
  const hash = hashString(`${versionSeed}:color`);
  return PIPELINE_ICON_COLORS[hash % PIPELINE_ICON_COLORS.length] ?? DEFAULT_PIPELINE_COLOR;
}

export function resolvePipelineIconOption(id: string) {
  return PIPELINE_ICON_OPTION_MAP[id] ?? PIPELINE_ICON_OPTION_MAP[DEFAULT_PIPELINE_ICON_ID];
}

export function pipelineIconConfig(pipelineId: string, appearance?: PipelineAppearance | null, revision?: string | null) {
  const iconId = appearance?.icon ?? defaultIconIdForPipeline(pipelineId, revision);
  const color = appearance?.color ?? defaultColorForPipeline(pipelineId, revision);
  const option = resolvePipelineIconOption(iconId);
  return { iconId: option.id, color, option };
}
