/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { StreamPipelineEndpoint } from './StreamPipelineEndpoint';
/**
 * Wire a port from one pipeline instance into an input port on another pipeline instance.
 */
export type StreamPipelineWire = {
    from: StreamPipelineEndpoint;
    to: StreamPipelineEndpoint;
};

