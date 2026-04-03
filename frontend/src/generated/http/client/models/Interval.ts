/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * Frame interval (fps) expressed as a rational.
 *
 * # Example
 * ```rust
 * use std::num::NonZeroU32;
 * use styx_core::prelude::Interval;
 *
 * let interval = Interval {
     * numerator: NonZeroU32::new(1).unwrap(),
     * denominator: NonZeroU32::new(30).unwrap(),
     * };
     * assert!(interval.fps() > 0.0);
     * ```
     */
    export type Interval = {
        /**
         * Denominator of the fps rational.
         */
        denominator: number;
        /**
         * Numerator of the fps rational.
         */
        numerator: number;
    };

