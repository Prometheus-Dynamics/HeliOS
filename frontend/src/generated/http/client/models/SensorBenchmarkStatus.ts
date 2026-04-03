/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SensorBenchmarkProgress } from './SensorBenchmarkProgress';
import type { SensorBenchmarkResult } from './SensorBenchmarkResult';
import type { SensorBenchmarkSummary } from './SensorBenchmarkSummary';
export type SensorBenchmarkStatus = ({
    progress: SensorBenchmarkProgress;
    started_at: string;
    status: 'running';
} | {
    result: SensorBenchmarkResult;
    status: 'completed';
    summary: SensorBenchmarkSummary;
} | {
    error: string;
    started_at: string;
    status: 'failed';
});

