import type {
  PipelineDataType,
  PipelineNodeValue
} from '$lib/types/pipeline';
import type { HandleType, XYPosition } from '@xyflow/system';
import type { NormalizedConnectionStyle } from './edgeStyle';

export type PipelineEdgeData = {
  edgeId: string;
  fromPort: string;
  toPort: string;
  fromType: PipelineDataType | undefined;
  toType: PipelineDataType | undefined;
  fromColor?: string | null;
  style: NormalizedConnectionStyle;
  fromNodeName?: string;
  toNodeName?: string;
};

export type EdgeSelection = {
  id: string;
  from: { node: string; port: string; dataType: PipelineDataType | undefined };
  to: { node: string; port: string; dataType: PipelineDataType | undefined };
};

export type ActiveConnection = {
  handleType: HandleType;
  typeKey: string | null;
};

export type EnumSelectionHandler = (payload: { nodeId: string; port: string; value: string | null }) => void;

export type BooleanToggleHandler = (payload: { nodeId: string; port: string; value: boolean }) => void;

export type PixelConstantToggleHandler = (payload: { nodeId: string; port: string; enabled: boolean }) => void;

export type PixelColorChangeHandler = (payload: { nodeId: string; port: string; hex: string }) => void;

export type BooleanConstantHandler = (payload: { nodeId: string; port: string; enabled: boolean }) => void;

export type NumericConstantToggleHandler = (payload: { nodeId: string; port: string; enabled: boolean }) => void;

export type NumericValueChangeHandler = (payload: { nodeId: string; port: string; value: string }) => void;

export type NodePortEditorState = {
  mode: 'node';
  nodeId: string;
  port: string;
  dataType: PipelineDataType | undefined;
  dataTypeKey: string | null;
  existingValue: PipelineNodeValue | null;
  settable: boolean;
  anchor: XYPosition;
  variants: string[];
};

export type PipelineInputEditorState = {
  mode: 'pipeline-input';
  port: string;
  dataType: PipelineDataType | undefined;
  dataTypeKey: string | null;
  existingValue: PipelineNodeValue | null;
  settable: boolean;
  anchor: XYPosition;
  variants: string[];
};

export type PortEditorState = NodePortEditorState | PipelineInputEditorState;

export type PipelineGraphHeatmapNode = {
  totalTimeMs: number;
  peakTimeMs: number;
  averageFps: number | null;
  sampleCount: number;
  streamCount: number;
};

export type PipelineGraphHeatmap = {
  enabled: boolean;
  maxValue: number;
  minValue: number;
  nodes: Record<string, PipelineGraphHeatmapNode>;
};

export type PipelineNodeHeatmapPayload = PipelineGraphHeatmapNode & {
  normalized: number;
  globalAverageTimeMs: number | null;
  deltaFromAverageMs: number | null;
  deltaFromAveragePercent: number | null;
};

export type PipelineNodeDiagnostics = {
  nodeMessages: string[];
  hasNodeIssue: boolean;
  inputPorts: string[];
  outputPorts: string[];
  portMessages?: Record<string, string[]>;
};

export type PipelineGraphDiagnostics = Record<string, PipelineNodeDiagnostics>;
