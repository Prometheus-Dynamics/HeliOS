/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BackendHandle } from './BackendHandle';
import type { BackendKind } from './BackendKind';
import type { CaptureDeviceIdentity } from './CaptureDeviceIdentity';
import type { ControlAssignment } from './ControlAssignment';
import type { Interval } from './Interval';
import type { ModeId } from './ModeId';
export type CaptureConfig = {
    backend: BackendKind;
    controls?: Array<ControlAssignment>;
    device_identity?: (null | CaptureDeviceIdentity);
    device_keys?: Array<string>;
    enable_tdn_output?: boolean;
    handle: BackendHandle;
    interval?: (null | Interval);
    mode: ModeId;
    /**
     * Target FPS request used by the engine for pacing/metrics and libcamera frame timing.
     *
     * When `interval` is not explicitly set, `target_fps` is translated into a frame interval for
     * all backends, including libcamera. The live OV9782 path can sustain higher rates than the
     * default 33.3 ms cadence, and treating `target_fps` as a no-op leaves the sensor parked at
     * ~30 fps even when the requested stream target is higher.
     */
    target_fps?: number | null;
};

