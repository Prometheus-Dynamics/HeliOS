/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LensModel } from './LensModel';
export type CalibrationSolveConfig = {
    lensModel?: LensModel;
    minPointsPerView?: number;
    minViews?: number;
    refineDistortion?: boolean;
    refineUndistortIters?: number;
    undistortIters?: number;
};

