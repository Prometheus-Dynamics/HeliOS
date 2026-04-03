/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ProcessMappingMetrics } from './ProcessMappingMetrics';
export type ProcessMemoryMetrics = {
    anonymous_pss_bytes: number;
    deleted_pss_bytes: number;
    device_pss_bytes: number;
    executable?: string | null;
    executable_file_bytes?: number | null;
    executable_pss_bytes: number;
    heap_pss_bytes: number;
    name: string;
    other_pss_bytes: number;
    pid: number;
    private_dirty_bytes: number;
    pss_bytes?: number | null;
    rss_bytes: number;
    shared_lib_pss_bytes: number;
    stack_pss_bytes: number;
    swap_bytes: number;
    threads: number;
    top_pss_mappings?: Array<ProcessMappingMetrics>;
};

