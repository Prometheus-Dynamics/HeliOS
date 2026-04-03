/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LightingColorPayload } from './LightingColorPayload';
import type { LightingTimelineEasingPayload } from './LightingTimelineEasingPayload';
export type LightingTimelineKeyframePayload = {
    easing?: LightingTimelineEasingPayload;
    frame?: Array<LightingColorPayload>;
    time_ms?: number;
};

