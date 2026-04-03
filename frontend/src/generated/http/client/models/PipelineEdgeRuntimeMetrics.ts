/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type PipelineEdgeRuntimeMetrics = {
    average_payload_bytes?: number;
    average_wait_ms?: number;
    capacity?: number | null;
    current_depth?: number;
    current_queue_bytes?: number;
    dropped?: number;
    edge_index?: number;
    from_node_index?: number | null;
    from_node_label?: string | null;
    from_port?: string | null;
    gpu_downloads?: number;
    gpu_uploads?: number;
    last_sample_age_ms?: number | null;
    max_depth?: number;
    payload_bytes?: number;
    payload_count?: number;
    peak_queue_bytes?: number;
    policy?: string | null;
    to_node_index?: number | null;
    to_node_label?: string | null;
    to_port?: string | null;
    wait_sample_count?: number;
    window_size?: number;
};

