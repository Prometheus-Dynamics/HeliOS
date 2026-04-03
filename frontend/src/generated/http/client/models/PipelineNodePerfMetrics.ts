/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type PipelineNodePerfMetrics = {
    /**
     * Average branch instructions per node call (rolling window).
     */
    average_branch_instructions?: number;
    /**
     * Average branch misses per node call (rolling window).
     */
    average_branch_misses?: number;
    /**
     * Average cache misses per node call (rolling window).
     */
    average_cache_misses?: number;
    last_sample_age_ms?: number | null;
    sample_count?: number;
    window_size?: number;
};

