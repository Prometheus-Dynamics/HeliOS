/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { StreamDemandPipelineRuntimeState } from './StreamDemandPipelineRuntimeState';
import type { StreamEncoderDemandMetrics } from './StreamEncoderDemandMetrics';
import type { StreamFrameDemandMetrics } from './StreamFrameDemandMetrics';
import type { StreamGraphDemandRuntimeState } from './StreamGraphDemandRuntimeState';
import type { StreamRecordingDemandRuntimeState } from './StreamRecordingDemandRuntimeState';
import type { StreamViewerDemandRuntimeState } from './StreamViewerDemandRuntimeState';
export type StreamDemandRuntimeState = {
    encoder?: StreamEncoderDemandMetrics;
    frame?: StreamFrameDemandMetrics;
    graph?: StreamGraphDemandRuntimeState;
    live_active?: boolean;
    pipeline?: StreamDemandPipelineRuntimeState;
    recording?: StreamRecordingDemandRuntimeState;
    viewers?: StreamViewerDemandRuntimeState;
};

