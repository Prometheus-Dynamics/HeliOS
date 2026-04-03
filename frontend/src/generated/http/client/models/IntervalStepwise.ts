/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { Interval } from './Interval';
/**
 * Stepwise interval description (min/max/step).
 *
 * # Example
 * ```rust
 * use std::num::NonZeroU32;
 * use styx_core::prelude::{Interval, IntervalStepwise};
 *
 * let make = |n, d| Interval {
     * numerator: NonZeroU32::new(n).unwrap(),
     * denominator: NonZeroU32::new(d).unwrap(),
     * };
     * let stepwise = IntervalStepwise {
         * min: make(1, 60),
         * max: make(1, 30),
         * step: make(1, 30),
         * };
         * assert!(stepwise.contains(make(1, 30)));
         * ```
         */
        export type IntervalStepwise = {
            max: Interval;
            min: Interval;
            step: Interval;
        };

