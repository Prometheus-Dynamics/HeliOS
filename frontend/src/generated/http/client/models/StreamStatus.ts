/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { StreamState } from './StreamState';
export type StreamStatus = {
    disabled_reason?: string | null;
    disabled_since_ms?: number | null;
    recording_active?: boolean;
    recording_since_ms?: number | null;
    started_at_ms?: number | null;
    state?: StreamState;
};

