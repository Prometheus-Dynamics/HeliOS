/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SolveCalibrationRequest } from '../models/SolveCalibrationRequest';
import type { SolveCalibrationResponse } from '../models/SolveCalibrationResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class CalibrationService {
    /**
     * @returns SolveCalibrationResponse Solved camera intrinsics
     * @throws ApiError
     */
    public static solveCalibration({
        requestBody,
    }: {
        requestBody: SolveCalibrationRequest,
    }): CancelablePromise<SolveCalibrationResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/calibration/solve',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid input`,
                404: `Missing graph or media`,
                500: `Calibration solve failed`,
                502: `Engine unavailable`,
                504: `Calibration solve timed out`,
            },
        });
    }
}
