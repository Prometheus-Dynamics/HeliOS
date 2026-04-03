/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BenchCodecStat } from './BenchCodecStat';
export type BenchFormatGroup = {
    /**
     * Capture stage throughput for this format (context).
     */
    capture_avg_fps: number;
    decoders?: Array<BenchCodecStat>;
    encoders?: Array<BenchCodecStat>;
    format: string;
    host_avg_fps: number;
};

