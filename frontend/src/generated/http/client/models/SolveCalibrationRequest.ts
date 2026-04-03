/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CalibrationSolveConfig } from './CalibrationSolveConfig';
import type { SolveCalibrationBoard } from './SolveCalibrationBoard';
export type SolveCalibrationRequest = {
    board: SolveCalibrationBoard;
    config?: (null | CalibrationSolveConfig);
    detectionsPort?: string | null;
    graph?: any;
    graphId?: string | null;
    graphTemplateId?: string | null;
    images: Array<string>;
    includeOverlays?: boolean | null;
    overlayPort?: string | null;
};

