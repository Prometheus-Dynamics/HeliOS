/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CaptureStageMetrics } from './CaptureStageMetrics';
import type { CodecMetrics } from './CodecMetrics';
import type { PipelineGraphMetrics } from './PipelineGraphMetrics';
import type { StreamEncoderDemandMetrics } from './StreamEncoderDemandMetrics';
import type { StreamFrameDemandMetrics } from './StreamFrameDemandMetrics';
import type { StreamMemoryMetrics } from './StreamMemoryMetrics';
import type { StreamPreviewTransportMetrics } from './StreamPreviewTransportMetrics';
export type StreamMetrics = {
    capture: CaptureStageMetrics;
    decoder?: (null | CodecMetrics);
    encoder?: (null | CodecMetrics);
    encoder_demand?: (null | StreamEncoderDemandMetrics);
    frame_demand?: (null | StreamFrameDemandMetrics);
    graph_stage: CaptureStageMetrics;
    host: CaptureStageMetrics;
    memory?: (null | StreamMemoryMetrics);
    pipeline?: (null | PipelineGraphMetrics);
    pipeline_instances?: any | null;
    preview_transport?: (null | StreamPreviewTransportMetrics);
};

