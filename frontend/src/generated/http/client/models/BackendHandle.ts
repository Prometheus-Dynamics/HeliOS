/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * Backend-specific handle used for configuration/streaming.
 *
 * # Example
 * ```rust,ignore
 * use styx::prelude::*;
 *
 * let dev = probe_all().into_iter().next().expect("device");
 * let handle = &dev.backends[0].handle;
 * println!("backend kind: {:?}", handle.kind());
 * ```
 */
export type BackendHandle = ({
    V4l2: {
        path: string;
    };
} | {
    Libcamera: {
        id: string;
    };
} | 'Virtual' | {
    Netcam: {
        fps: number;
        height: number;
        url: string;
        width: number;
    };
} | {
    File: {
        fps: number;
        loop_forever: boolean;
        paths: Array<string>;
    };
});

