/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type ErrorHistoryEntry = {
    code: string;
    details?: string | null;
    error: string;
    id: string;
    operation?: string | null;
    remediation?: string | null;
    reported_by?: string | null;
    request_id?: string | null;
    retryable?: boolean | null;
    source?: string | null;
    status?: number | null;
    timestamp_ms: number;
    trace_id?: string | null;
    transport?: string | null;
};

