/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { RecordingSource } from './RecordingSource';
export type StartRecordingRequest = {
    bitrate_bps?: number | null;
    codec?: string | null;
    container?: string | null;
    duration_ms?: number | null;
    fps?: number | null;
    gop?: number | null;
    imu_interval_ms?: number | null;
    include_imu?: boolean | null;
    max_height?: number | null;
    max_width?: number | null;
    name?: string | null;
    quality?: number | null;
    source?: (null | RecordingSource);
};

