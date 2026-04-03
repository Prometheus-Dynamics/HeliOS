/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ArucoBitGrid } from './ArucoBitGrid';
import type { LocalizationPose } from './LocalizationPose';
export type LocalizationDetectionPose = {
    cameraUid: string;
    codeRotation?: number | null;
    pose: LocalizationPose;
    quality?: number;
    sourceId: string;
    tagBits?: (null | ArucoBitGrid);
    tagId: number;
    tagSize?: number | null;
    weight?: number;
};

