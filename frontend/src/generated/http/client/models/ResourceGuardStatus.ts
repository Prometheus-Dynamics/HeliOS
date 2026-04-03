/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ResourceGuardAction } from './ResourceGuardAction';
import type { ResourceGuardDegradedStream } from './ResourceGuardDegradedStream';
export type ResourceGuardStatus = {
    cooldown_ms: number;
    degraded_streams?: Array<ResourceGuardDegradedStream>;
    enabled: boolean;
    last_action?: (null | ResourceGuardAction);
    last_mem_available_kb?: number | null;
    mem_low_kb: number;
    mem_recover_kb: number;
    poll_ms: number;
    pressure_active: boolean;
    recent_actions?: Array<ResourceGuardAction>;
};

