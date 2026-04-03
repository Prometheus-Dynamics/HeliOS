/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type PipelinePerfMetrics = {
    /**
     * Average branch instructions per graph run (rolling window).
     */
    average_branch_instructions?: number;
    /**
     * Average branch misses per graph run (rolling window).
     */
    average_branch_misses?: number;
    /**
     * Average cache misses per graph run (rolling window).
     */
    average_cache_misses?: number;
    last_sample_age_ms?: number | null;
    sample_count?: number;
    window_size?: number;
};

