/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { FanMode } from './FanMode';
export type FanStatus = {
    last_error?: string | null;
    mode?: (null | FanMode);
    path_in_use?: string | null;
    present?: boolean;
    rpm?: number | null;
    target_percent?: number | null;
    temperature_c?: number | null;
    updated_at_ms?: number | null;
};

