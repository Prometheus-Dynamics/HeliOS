import type { PipelineDataType } from '$lib/types/pipeline';

export type BoundaryDraftPort = {
  id: string;
  name: string;
  dataTypeKey: string;
  originalName?: string | null;
};

export type BoundaryDraft = {
  direction: 'input' | 'output';
  nodeId: string | null;
  ports: BoundaryDraftPort[];
};

export type GroupDraft = {
  nodeId: string;
  name: string;
  summary: string;
  color: string;
};

export type GraphContextPort = {
  nodeId: string;
  port: string;
  direction: 'input' | 'output';
  dataType?: PipelineDataType | null;
};

export type GraphContextMenuState =
  | { visible: false; port?: null; nodeId?: null; flowPosition?: { x: number; y: number } | null }
  | {
      visible: true;
      mode: 'actions' | 'registry' | 'boundary' | 'group';
      position: { x: number; y: number };
      flowPosition: { x: number; y: number };
      nodeId: string | null;
      size: { width: number; height: number };
      boundary?: BoundaryDraft | null;
      group?: GroupDraft | null;
      port?: GraphContextPort | null;
    };

export type PipelineEventPayload = {
  type?: string;
  pipeline_id?: string;
};

export type PipelineBreadcrumb = {
  id: string;
  name: string;
  status?: 'embedded' | 'linked' | 'mismatch' | 'unresolved';
  targetId?: string | null;
};

export type InspectorTabKey = 'pipeline' | 'node' | 'connection' | 'boundary' | 'run' | 'metrics';
