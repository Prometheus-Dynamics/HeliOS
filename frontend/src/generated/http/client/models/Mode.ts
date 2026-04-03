/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { Interval } from './Interval';
import type { IntervalStepwise } from './IntervalStepwise';
import type { MediaFormat } from './MediaFormat';
import type { ModeId } from './ModeId';
/**
 * Descriptor for a single capture mode (format + intervals).
 *
 * # Example
 * ```rust
 * use styx_capture::prelude::*;
 *
 * let res = Resolution::new(320, 240).unwrap();
 * let format = MediaFormat::new(FourCc::new(*b"RG24"), res, ColorSpace::Srgb);
 * let mode = Mode {
     * id: ModeId { format, interval: None },
     * format,
     * intervals: smallvec::smallvec![],
     * interval_stepwise: None,
     * };
     * assert_eq!(mode.format.code.to_string(), "RG24");
     * ```
     */
    export type Mode = {
        /**
         * Media format associated with the mode.
         */
        format: MediaFormat;
        /**
         * Identifier (format + optional interval) for this mode.
         */
        id: ModeId;
        interval_stepwise?: (null | IntervalStepwise);
        /**
         * Supported frame intervals.
         */
        intervals: Array<Interval>;
    };

