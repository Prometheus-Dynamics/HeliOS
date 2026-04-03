/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SensorBenchmarkSummary } from './SensorBenchmarkSummary';
import type { SensorBenchModeResult } from './SensorBenchModeResult';
export type SensorBenchmarkResult = {
    modes?: Array<SensorBenchModeResult>;
    summary: SensorBenchmarkSummary;
    warnings?: Array<string>;
};

