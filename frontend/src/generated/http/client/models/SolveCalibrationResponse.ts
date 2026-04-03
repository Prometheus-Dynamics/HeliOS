/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SolveCalibrationDebugView } from './SolveCalibrationDebugView';
import type { StreamCalibrationParams } from './StreamCalibrationParams';
export type SolveCalibrationResponse = {
    calibration: StreamCalibrationParams;
    debugViews?: Array<SolveCalibrationDebugView>;
    pointsUsed: number;
    reprojectionErrorPx: number;
    viewsUsed: number;
    warnings?: Array<string>;
};

