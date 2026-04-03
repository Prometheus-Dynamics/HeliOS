/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LimelightAdapterStatus } from './LimelightAdapterStatus';
export type LimelightAdapterRegistryStatus = {
    adapter_count: number;
    adapters?: Array<LimelightAdapterStatus>;
    emulate_enabled: boolean;
    last_error?: string | null;
    publish_phase: string;
    publish_ready: boolean;
    updated_at_ms: number;
};

