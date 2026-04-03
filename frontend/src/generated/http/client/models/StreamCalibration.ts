/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LensModel } from './LensModel';
export type StreamCalibration = {
    cx: number;
    cy: number;
    fx: number;
    fy: number;
    k1: number;
    k2: number;
    k3: number;
    lensModel?: LensModel;
    p1: number;
    p2: number;
    undistortIters: number;
};

