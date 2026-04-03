/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LightingAnimationPayload } from './LightingAnimationPayload';
import type { LightingTimelinePayload } from './LightingTimelinePayload';
export type LightingAnimationSaveRequest = {
    animation?: (null | LightingAnimationPayload);
    brightness?: number | null;
    duration_ms?: number | null;
    frame?: any[] | null;
    frames?: any[] | null;
    name: string;
    requested_by?: string | null;
    timeline?: (null | LightingTimelinePayload);
};

