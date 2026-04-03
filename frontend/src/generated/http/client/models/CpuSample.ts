/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type CpuSample = {
    /**
     * Average `helios-engine` CPU usage for this run (sysinfo `%`, can exceed 100 on multi-core).
     */
    engine_cpu_avg?: number | null;
    /**
     * Average total CPU usage for this run (sysinfo `%`).
     */
    system_cpu_avg?: number | null;
};

