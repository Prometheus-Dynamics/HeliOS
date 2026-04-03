/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ResolvedStreamConfig } from './ResolvedStreamConfig';
import type { StreamManifest } from './StreamManifest';
import type { ValidationWarning } from './ValidationWarning';
export type StreamValidateResponse = {
    manifest: StreamManifest;
    resolved: ResolvedStreamConfig;
    warnings?: Array<ValidationWarning>;
};

