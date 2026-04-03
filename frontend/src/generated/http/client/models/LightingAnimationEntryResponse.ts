/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LightingAnimationPayload } from './LightingAnimationPayload';
import type { LightingTimelinePayload } from './LightingTimelinePayload';
export type LightingAnimationEntryResponse = {
    animation?: (null | LightingAnimationPayload);
    brightness?: number | null;
    duration_ms?: number | null;
    frame?: any[] | null;
    frames?: any[] | null;
    name: string;
    timeline?: (null | LightingTimelinePayload);
};

