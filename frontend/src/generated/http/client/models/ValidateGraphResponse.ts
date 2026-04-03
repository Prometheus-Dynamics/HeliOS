/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { GpuEdgeBufferInfo } from './GpuEdgeBufferInfo';
import type { GpuSegment } from './GpuSegment';
import type { PlannerDiagnostic } from './PlannerDiagnostic';
export type ValidateGraphResponse = {
    diagnostics: Array<PlannerDiagnostic>;
    gpu_edges?: Array<GpuEdgeBufferInfo>;
    gpu_segments?: Array<GpuSegment>;
    node_ids?: Array<string>;
    ok: boolean;
};

