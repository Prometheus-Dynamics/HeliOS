/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ColorSpace } from './ColorSpace';
import type { Resolution } from './Resolution';
/**
 * Media format including code and geometry.
 *
 * # Example
 * ```rust
 * use styx_core::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};
 *
 * let res = Resolution::new(1920, 1080).unwrap();
 * let fmt = MediaFormat::new(FourCc::new(*b"RG24"), res, ColorSpace::Srgb);
 * assert_eq!(fmt.code.to_string(), "RG24");
 * ```
 */
export type MediaFormat = {
    /**
     * FourCc code describing pixel layout.
     */
    code: string;
    /**
     * Color space hint.
     */
    color: ColorSpace;
    /**
     * Resolution of the frame.
     */
    resolution: Resolution;
};

