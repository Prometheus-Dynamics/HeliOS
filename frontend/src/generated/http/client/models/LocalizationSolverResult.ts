/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LocalizationPoseSpace } from './LocalizationPoseSpace';
import type { LocalizationSolverMode } from './LocalizationSolverMode';
import type { LocalizationSolverOutputs } from './LocalizationSolverOutputs';
export type LocalizationSolverResult = {
    errors?: Array<string>;
    id: string;
    mode: LocalizationSolverMode;
    name: string;
    outputSpaces: Array<LocalizationPoseSpace>;
    outputs: LocalizationSolverOutputs;
};

