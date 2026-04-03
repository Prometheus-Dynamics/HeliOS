/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ControlMeta } from './ControlMeta';
import type { Mode } from './Mode';
/**
 * Descriptor for a capture device/source.
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
     * let descriptor = CaptureDescriptor { modes: vec![mode], controls: Vec::new() };
     * assert_eq!(descriptor.modes.len(), 1);
     * ```
     */
    export type CaptureDescriptor = {
        /**
         * Supported controls.
         */
        controls: Array<ControlMeta>;
        /**
         * Supported modes.
         */
        modes: Array<Mode>;
    };

