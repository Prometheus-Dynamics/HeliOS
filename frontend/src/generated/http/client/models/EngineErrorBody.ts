/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { EngineErrorCode } from './EngineErrorCode';
export type EngineErrorBody = {
    code: string;
    engine_code?: (null | EngineErrorCode);
    error: string;
    retryable?: boolean | null;
};

