/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DependenciesPayload } from './DependenciesPayload';
import type { FeaturesPayload } from './FeaturesPayload';
export type HealthPayload = {
    dependencies: DependenciesPayload;
    features: FeaturesPayload;
    ok: boolean;
    server_time_ms: number;
    uptime_ms: number;
    version: string;
};

