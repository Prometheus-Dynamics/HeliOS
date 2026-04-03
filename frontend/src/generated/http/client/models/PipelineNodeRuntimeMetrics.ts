/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PipelineNodeMetrics } from './PipelineNodeMetrics';
import type { PipelineNodePerfMetrics } from './PipelineNodePerfMetrics';
export type PipelineNodeRuntimeMetrics = {
    average_input_payload_bytes?: number;
    average_output_payload_bytes?: number;
    /**
     * Nested metrics for grouped nodes, keyed by node/group id.
     */
    children?: any | null;
    last_error?: string | null;
    last_error_at?: number | null;
    metrics: PipelineNodeMetrics;
    /**
     * Node index within the planned graph (`NodeRef.0`).
     */
    node_index?: number | null;
    /**
     * Optional node label from the planned graph.
     */
    node_label?: string | null;
    /**
     * Node type identifier (Daedalus node id), e.g. `cv:contour:contours`.
     */
    node_type?: string | null;
    peak_input_payload_bytes?: number;
    peak_output_payload_bytes?: number;
    peak_payload_working_set_bytes?: number;
    perf?: (null | PipelineNodePerfMetrics);
    retained_output_ports?: any | null;
    retained_output_sample_bytes?: number;
    retained_output_sample_count?: number;
};

