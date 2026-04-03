/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ReadModelFreshnessReason } from './ReadModelFreshnessReason';
import type { ReadModelFreshnessState } from './ReadModelFreshnessState';
export type ReadModelFreshness = {
    last_success_at_ms?: number | null;
    observed_at_ms: number;
    reason: ReadModelFreshnessReason;
    state: ReadModelFreshnessState;
};

