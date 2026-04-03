/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BackendHandle } from './BackendHandle';
import type { BackendKind } from './BackendKind';
import type { BenchModeRef } from './BenchModeRef';
import type { ControlAssignment } from './ControlAssignment';
export type BenchFormatsRequest = {
    backend: BackendKind;
    /**
     * Capture controls to apply during benchmark runs (e.g., exposure).
     */
    controls?: Array<ControlAssignment>;
    device_keys?: Array<string>;
    handle: BackendHandle;
    height: number;
    /**
     * Optional explicit mode list to benchmark (skips device probing on the server).
     *
     * When present, the server will benchmark exactly these mode ids and render each group using
     * the provided `format` (FOURCC) string.
     */
    modes?: Array<BenchModeRef>;
    /**
     * Stop conflicting streams while benchmarking, then restore them.
     */
    restore_existing?: boolean;
    sample_ms?: number;
    target_fps?: number;
    width: number;
};

