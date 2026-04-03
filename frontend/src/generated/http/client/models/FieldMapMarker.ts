/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { FieldMapTagBits } from './FieldMapTagBits';
import type { FieldQuaternion } from './FieldQuaternion';
export type FieldMapMarker = {
    family: string;
    headingDeg?: number;
    id: number;
    position: Array<number>;
    quaternion: FieldQuaternion;
    sizeM: number;
    tagBits?: (null | FieldMapTagBits);
    unique?: boolean;
};

