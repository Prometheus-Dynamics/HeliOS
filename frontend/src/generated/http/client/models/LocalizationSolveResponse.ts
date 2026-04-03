/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LocalizationSolverResult } from './LocalizationSolverResult';
import type { LocalizationSolveTimings } from './LocalizationSolveTimings';
import type { LocalizationSourceSampleStatus } from './LocalizationSourceSampleStatus';
export type LocalizationSolveResponse = {
    profileId: string;
    solvers: Array<LocalizationSolverResult>;
    sources: Array<LocalizationSourceSampleStatus>;
    timings?: LocalizationSolveTimings;
};

