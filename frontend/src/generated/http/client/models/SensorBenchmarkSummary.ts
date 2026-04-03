/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BackendKind } from './BackendKind';
export type SensorBenchmarkSummary = {
    backend: BackendKind;
    benchmark_id: string;
    canceled?: boolean;
    completed_at: string;
    device_keys: Array<string>;
    started_at: string;
};

