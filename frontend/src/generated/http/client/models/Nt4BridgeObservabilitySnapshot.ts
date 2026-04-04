/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type Nt4BridgeObservabilitySnapshot = {
    active_target?: string | null;
    last_entry_id?: number | null;
    last_publish_success_ms?: number | null;
    publish_cycles: number;
    publish_failures: number;
    reconnects: number;
    skipped_ticks: number;
};

