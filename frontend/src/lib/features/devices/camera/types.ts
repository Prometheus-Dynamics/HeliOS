import type { PipelineOverviewEntry } from '$lib/types/pipeline-api';
import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';

export type PipelinePreviewSource = {
  pipelineId: string;
  pipelineName: string;
  pipelineAlias: string | null;
  outputKey: string;
  outputLabel: string;
  typeLabel: string | null;
  supportsMultiplex: boolean;
};

export type CalibrationPose = {
  x: number;
  y: number;
  z: number;
  roll: number;
  pitch: number;
  yaw: number;
};

export type PipelineNodeParameterDescriptor = {
  nodeId: string;
  nodeLabel: string;
  portKey: string;
  portLabel: string;
  dataType: PipelineDataType | null;
  defaultValue: PipelineNodeValue | null;
  pipelineOverrideValue: PipelineNodeValue | null;
  settable: boolean;
};

export type PipelineInputDescriptor = {
  key: string;
  canonicalKey: string;
  dataType: PipelineDataType | null;
  defaultValue: PipelineNodeValue | null;
};

export type PipelineOption = PipelineOverviewEntry & {
  __outputs?: Array<{ key: string; dataType: PipelineDataType | null }>;
  __inputs?: PipelineInputDescriptor[];
  __nodeParameters?: PipelineNodeParameterDescriptor[];
};

export type PipelineInputRange = { min: number; max: number; step: number };

export type MultiplexSlotState = {
  slotId: string;
  pipelineId: string | null;
  outputKey: string | null;
  row: number;
  column: number;
  rowSpan: number;
  columnSpan: number;
};

export type MultiplexGridState = {
  columns: number;
  rows: number;
  slots: MultiplexSlotState[];
};

export type PipelinesController = Record<string, unknown>;
