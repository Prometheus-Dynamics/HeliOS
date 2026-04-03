/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CaptureConfig } from './CaptureConfig';
import type { DeviceIdentity } from './DeviceIdentity';
import type { JsonWire } from './JsonWire';
import type { RequestedDecoderConfigSchema } from './RequestedDecoderConfigSchema';
import type { RequestedEncoderConfigSchema } from './RequestedEncoderConfigSchema';
import type { RigPose } from './RigPose';
import type { StreamCalibration } from './StreamCalibration';
import type { StreamPipelineBinding } from './StreamPipelineBinding';
import type { StreamPipelineLayout } from './StreamPipelineLayout';
import type { StreamPipelineWire } from './StreamPipelineWire';
import type { StreamRecordingMode } from './StreamRecordingMode';
export type StreamManifest = {
    /**
     * Active pipeline ID when `pipelines` is set.
     */
    active_pipeline_id?: string | null;
    /**
     * Selected host output port for the active pipeline when `pipelines` is set.
     */
    active_pipeline_output?: string | null;
    calibration?: (null | StreamCalibration);
    capture: CaptureConfig;
    decoder: RequestedDecoderConfigSchema;
    encoder: RequestedEncoderConfigSchema;
    host_buffer: number;
    identity: DeviceIdentity;
    /**
     * Internal streams are created by the system for tasks like benchmarking and should not
     * appear in user-facing stream lists / registration UX.
     */
    internal: boolean;
    /**
     * When set to `false`, force the stream to run without any pipeline graph (raw frames).
     */
    pipeline_enabled: boolean;
    /**
     * Persisted host-bridge input values applied to the active pipeline graph at startup/rebuild.
     *
     * Used for stream-level controls like ROI crop, crosshair, and ordering mode.
     */
    pipeline_host_inputs: Record<string, JsonWire>;
    pipeline_layout?: (null | StreamPipelineLayout);
    /**
     * Optional wiring between pipeline outputs and downstream pipeline inputs.
     */
    pipeline_wires: Array<StreamPipelineWire>;
    /**
     * Optional additional pipeline graphs to run in multiplex/debug view.
     */
    pipelines: Array<StreamPipelineBinding>;
    pose?: (null | RigPose);
    preview_jpeg_quality: number;
    /**
     * Recording mode contract for capture-last buffers and future recording policies.
     */
    recording_mode: StreamRecordingMode;
    schema_version: number;
    start_on_boot: boolean;
};

