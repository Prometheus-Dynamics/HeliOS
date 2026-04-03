/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { StreamCaptureState } from './StreamCaptureState';
export type StreamCaptureRuntimeState = {
    capture_fourcc?: string | null;
    disabled_reason?: string | null;
    disabled_since_ms?: number | null;
    started_at_ms?: number | null;
    state?: StreamCaptureState;
};

