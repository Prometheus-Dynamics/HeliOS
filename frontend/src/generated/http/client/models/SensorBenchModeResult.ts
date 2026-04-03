/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BenchCodecStatCpu } from './BenchCodecStatCpu';
import type { CpuSample } from './CpuSample';
import type { ModeId } from './ModeId';
export type SensorBenchModeResult = {
    baseline_cpu?: CpuSample;
    capture_avg_fps: number;
    decoders?: Array<BenchCodecStatCpu>;
    /**
     * For encoder benchmarks we pick a single decoder (if available) to provide RG24.
     */
    encoder_input_decoder?: string | null;
    encoders?: Array<BenchCodecStatCpu>;
    format: string;
    host_avg_fps: number;
    mode_id: ModeId;
    resolution: string;
};

