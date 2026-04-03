/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LocalizationPoseSpace } from './LocalizationPoseSpace';
import type { LocalizationSolverMode } from './LocalizationSolverMode';
export type LocalizationValidationConstraints = {
    defaultPollHz: number;
    maxMapUploadBytes: number;
    maxPollHz: number;
    minPollHz: number;
    pollStepHz: number;
    supportedPoseSpaces: Array<LocalizationPoseSpace>;
    supportedSolverModes: Array<LocalizationSolverMode>;
};

