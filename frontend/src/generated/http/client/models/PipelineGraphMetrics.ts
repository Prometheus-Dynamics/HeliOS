/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PipelineFlamegraphMetrics } from './PipelineFlamegraphMetrics';
import type { PipelineImageWorkingSetMetrics } from './PipelineImageWorkingSetMetrics';
import type { PipelineNodeRuntimeMetrics } from './PipelineNodeRuntimeMetrics';
import type { PipelinePerfMetrics } from './PipelinePerfMetrics';
import type { PipelineSampleCacheMetrics } from './PipelineSampleCacheMetrics';
import type { PipelineTimingMetrics } from './PipelineTimingMetrics';
export type PipelineGraphMetrics = {
    edges?: any | null;
    executor?: (null | PipelineTimingMetrics);
    executor_lock?: (null | PipelineTimingMetrics);
    flamegraph?: (null | PipelineFlamegraphMetrics);
    /**
     * Optional aggregated node-group metrics (e.g. embedded graphs).
     */
    groups?: any | null;
    image_working_set?: (null | PipelineImageWorkingSetMetrics);
    nodes?: Record<string, PipelineNodeRuntimeMetrics>;
    output_materialization?: (null | PipelineTimingMetrics);
    perf?: (null | PipelinePerfMetrics);
    sample_cache?: (null | PipelineSampleCacheMetrics);
    warnings?: Array<string>;
    wrapper?: (null | PipelineTimingMetrics);
};

