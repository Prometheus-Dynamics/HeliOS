/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type CodecMetrics = {
    /**
     * Average inter-frame time at this stage (derived from throughput).
     */
    average_time_ms?: number;
    backpressure?: number;
    errors?: number;
    /**
     * Average throughput at this stage.
     */
    fps?: number;
    /**
     * Last observed inter-frame time at this stage (may be 0 if unknown).
     */
    last_time_ms?: number;
    processed?: number;
    sample_count?: number;
    /**
     * Average processing time spent inside this stage (work time), independent of downstream waits.
     */
    work_average_time_ms?: number;
    /**
     * Last processing time spent inside this stage (work time), independent of downstream waits.
     */
    work_last_time_ms?: number;
};

