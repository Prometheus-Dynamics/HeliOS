/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LocalizationPoseSpace } from './LocalizationPoseSpace';
import type { LocalizationSolverMode } from './LocalizationSolverMode';
import type { LocalizationSolverRuntimeTuningConfig } from './LocalizationSolverRuntimeTuningConfig';
import type { LocalizationTemporalStabilizationConfig } from './LocalizationTemporalStabilizationConfig';
export type LocalizationSolverConfig = {
    color?: string | null;
    id: string;
    mode: LocalizationSolverMode;
    name: string;
    outputSpaces?: Array<LocalizationPoseSpace>;
    runtimeTuning?: LocalizationSolverRuntimeTuningConfig;
    sourceIds?: Array<string>;
    temporalStabilization?: (null | LocalizationTemporalStabilizationConfig);
};

