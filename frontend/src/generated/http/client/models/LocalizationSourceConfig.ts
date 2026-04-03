/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LocalizationPoseSpace } from './LocalizationPoseSpace';
export type LocalizationSourceConfig = {
    cameraUid: string;
    enabled?: boolean;
    id: string;
    inputKey?: string | null;
    outputKey: string;
    poseSpace?: (null | LocalizationPoseSpace);
    streamId: string;
    weight?: number;
};

