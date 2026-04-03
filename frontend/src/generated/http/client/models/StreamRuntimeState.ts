/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { StreamCaptureRuntimeState } from './StreamCaptureRuntimeState';
import type { StreamCodecChainRuntimeState } from './StreamCodecChainRuntimeState';
import type { StreamDemandRuntimeState } from './StreamDemandRuntimeState';
import type { StreamPipelineRuntimeState } from './StreamPipelineRuntimeState';
import type { StreamRecordingRuntimeState } from './StreamRecordingRuntimeState';
export type StreamRuntimeState = {
    capture?: StreamCaptureRuntimeState;
    codecs?: StreamCodecChainRuntimeState;
    demand?: StreamDemandRuntimeState;
    pipeline?: StreamPipelineRuntimeState;
    recording?: StreamRecordingRuntimeState;
};

