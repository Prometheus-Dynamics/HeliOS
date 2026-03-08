import type {
  PipelineDataType,
  PipelineGraphNode,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import type { ApiGraphNode } from '$lib/types/pipeline-api';
import type {
  ActiveConnection,
  EnumSelectionHandler,
  BooleanToggleHandler,
  BooleanConstantHandler,
  PixelConstantToggleHandler,
  PixelColorChangeHandler,
  NumericConstantToggleHandler,
  NumericValueChangeHandler,
  PipelineNodeDiagnostics,
  PipelineNodeHeatmapPayload
} from '../pipeline-graph/types';

export type PortInteractionHandler = (payload: {
  direction: 'input' | 'output';
  port: string;
  event: MouseEvent;
}) => void;

export type EnumVariantOption = {
  raw: string;
  label: string;
  value: string;
};

export interface PipelineNodeData extends Record<string, unknown> {
  node: PipelineGraphNode;
  apiNode: ApiGraphNode | null;
  registryEntry: PipelineRegistryEntry | null;
  inputOrder: string[];
  outputOrder: string[];
  activeConnection: ActiveConnection | null;
  searchActive?: boolean;
  searchMatch?: boolean;
  searchTokens?: string[];
  gpuSegment?: number | null;
  gpuPeers?: number | null;
  heatmap?: PipelineNodeHeatmapPayload | null;
  heatmapMode?: boolean;
  onPortDoubleClick?: PortInteractionHandler;
  onPortClear?: (payload: { nodeId: string; port: string }) => void;
  onPortContextMenu?: PortInteractionHandler;
  onPortEnumChange?: EnumSelectionHandler;
  onPortBooleanConstantToggle?: BooleanConstantHandler;
  onPortBooleanToggle?: BooleanToggleHandler;
  onPortPixelConstantToggle?: PixelConstantToggleHandler;
  onPortPixelColorChange?: PixelColorChangeHandler;
  onPortNumericConstantToggle?: NumericConstantToggleHandler;
  onPortNumericValueChange?: NumericValueChangeHandler;
  diagnostics?: PipelineNodeDiagnostics | null;
  highlight?: { port: string | null; token: number | null } | null;
  syncOverlay?: { enabled: boolean; focus: boolean } | null;
  runtimeWarning?: { message: string; at: number | null } | null;
}

export type PortSyncState =
  | { role: 'group'; groupId: string; color: string }
  | { role: 'unsynced'; groupId: null; color: null }
  | null;

export type PortRenderInfo = {
  name: string;
  handleId: string;
  type: PipelineDataType;
  label: string | null;
  detail: string | null;
  hasConstant: boolean;
  isSearchMatch: boolean;
  constantDisplay: string | null;
  constantTooltip: string | null;
  constantRaw: string | null;
  issueMessages?: string[] | null;
  color: string;
  hasCustomColor: boolean;
  kind: string | null;
  variants: EnumVariantOption[];
  dataTypeKey: string | null;
  isBoolean: boolean;
  booleanValue: boolean;
  settable: boolean;
  isPixel: boolean;
  pixelHex: string | null;
  pixelAlpha: number | null;
  isEnum: boolean;
  isNumeric: boolean;
  numericValue: number | null;
  numericDisplay: string | null;
  numericStep: string;
  numericMin: number | null;
  numericMax: number | null;
  numericHasSlider: boolean;
  numericSliderStep: string;
  defaultDisplay: string | null;
  defaultTooltip: string | null;
  hasIssue: boolean;
  isHighlighted: boolean;
  syncState: PortSyncState;
};

export type BasePortRenderInfo = Omit<
  PortRenderInfo,
  'hasIssue' | 'isHighlighted' | 'syncState' | 'isSearchMatch'
>;

export type PortInteractionHandlers = {
  handlePortDoubleClick: (
    direction: 'input' | 'output',
    port: PortRenderInfo,
    event: MouseEvent
  ) => void;
  handlePortContextMenu: (
    direction: 'input' | 'output',
    port: PortRenderInfo,
    event: MouseEvent
  ) => void;
  handleEnumSelect: (port: string, event: Event) => void;
  handleBooleanToggle: (port: string, event: Event) => void;
  handleNumericInput: (port: PortRenderInfo, event: Event) => void;
  handleNumericBlur: (port: PortRenderInfo, event: FocusEvent) => void;
  handleNumericBeforeInput: (port: PortRenderInfo, event: InputEvent) => void;
  handlePixelPreviewClick: (port: PortRenderInfo, event: MouseEvent) => void;
  handlePixelColorInput: (port: PortRenderInfo, event: Event) => void;
};

export type PortSyncAssignments = Map<string, { groupId: string; color: string }>;
