import type { XYPosition } from '@xyflow/system';
import type { PipelineDiagnostics, PipelineGraphPlan, PipelineRegistryEntry } from '$lib/types/pipeline';
import type { PipelineGraphStore } from '$lib/features/pipelines/graphStore';
import type { PipelineGraphTheme } from './editorUtils';
import type { PipelineGraphHeatmap } from './types';

export type GraphContextEvent =
  | {
      type: 'pane' | 'palette';
      position: XYPosition;
      flowPosition: XYPosition;
      nodeId?: string | null;
    }
  | {
      type: 'node';
      position: XYPosition;
      flowPosition: XYPosition;
      nodeId?: string | null;
      port?: string | null;
      direction?: 'input' | 'output';
    }
  | {
      type: 'port';
      position: XYPosition;
      flowPosition: XYPosition;
      nodeId: string;
      port: string;
      direction: 'input' | 'output';
    };

export type PipelineGraphEditorProps = {
  plan: PipelineGraphPlan;
  interactive?: boolean;
  height?: number;
  fluid?: boolean;
  className?: string;
  selectedEdgeId?: string | null;
  registryEntries?: PipelineRegistryEntry[];
  theme?: Partial<PipelineGraphTheme>;
  metricsHeatmap?: PipelineGraphHeatmap | null;
  heatmapMode?: boolean;
  gpuOverlayMode?: boolean;
  gpuOverlaySegments?: Array<{ id: number; nodes: string[] }> | null;
  diagnostics?: PipelineDiagnostics | null;
  syncInspector?: { enabled: boolean; focusNodeId?: string | null };
  runtimeWarnings?: Record<string, { message: string; at: number | null }>;
  searchQuery?: string;
  graphStore?: PipelineGraphStore | null;
};
