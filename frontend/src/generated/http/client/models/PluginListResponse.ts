/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PluginCompatibility } from './PluginCompatibility';
import type { PluginFile } from './PluginFile';
export type PluginListResponse = {
    compatibility: Array<PluginCompatibility>;
    disabled: Array<PluginFile>;
    engine_available: boolean;
    installed: Array<PluginFile>;
    uploads: Array<PluginFile>;
};

