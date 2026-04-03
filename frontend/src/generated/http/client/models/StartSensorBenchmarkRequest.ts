/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BackendHandle } from './BackendHandle';
import type { BackendKind } from './BackendKind';
import type { ControlAssignment } from './ControlAssignment';
import type { ModeId } from './ModeId';
export type StartSensorBenchmarkRequest = {
    backend: BackendKind;
    /**
     * Capture controls to apply during benchmark runs (e.g., exposure).
     */
    controls?: Array<ControlAssignment>;
    device_keys?: Array<string>;
    handle: BackendHandle;
    /**
     * Benchmark only these mode IDs (defaults to all modes for the backend).
     */
    mode_ids?: Array<ModeId>;
    /**
     * Stop conflicting streams while benchmarking, then restore them.
     */
    restore_existing?: boolean;
    /**
     * Number of frames to wait per sampling window (per mode + per codec).
     *
     * If unset, `sample_ms` determines the sampling window.
     */
    sample_frames?: number | null;
    sample_ms?: number;
    /**
     * Max time to wait per sampling window (per mode + per codec), in milliseconds.
     *
     * If unset, `sample_ms` determines the sampling window.
     */
    sample_timeout_ms?: number | null;
    target_fps?: number;
};

