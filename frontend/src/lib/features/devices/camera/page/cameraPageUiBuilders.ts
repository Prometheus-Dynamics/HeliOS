import type { StreamInfo, StreamManifest, StreamMetrics } from '$lib/api/client';

export type CameraPageCoreInput = {
  loading: boolean;
  refreshing: boolean;
  stopping: boolean;
  error: string | null;
  stream: StreamInfo | null;
  manifestState: StreamManifest | null;
  streamMetrics: StreamMetrics | null;
  controls: unknown[];
  controlState: Record<number, number | boolean | null>;
  controlAppliedState: Record<number, number | boolean | null>;
  controlBusy: Record<number, boolean>;
  streamLookupDebug: string | null;
  streamApiBase: string | null;
  descriptor: { modes: unknown[]; controls: unknown[] };
  currentCalibrationParams: unknown;
  streamViewerHost: HTMLDivElement | null;
  streamViewerBounds: { width: number; height: number };
  streamId: string;
  data: unknown;
  streamState: unknown;
  pipelineState: unknown;
  calibrationState: unknown;
};

export type CameraPageDerivedInput = {
  activeMode: unknown;
  streamViewerAspect: number;
  streamViewerFit: { width: number; height: number };
  encoderSettingsAvailable: boolean;
  tabs: Array<{ id: string; label: string; ready?: boolean; icon?: unknown; hidden?: boolean }>;
};

export const buildCameraPageCore = (input: CameraPageCoreInput) => ({
  loading: input.loading,
  refreshing: input.refreshing,
  stopping: input.stopping,
  error: input.error,
  stream: input.stream,
  manifestState: input.manifestState,
  streamMetrics: input.streamMetrics,
  controls: input.controls,
  controlState: input.controlState,
  controlAppliedState: input.controlAppliedState,
  controlBusy: input.controlBusy,
  streamLookupDebug: input.streamLookupDebug,
  streamApiBase: input.streamApiBase,
  descriptor: input.descriptor,
  currentCalibrationParams: input.currentCalibrationParams,
  streamViewerHost: input.streamViewerHost,
  streamViewerBounds: input.streamViewerBounds,
  streamId: input.streamId,
  data: input.data,
  streamState: input.streamState,
  pipelineState: input.pipelineState,
  calibrationState: input.calibrationState
});

export const buildCameraPageDerived = (input: CameraPageDerivedInput) => ({
  activeMode: input.activeMode,
  streamViewerAspect: input.streamViewerAspect,
  streamViewerFit: input.streamViewerFit,
  encoderSettingsAvailable: input.encoderSettingsAvailable,
  tabs: input.tabs
});

export const buildCameraPageUi = (input: Record<string, unknown>) => ({ ...input });

export const buildCameraPageConstants = (input: Record<string, unknown>) => ({ ...input });
