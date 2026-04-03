/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ResourceGuardActionKind } from './ResourceGuardActionKind';
export type ResourceGuardAction = {
    alias?: string | null;
    at_ms: number;
    kind: ResourceGuardActionKind;
    mem_available_kb?: number | null;
    reason: string;
    score: number;
    stream_id: string;
};

