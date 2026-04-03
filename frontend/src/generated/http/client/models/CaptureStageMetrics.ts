/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type CaptureStageMetrics = {
    /**
     * Throughput-derived cadence for this stage (`1000 / fps`).
     */
    average_time_ms?: number;
    fps?: number;
    /**
     * Last observed cadence for this stage. Kept aligned with `average_time_ms`.
     */
    last_time_ms?: number;
    sample_count?: number;
    /**
     * Actual stage work time over the rolling window.
     */
    work_average_time_ms?: number;
    /**
     * Actual work time for the last completed sample.
     */
    work_last_time_ms?: number;
};

