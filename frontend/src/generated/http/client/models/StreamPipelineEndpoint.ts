/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * Endpoint in the multiplex pipeline wiring graph.
 *
 * This identifies a specific pipeline *instance* (pipeline ID + optional layout `output_key`)
 * and a port name on that instance.
 */
export type StreamPipelineEndpoint = {
    /**
     * Optional instance discriminator when a pipeline appears multiple times with different
     * layout `output_key` values.
     */
    output_key?: string | null;
    pipeline_id: string;
    /**
     * Port name on the pipeline instance. For pipeline frame input, this is typically `frame`.
     * For pipeline outputs, this can be omitted to use the instance's selected output port.
     */
    port?: string | null;
};

