/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { Interval } from './Interval';
import type { MediaFormat } from './MediaFormat';
/**
 * Identifier for a capture mode keyed by its format and optional interval.
 *
 * # Example
 * ```rust
 * use styx_capture::prelude::*;
 *
 * let res = Resolution::new(640, 480).unwrap();
 * let format = MediaFormat::new(FourCc::new(*b"RG24"), res, ColorSpace::Srgb);
 * let id = ModeId { format, interval: None };
 * assert_eq!(id.format.code.to_string(), "RG24");
 * ```
 */
export type ModeId = {
    /**
     * Pixel format and resolution for this mode.
     */
    format: MediaFormat;
    interval?: (null | Interval);
};

