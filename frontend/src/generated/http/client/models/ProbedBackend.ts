/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BackendHandle } from './BackendHandle';
import type { BackendKind } from './BackendKind';
import type { CaptureDescriptor } from './CaptureDescriptor';
/**
 * Backend-specific entry for a probed device.
 *
 * # Example
 * ```rust,ignore
 * use styx::prelude::*;
 *
 * let dev = probe_all().into_iter().next().expect("device");
 * for backend in dev.backends {
     * println!("{:?}: {} modes", backend.kind, backend.descriptor.modes.len());
     * }
     * ```
     */
    export type ProbedBackend = {
        descriptor: CaptureDescriptor;
        handle: BackendHandle;
        kind: BackendKind;
        properties: Array<any[]>;
    };

