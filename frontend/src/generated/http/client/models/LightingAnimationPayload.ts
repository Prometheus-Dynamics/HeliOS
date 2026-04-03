/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LightingColorPayload } from './LightingColorPayload';
export type LightingAnimationPayload = ({
    kind: 'off';
} | {
    color: LightingColorPayload;
    kind: 'chase';
    speed_hz?: number;
} | {
    color: LightingColorPayload;
    high?: number;
    kind: 'pulse';
    low?: number;
    period_ms?: number;
} | {
    kind: 'rainbow';
    speed_hz?: number;
} | {
    high?: number;
    kind: 'breathing_rainbow';
    low?: number;
    period_ms?: number;
    speed_hz?: number;
});

