/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CaptureConfig } from './CaptureConfig';
import type { DeviceIdentity } from './DeviceIdentity';
import type { JsonWire } from './JsonWire';
import type { ResolvedDecoderConfig } from './ResolvedDecoderConfig';
import type { ResolvedEncoderConfig } from './ResolvedEncoderConfig';
import type { RigPose } from './RigPose';
import type { StreamCalibration } from './StreamCalibration';
import type { StreamPipelineBinding } from './StreamPipelineBinding';
import type { StreamPipelineLayout } from './StreamPipelineLayout';
import type { StreamPipelineWire } from './StreamPipelineWire';
import type { StreamRecordingMode } from './StreamRecordingMode';
export type ResolvedStreamConfig = {
    activePipelineId?: string | null;
    activePipelineOutput?: string | null;
    calibration?: (null | StreamCalibration);
    capture: CaptureConfig;
    decoder?: ResolvedDecoderConfig;
    encoder?: ResolvedEncoderConfig;
    hostBuffer: number;
    identity: DeviceIdentity;
    internal: boolean;
    pipelineEnabled: boolean;
    pipelineHostInputs?: Record<string, JsonWire>;
    pipelineLayout?: (null | StreamPipelineLayout);
    pipelineWires?: Array<StreamPipelineWire>;
    pipelines?: Array<StreamPipelineBinding>;
    pose?: (null | RigPose);
    previewJpegQuality: number;
    recordingMode?: StreamRecordingMode;
    startOnBoot: boolean;
};

