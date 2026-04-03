/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DeviceIdentity } from './DeviceIdentity';
import type { ProbedBackend } from './ProbedBackend';
/**
 * Unified device descriptor for probed backends.
 *
 * # Example
 * ```rust,ignore
 * use styx::prelude::*;
 *
 * for dev in probe_all() {
     * println!("{} backends: {}", dev.identity.display, dev.backends.len());
     * }
     * ```
     */
    export type ProbedDevice = {
        backends: Array<ProbedBackend>;
        identity: DeviceIdentity;
    };

