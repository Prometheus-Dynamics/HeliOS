/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApiRuntimeMetrics } from './ApiRuntimeMetrics';
import type { CpuCoreMetrics } from './CpuCoreMetrics';
import type { DeviceHealthIssue } from './DeviceHealthIssue';
import type { DiskMetrics } from './DiskMetrics';
import type { ProcessMemoryMetrics } from './ProcessMemoryMetrics';
import type { TempReading } from './TempReading';
export type DeviceMetrics = {
    api?: (null | ApiRuntimeMetrics);
    cpu_avg_pct: number;
    cpu_freq_mhz: number;
    cpus: Array<CpuCoreMetrics>;
    disks: Array<DiskMetrics>;
    issues?: Array<DeviceHealthIssue>;
    mem_total_bytes: number;
    mem_used_bytes: number;
    processes?: Array<ProcessMemoryMetrics>;
    status: string;
    swap_total_bytes: number;
    swap_used_bytes: number;
    temps: Array<TempReading>;
};

