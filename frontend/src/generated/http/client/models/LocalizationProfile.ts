/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LocalizationFieldOriginConfig } from './LocalizationFieldOriginConfig';
import type { LocalizationSolverConfig } from './LocalizationSolverConfig';
import type { LocalizationSourceConfig } from './LocalizationSourceConfig';
import type { LocalizationTemporalStabilizationConfig } from './LocalizationTemporalStabilizationConfig';
export type LocalizationProfile = {
    allowedTagIds?: Array<number>;
    color?: string | null;
    enabled?: boolean;
    excludedTagIds?: Array<number>;
    fieldMapId?: string | null;
    fieldOrigin?: LocalizationFieldOriginConfig;
    id: string;
    name: string;
    /**
     * Snap field-space pitch to level (0 deg).
     */
    snapPitchToGround?: boolean;
    /**
     * Snap field-space roll to level (0 deg).
     */
    snapRollToGround?: boolean;
    /**
     * Snap the "height" axis to the ground plane when reporting field-space poses.
     *
     * Note: in the viewer/three.js frame, +Y is up. This option name uses "Z" to match
     * common robotics conventions (e.g. WPILib) where Z is up.
     */
    snapZToGround?: boolean;
    solvers?: Array<LocalizationSolverConfig>;
    sources?: Array<LocalizationSourceConfig>;
    tagSizeM?: number | null;
    temporalStabilization?: LocalizationTemporalStabilizationConfig;
    viewEnabled?: boolean;
};

