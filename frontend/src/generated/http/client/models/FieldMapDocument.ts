/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { FieldMapMarker } from './FieldMapMarker';
import type { FieldMapOverlay } from './FieldMapOverlay';
import type { FieldMapSource } from './FieldMapSource';
export type FieldMapDocument = {
    depthM: number;
    id: string;
    markers: Array<FieldMapMarker>;
    name: string;
    overlay?: (null | FieldMapOverlay);
    schemaVersion: number;
    source: FieldMapSource;
    widthM: number;
};

