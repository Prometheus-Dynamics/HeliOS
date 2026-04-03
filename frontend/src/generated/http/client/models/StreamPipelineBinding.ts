/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { JsonWire } from './JsonWire';
export type StreamPipelineBinding = {
    pipeline_graph?: (null | JsonWire);
    pipeline_id: string;
    /**
     * Optional host-bridge output port to use as the pipeline's frame output.
     */
    pipeline_output?: string | null;
    pipeline_patch?: (null | JsonWire);
};

