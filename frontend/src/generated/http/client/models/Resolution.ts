/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * Resolution of a frame.
 *
 * # Example
 * ```rust
 * use styx_core::prelude::Resolution;
 *
 * let res = Resolution::new(640, 480).unwrap();
 * assert_eq!(res.width.get(), 640);
 * ```
 */
export type Resolution = {
    /**
     * Height in pixels (non-zero).
     */
    height: number;
    /**
     * Width in pixels (non-zero).
     */
    width: number;
};

